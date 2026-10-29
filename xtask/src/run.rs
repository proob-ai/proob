//! The one effectful module: spawns the steps `plan` produced, argv only and
//! never through a shell, prints the exact command line first, and renders
//! `ci`'s summary.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::iter::once;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cli::Task;
use crate::commit;
use crate::error::Error;
use crate::plan::{self, CI_ORDER, EnvChange, Exec, Expect, NO_GH_DIR, Step};
use crate::tools::{Tools, find_on_path};

pub struct Context {
    pub root: PathBuf,
    pub tools: Tools,
}

struct Outcome {
    name: &'static str,
    result: Result<(), String>,
}

pub fn task(ctx: &Context, task: &Task) -> Result<(), Error> {
    plan::plan(task, &ctx.tools)?.iter().try_for_each(|step| run_step(&ctx.root, step))
}

/// Every task of `CI_ORDER`, continuing past failures; the summary says which failed.
pub fn ci(ctx: &Context) -> Result<(), Error> {
    let outcomes: Vec<Outcome> = CI_ORDER.iter().map(|t| run_named(ctx, t)).collect();
    print_summary(&outcomes);
    match outcomes.iter().filter(|o| o.result.is_err()).count() {
        0 => Ok(()),
        failed => Err(Error::CiFailed(failed)),
    }
}

pub fn dry_run(ctx: &Context) -> Result<(), Error> {
    println!("ci plan (dry run), working directory {}", ctx.root.display());
    CI_ORDER.iter().for_each(|t| print_plan(ctx, t));
    Ok(())
}

fn run_named(ctx: &Context, t: &Task) -> Outcome {
    println!("== {} ==", t.name());
    Outcome { name: t.name(), result: task(ctx, t).map_err(|e| e.to_string()) }
}

fn print_plan(ctx: &Context, t: &Task) {
    println!("== {} ==", t.name());
    match plan::plan(t, &ctx.tools) {
        Ok(steps) => steps.iter().for_each(|step| println!("{}", describe(step))),
        Err(error) => println!("cannot run: {error}"),
    }
}

fn print_summary(outcomes: &[Outcome]) {
    println!("\nci summary");
    outcomes.iter().for_each(|o| println!("  {:<16} {}", o.name, verdict(&o.result)));
}

fn verdict(result: &Result<(), String>) -> String {
    match result {
        Ok(()) => "ok".to_owned(),
        Err(why) => format!("failed ({why})"),
    }
}

fn run_step(root: &Path, step: &Step) -> Result<(), Error> {
    println!("{}", describe(step));
    match step {
        Step::Exec(exec) => run_exec(root, exec),
        Step::Capture { exec, expect } => run_capture(root, exec, expect),
        Step::EnvUnset(names) => check_unset(names),
        Step::LineCount(dir) => count_lines(root, dir),
        Step::CommitLint(range) => lint_range(root, range),
        Step::TitleLint(title) => report_lint(&[Linted::of("title", title, commit::lint_subject(title))]),
    }
}

/// One subject and its problems; it passes when there are none.
struct Linted {
    label: String,
    subject: String,
    problems: Vec<String>,
}

impl Linted {
    fn of(label: &str, subject: &str, problems: Vec<String>) -> Linted {
        Linted { label: label.to_owned(), subject: subject.to_owned(), problems }
    }
}

/// The subjects of `range`, oldest first, through `git log`.
fn lint_range(root: &Path, range: &str) -> Result<(), Error> {
    let exec = command_of("git", &["log", "--reverse", "--format=%H%x1f%P%x1f%s", range]);
    let output = command(root, &exec)?.stderr(Stdio::inherit()).output().map_err(|e| spawn_error(&exec, e))?;
    if !output.status.success() {
        return Err(Error::Failed { command: command_line(&exec), code: output.status.code() });
    }
    let linted: Vec<Linted> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(commit::parse_log_line)
        .map(|c| Linted::of(&c.id[..c.id.len().min(10)], &c.subject, commit::lint_commit(&c)))
        .collect();
    println!("  {} commit(s) in {range}", linted.len());
    report_lint(&linted)
}

fn command_of(program: &str, args: &[&str]) -> Exec {
    Exec { program: program.to_owned(), args: args.iter().map(|a| (*a).to_owned()).collect(), env: Vec::new() }
}

/// Prints each failing subject with its problems; fails when there is any.
fn report_lint(linted: &[Linted]) -> Result<(), Error> {
    let failing: Vec<&Linted> = linted.iter().filter(|l| !l.problems.is_empty()).collect();
    failing.iter().for_each(|l| {
        println!("  {} {}", l.label, l.subject);
        l.problems.iter().for_each(|p| println!("    - {p}"));
    });
    match failing.len() {
        0 => Ok(()),
        n => Err(Error::Lint(n)),
    }
}

fn run_exec(root: &Path, exec: &Exec) -> Result<(), Error> {
    let status = command(root, exec)?.status().map_err(|source| spawn_error(exec, source))?;
    match status.success() {
        true => Ok(()),
        false => Err(Error::Failed { command: command_line(exec), code: status.code() }),
    }
}

fn run_capture(root: &Path, exec: &Exec, expect: &[Expect]) -> Result<(), Error> {
    let mut cmd = command(root, exec)?;
    cmd.stderr(Stdio::inherit());
    let output = cmd.output().map_err(|source| spawn_error(exec, source))?;
    let text = String::from_utf8_lossy(&output.stdout);
    match output.status.success() {
        true => expect.iter().try_for_each(|e| check(e, &text, exec)),
        false => Err(Error::Failed { command: command_line(exec), code: output.status.code() }),
    }
}

fn check(expect: &Expect, text: &str, exec: &Exec) -> Result<(), Error> {
    match expect.holds(text) {
        true => {
            println!("  ok: {expect}");
            Ok(())
        }
        false => {
            text.lines().take(40).for_each(|line| println!("  | {line}"));
            Err(Error::Expectation { command: command_line(exec), expect: expect.to_string() })
        }
    }
}

fn check_unset(names: &[&'static str]) -> Result<(), Error> {
    let present = names.iter().find(|name| env::var_os(name).is_some_and(|v| !v.is_empty()));
    match present {
        Some(name) => Err(Error::TokenPresent(name)),
        None => Ok(()),
    }
}

fn count_lines(root: &Path, dir: &str) -> Result<(), Error> {
    let counts = rust_files(&root.join(dir))?
        .into_iter()
        .map(|file| line_count(&file).map(|n| (file, n)))
        .collect::<Result<Vec<_>, Error>>()?;
    counts.iter().for_each(|(file, n)| println!("{n:>8} {dir}/{}", file_name(file)));
    if counts.len() > 1 {
        println!("{:>8} total", counts.iter().map(|(_, n)| n).sum::<usize>());
    }
    Ok(())
}

/// The `.rs` files directly in `dir`, sorted by name.
fn rust_files(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let entries = fs::read_dir(dir).map_err(|source| Error::Io { path: dir.to_path_buf(), source })?;
    let sorted: BTreeSet<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    Ok(sorted.into_iter().collect())
}

/// Newlines, as `wc -l` counts them.
fn line_count(file: &Path) -> Result<usize, Error> {
    fs::read_to_string(file)
        .map(|text| text.matches('\n').count())
        .map_err(|source| Error::Io { path: file.to_path_buf(), source })
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn command(root: &Path, exec: &Exec) -> Result<Command, Error> {
    let mut cmd = Command::new(resolve(root, &exec.program));
    cmd.args(&exec.args).current_dir(root);
    exec.env.iter().try_for_each(|change| apply(root, &mut cmd, change))?;
    Ok(cmd)
}

/// A program with a `/` is a path relative to the workspace root (std leaves a
/// relative program ambiguous under `current_dir`); a bare name is looked up on PATH.
fn resolve(root: &Path, program: &str) -> PathBuf {
    match program.contains('/') {
        true => root.join(program),
        false => PathBuf::from(program),
    }
}

fn apply(root: &Path, cmd: &mut Command, change: &EnvChange) -> Result<(), Error> {
    match change {
        EnvChange::PrependPath(dir) => prepend_path(cmd, dir),
        EnvChange::NoGitHubToken => prepend_path(cmd, &no_gh_dir(root)?),
    }
}

fn prepend_path(cmd: &mut Command, dir: &Path) -> Result<(), Error> {
    let current = env::var_os("PATH").unwrap_or_default();
    let joined = env::join_paths(once(dir.to_path_buf()).chain(env::split_paths(&current)))
        .map_err(|why| Error::Path(why.to_string()))?;
    cmd.env("PATH", joined);
    Ok(())
}

/// `NO_GH_DIR` with its one program, `gh`, a link to `false`: created once,
/// under target/, so proob's `gh auth token` fallback fails there.
fn no_gh_dir(root: &Path) -> Result<PathBuf, Error> {
    let dir = root.join(NO_GH_DIR);
    fs::create_dir_all(&dir).map_err(|source| Error::Io { path: dir.clone(), source })?;
    let stub = dir.join("gh");
    match fs::symlink_metadata(&stub).is_ok() {
        true => Ok(dir),
        false => link_to_false(&stub).map(|()| dir),
    }
}

#[cfg(unix)]
fn link_to_false(stub: &Path) -> Result<(), Error> {
    let target = find_on_path("false", &env::var_os("PATH").unwrap_or_default())
        .unwrap_or_else(|| PathBuf::from("/usr/bin/false"));
    std::os::unix::fs::symlink(target, stub).map_err(|source| Error::Io { path: stub.to_path_buf(), source })
}

#[cfg(not(unix))]
fn link_to_false(stub: &Path) -> Result<(), Error> {
    Err(Error::Io {
        path: stub.to_path_buf(),
        source: std::io::Error::other("the no-token declaration check needs a unix host"),
    })
}

fn spawn_error(exec: &Exec, source: std::io::Error) -> Error {
    Error::Spawn { program: exec.program.clone(), source }
}

/// The line printed before a step runs: the exact argv, shell-quoted for reading only.
pub fn describe(step: &Step) -> String {
    match step {
        Step::Exec(exec) => format!("$ {}", command_line(exec)),
        Step::Capture { exec, expect } => {
            let wanted: Vec<String> = expect.iter().map(ToString::to_string).collect();
            format!("$ {}  # expect {}", command_line(exec), wanted.join(", "))
        }
        Step::EnvUnset(names) => format!("check: {} unset", names.join(", ")),
        Step::LineCount(dir) => format!("count: lines of {dir}/*.rs"),
        Step::CommitLint(range) => format!("lint: commit subjects of {range} (CONTRIBUTING.md, Commits)"),
        Step::TitleLint(title) => format!("lint: title {}", shell_word(title)),
    }
}

pub fn command_line(exec: &Exec) -> String {
    let words: Vec<String> = exec
        .env
        .iter()
        .map(env_word)
        .chain(once(shell_word(&exec.program)))
        .chain(exec.args.iter().map(|arg| shell_word(arg)))
        .collect();
    words.join(" ")
}

fn env_word(change: &EnvChange) -> String {
    match change {
        EnvChange::PrependPath(dir) => format!("PATH={}:$PATH", shell_word(&dir.to_string_lossy())),
        EnvChange::NoGitHubToken => format!("PATH={NO_GH_DIR}:$PATH"),
    }
}

/// Single-quote a word unless it is plainly safe to read unquoted.
pub fn shell_word(word: &str) -> String {
    let plain = !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=+,@%".contains(c));
    match plain {
        true => word.to_owned(),
        false => format!("'{}'", word.replace('\'', "'\\''")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(program: &str, args: &[&str], env: Vec<EnvChange>) -> Exec {
        Exec { program: program.to_owned(), args: args.iter().map(|a| (*a).to_owned()).collect(), env }
    }

    #[test]
    fn plain_words_stay_bare_and_others_are_quoted() {
        assert_eq!(shell_word("--locked"), "--locked");
        assert_eq!(shell_word("./target/debug/proob"), "./target/debug/proob");
        assert_eq!(shell_word("a b"), "'a b'");
        assert_eq!(shell_word(""), "''");
        assert_eq!(shell_word("it's"), "'it'\\''s'");
        assert_eq!(shell_word("$HOME"), "'$HOME'");
    }

    #[test]
    fn the_printed_line_is_the_argv_with_env_first() {
        let plain = exec("cargo", &["build", "--locked"], vec![]);
        assert_eq!(command_line(&plain), "cargo build --locked");
        let with_path = exec("cargo", &["verus"], vec![EnvChange::PrependPath(PathBuf::from("/v/bin"))]);
        assert_eq!(command_line(&with_path), "PATH=/v/bin:$PATH cargo verus");
        let no_token = exec("./target/debug/proob", &["status"], vec![EnvChange::NoGitHubToken]);
        assert_eq!(command_line(&no_token), "PATH=target/xtask/no-gh:$PATH ./target/debug/proob status");
    }

    #[test]
    fn the_gh_stub_is_a_gh_that_fails() {
        let root = std::env::temp_dir().join(format!("xtask-no-gh-{}", std::process::id()));
        let dir = no_gh_dir(&root).expect("creates the stub directory");
        assert_eq!(dir, root.join("target/xtask/no-gh"));
        let status = Command::new(dir.join("gh")).args(["auth", "token"]).status().expect("runs");
        assert!(!status.success(), "the stub gh must fail");
        assert_eq!(no_gh_dir(&root).expect("idempotent"), dir);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn every_step_kind_describes_itself() {
        assert_eq!(describe(&Step::Exec(exec("x", &["y"], vec![]))), "$ x y");
        let capture = Step::Capture {
            exec: exec("x", &["--json"], vec![]),
            expect: vec![Expect::Field { key: "state", value: "unverified" }],
        };
        assert_eq!(describe(&capture), "$ x --json  # expect \"state\": \"unverified\" present");
        assert_eq!(describe(&Step::EnvUnset(&["A", "B"])), "check: A, B unset");
        assert_eq!(describe(&Step::LineCount("crates/x/src")), "count: lines of crates/x/src/*.rs");
        assert_eq!(
            describe(&Step::CommitLint("a..b".into())),
            "lint: commit subjects of a..b (CONTRIBUTING.md, Commits)"
        );
        assert_eq!(describe(&Step::TitleLint("feat: x".into())), "lint: title 'feat: x'");
    }

    #[test]
    fn programs_with_a_slash_are_workspace_relative() {
        let root = Path::new("/ws");
        assert_eq!(resolve(root, "cargo"), PathBuf::from("cargo"));
        assert_eq!(resolve(root, "tools/x.sh"), PathBuf::from("/ws/tools/x.sh"));
        assert_eq!(resolve(root, "./target/debug/proob"), PathBuf::from("/ws/./target/debug/proob"));
        assert_eq!(resolve(root, "/abs/claude"), PathBuf::from("/abs/claude"));
    }

    #[test]
    fn rust_files_lists_only_rs_sorted() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root");
        let files = rust_files(&root.join("crates/proob-trusted/src")).expect("reads");
        assert!(!files.is_empty());
        assert!(files.iter().all(|f| f.extension().is_some_and(|e| e == "rs")));
        assert!(files.windows(2).all(|w| w[0] < w[1]));
    }
}
