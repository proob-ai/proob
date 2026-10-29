//! What each task runs, as data. `plan` is a pure function of the task and
//! the tool locations, so every argv below is checked by the tests without
//! spawning anything; `run` is the only module that executes a `Step`.

use std::fmt;
use std::path::PathBuf;

use crate::cli::{Lint, Task};
use crate::tools::{Tools, Verus};

/// A program with its arguments. A `program` containing `/` is a path, relative
/// to the workspace root or absolute; a bare name is looked up on PATH. Every
/// step runs with the workspace root as its working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<EnvChange>,
}

/// An addition to the child's environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvChange {
    /// Put this directory first on PATH.
    PrependPath(PathBuf),
    /// No GitHub token is reachable: `GITHUB_TOKEN`/`GH_TOKEN` are already
    /// required unset, and `NO_GH_DIR`, whose only program is a `gh` that
    /// always fails, goes first on PATH, so proob's `gh auth token` fallback
    /// finds no login. The declaration job has no token in CI; this makes the
    /// same path the one exercised on a developer's machine signed in to gh.
    NoGitHubToken,
}

/// A property of captured stdout: the JSON field `"key": "value"` (any number
/// of spaces after the colon, as `grep '"key": *"value"'` matches it) is
/// present, or absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    Field { key: &'static str, value: &'static str },
    NoField { key: &'static str, value: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Run to completion; a non-zero exit fails the task.
    Exec(Exec),
    /// Run, capture stdout, and require every expectation of it.
    Capture { exec: Exec, expect: Vec<Expect> },
    /// Fail when any of these variables is set to a non-empty value.
    EnvUnset(&'static [&'static str]),
    /// Count and print the lines of the `.rs` files in this directory.
    LineCount(&'static str),
    /// Every commit subject of this revision range follows Conventional Commits.
    CommitLint(String),
    /// This one subject line (a pull request title) does.
    TitleLint(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    VerusMissing { version: String },
}

/// The tasks `ci` runs, in the order ci.yml's jobs run them:
/// lint (fmt, clippy, commit-lint), test (build, test, sim, bench soundness), verify (verify, trusted base, verify-negative),
/// crate-graph, shim (shim, hooks), readme-sync, declaration, bench (replay), plugin-validate.
/// ci.yml's test job runs `sim lifecycle honesty requirement shadow soundness`, then scores
/// them (`bench soundness`); `ci` runs every scenario
/// (under a minute without Docker; `workflows` skips itself when Docker is down).
pub const CI_ORDER: [Task; 17] = [
    Task::Fmt,
    Task::Clippy,
    Task::CommitLint(Lint::Default),
    Task::Build,
    Task::Test(Vec::new()),
    Task::Sim(Vec::new()),
    Task::BenchSoundness,
    Task::Verify,
    Task::TrustedBase,
    Task::VerifyNegative,
    Task::CrateGraph,
    Task::ShimTests,
    Task::HookTests,
    Task::ReadmeSync,
    Task::Declaration,
    Task::BenchReplay(Vec::new()),
    Task::PluginValidate,
];

/// What `commit-lint` with no range checks: the commits not yet on the default
/// branch. In CI the lint job passes the pull request's own `base..head`.
pub const DEFAULT_RANGE: &str = "origin/master..HEAD";

/// The variables that must not reach the declaration check.
pub const TOKENS: &[&str] = &["GITHUB_TOKEN", "GH_TOKEN"];

const PROOB: &str = "./target/debug/proob";
const SIM: &str = "./target/debug/sim";

/// Where `run` puts the failing `gh` stub (workspace-relative, under target/).
pub const NO_GH_DIR: &str = "target/xtask/no-gh";

const VERIFY: &[&str] = &[
    "verus",
    "verify",
    "--locked",
    "-p",
    "proob-core",
    "--fwd-verus-args-to",
    "roots",
    "--",
    "--no-cheating",
    "--rlimit",
    "10",
];

const CLIPPY: &[&str] = &["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"];

pub fn plan(task: &Task, tools: &Tools) -> Result<Vec<Step>, PlanError> {
    match task {
        Task::Build => Ok(vec![exec("cargo", &["build", "--locked", "--workspace"])]),
        Task::Test(extra) => Ok(vec![test(extra)]),
        Task::Verify => verus_env(tools).map(|env| vec![Step::Exec(command("cargo", VERIFY, env))]),
        Task::VerifyNegative => {
            verus_env(tools).map(|env| vec![Step::Exec(command("tools/ci/verify-negative.sh", &[], env))])
        }
        Task::CrateGraph => {
            Ok(vec![exec("tools/crate-graph-check.sh", &[]), exec("tools/ci/crate-graph-negative.sh", &[])])
        }
        Task::ShimTests => Ok(vec![exec("tests/shim/test_shim.sh", &[])]),
        Task::ReadmeSync => Ok(vec![exec("tools/readme-sync.sh", &[])]),
        Task::HookTests => Ok(vec![exec("tests/hooks/test_hooks.sh", &[])]),
        Task::TrustedBase => Ok(vec![Step::LineCount("crates/proob-trusted/src")]),
        Task::Declaration => Ok(declaration()),
        Task::PluginValidate => Ok(vec![exec(&tools.claude, &["plugin", "validate", "--strict", "."])]),
        Task::Sim(args) => Ok(sim(args)),
        Task::BenchReplay(args) => Ok(bench_replay(args)),
        Task::BenchSoundness => Ok(bench_soundness()),
        Task::Fmt => Ok(vec![exec("cargo", &["fmt", "--check"])]),
        Task::Clippy => Ok(vec![exec("cargo", CLIPPY)]),
        Task::CommitLint(Lint::Default) => Ok(vec![Step::CommitLint(DEFAULT_RANGE.to_owned())]),
        Task::CommitLint(Lint::Range(range)) => Ok(vec![Step::CommitLint(range.clone())]),
        Task::CommitLint(Lint::Title(title)) => Ok(vec![Step::TitleLint(title.clone())]),
    }
}

/// Score the verdicts the last `sim` run wrote (bench/README.md, "Measure 3");
/// the scorer is the sim binary's, which has the JSON parser xtask does without.
fn bench_soundness() -> Vec<Step> {
    vec![
        exec("cargo", &["build", "--locked", "-p", "sim"]),
        Step::Exec(Exec { program: SIM.to_owned(), args: vec!["--score".to_owned()], env: Vec::new() }),
    ]
}

/// Build the binary under test and the simulation once, then run the scenarios
/// (sim/src/main.rs), which drive `./target/debug/proob` as users run it.
fn sim(args: &[String]) -> Vec<Step> {
    vec![
        exec("cargo", &["build", "--locked", "-p", "proob", "-p", "sim"]),
        Step::Exec(Exec { program: SIM.to_owned(), args: args.to_vec(), env: Vec::new() }),
    ]
}

/// Build the binary the corpus replays against, then replay (bench/README.md,
/// "Regression corpus"), with `python3 -I` as every bench harness runs.
fn bench_replay(args: &[String]) -> Vec<Step> {
    let fixed = ["-I", "bench/harness/replay.py", "run"];
    let args = fixed.iter().map(|a| (*a).to_owned()).chain(args.iter().cloned()).collect();
    vec![
        exec("cargo", &["build", "--locked", "-p", "proob"]),
        Step::Exec(Exec { program: "python3".to_owned(), args, env: Vec::new() }),
    ]
}

fn command(program: &str, args: &[&str], env: Vec<EnvChange>) -> Exec {
    Exec { program: program.to_owned(), args: args.iter().map(|a| (*a).to_owned()).collect(), env }
}

fn exec(program: &str, args: &[&str]) -> Step {
    Step::Exec(command(program, args, Vec::new()))
}

fn test(extra: &[String]) -> Step {
    let fixed = ["nextest", "run", "--locked", "--workspace", "--no-fail-fast"];
    let args = fixed.iter().map(|a| (*a).to_owned()).chain(extra.iter().cloned()).collect();
    Step::Exec(Exec { program: "cargo".to_owned(), args, env: Vec::new() })
}

/// Verus is found on PATH, or prepended from where tools/get-verus.sh put it;
/// xtask never downloads it.
fn verus_env(tools: &Tools) -> Result<Vec<EnvChange>, PlanError> {
    match &tools.verus {
        Verus::OnPath => Ok(Vec::new()),
        Verus::Cached(dir) => Ok(vec![EnvChange::PrependPath(dir.clone())]),
        Verus::Missing => Err(PlanError::VerusMissing { version: tools.verus_version.clone() }),
    }
}

/// The `declaration` job of ci.yml: the declaration parses, and with no token
/// `status` renders Unverified and never Holds.
fn declaration() -> Vec<Step> {
    vec![
        exec("cargo", &["build", "--locked", "-p", "proob"]),
        exec(PROOB, &["validate"]),
        Step::EnvUnset(TOKENS),
        Step::Exec(command(PROOB, &["status"], vec![EnvChange::NoGitHubToken])),
        Step::Capture {
            exec: command(PROOB, &["status", "--json"], vec![EnvChange::NoGitHubToken]),
            expect: vec![
                Expect::Field { key: "state", value: "unverified" },
                Expect::NoField { key: "word", value: "holds" },
            ],
        },
    ]
}

impl Expect {
    pub fn holds(&self, text: &str) -> bool {
        match self {
            Expect::Field { key, value } => has_field(text, key, value),
            Expect::NoField { key, value } => !has_field(text, key, value),
        }
    }
}

/// True when `"key":`, optional spaces, `"value"` occurs in `text`.
fn has_field(text: &str, key: &str, value: &str) -> bool {
    let needle = format!("\"{key}\":");
    let wanted = format!("\"{value}\"");
    text.match_indices(&needle).any(|(at, _)| text[at + needle.len()..].trim_start_matches(' ').starts_with(&wanted))
}

impl fmt::Display for Expect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expect::Field { key, value } => write!(f, "\"{key}\": \"{value}\" present"),
            Expect::NoField { key, value } => write!(f, "\"{key}\": \"{value}\" absent"),
        }
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::VerusMissing { version } => write!(
                f,
                "verify needs the pinned Verus release {version}: run tools/get-verus.sh, or put cargo-verus on PATH"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools(verus: Verus) -> Tools {
        Tools { verus, verus_version: "0.2026.10.26.d17fbd0".to_owned(), claude: "claude".to_owned() }
    }

    fn words(step: &Step) -> Vec<String> {
        match step {
            Step::Exec(exec) | Step::Capture { exec, .. } => {
                std::iter::once(exec.program.clone()).chain(exec.args.iter().cloned()).collect()
            }
            other => panic!("not a command: {other:?}"),
        }
    }

    #[test]
    fn build_and_validate_run_in_the_plain_environment() {
        let steps = plan(&Task::Declaration, &tools(Verus::OnPath)).expect("plans");
        for step in &steps[..2] {
            let Step::Exec(exec) = step else { panic!("exec") };
            assert!(exec.env.is_empty(), "{}", argv(step));
        }
    }

    fn argv(step: &Step) -> String {
        words(step).join(" ")
    }

    fn only(task: &Task, verus: Verus) -> Step {
        let steps = plan(task, &tools(verus)).expect("plans");
        assert_eq!(steps.len(), 1, "{task:?} is one step");
        steps.into_iter().next().expect("one step")
    }

    #[test]
    fn build_is_the_locked_workspace_build() {
        assert_eq!(argv(&only(&Task::Build, Verus::OnPath)), "cargo build --locked --workspace");
    }

    #[test]
    fn test_is_nextest_with_pass_through() {
        assert_eq!(
            argv(&only(&Task::Test(vec![]), Verus::OnPath)),
            "cargo nextest run --locked --workspace --no-fail-fast"
        );
        let extra = vec!["-p".to_owned(), "xtask".to_owned()];
        assert_eq!(
            argv(&only(&Task::Test(extra), Verus::OnPath)),
            "cargo nextest run --locked --workspace --no-fail-fast -p xtask"
        );
    }

    #[test]
    fn verify_is_the_exact_ci_command() {
        let step = only(&Task::Verify, Verus::OnPath);
        assert_eq!(
            argv(&step),
            "cargo verus verify --locked -p proob-core --fwd-verus-args-to roots -- --no-cheating --rlimit 10"
        );
        let Step::Exec(exec) = step else { panic!("exec") };
        assert!(exec.env.is_empty(), "nothing to prepend when cargo-verus is on PATH");
    }

    #[test]
    fn verify_prepends_the_cached_release_to_path() {
        let dir = PathBuf::from("/home/me/.proob-dev/verus/0.2026.10.26.d17fbd0/verus-x86-linux");
        let Step::Exec(exec) = only(&Task::Verify, Verus::Cached(dir.clone())) else { panic!("exec") };
        assert_eq!(exec.env, vec![EnvChange::PrependPath(dir)]);
    }

    #[test]
    fn verify_without_verus_is_a_plan_error_with_the_pinned_version() {
        let error = plan(&Task::Verify, &tools(Verus::Missing)).expect_err("no verus");
        assert_eq!(error, PlanError::VerusMissing { version: "0.2026.10.26.d17fbd0".to_owned() });
        assert_eq!(
            error.to_string(),
            "verify needs the pinned Verus release 0.2026.10.26.d17fbd0: run tools/get-verus.sh, or put cargo-verus on PATH"
        );
        assert!(!error.to_string().contains('\n'), "one line");
    }

    #[test]
    fn verify_negative_runs_the_script_with_the_same_verus_env() {
        let dir = PathBuf::from("/v");
        let Step::Exec(exec) = only(&Task::VerifyNegative, Verus::Cached(dir.clone())) else { panic!("exec") };
        assert_eq!(exec.program, "tools/ci/verify-negative.sh");
        assert!(exec.args.is_empty());
        assert_eq!(exec.env, vec![EnvChange::PrependPath(dir)]);
        assert!(plan(&Task::VerifyNegative, &tools(Verus::Missing)).is_err());
    }

    #[test]
    fn crate_graph_is_the_check_then_the_negative_test() {
        let steps = plan(&Task::CrateGraph, &tools(Verus::OnPath)).expect("plans");
        let argvs: Vec<String> = steps.iter().map(argv).collect();
        assert_eq!(argvs, ["tools/crate-graph-check.sh", "tools/ci/crate-graph-negative.sh"]);
    }

    #[test]
    fn shell_suites_run_their_scripts() {
        assert_eq!(argv(&only(&Task::ShimTests, Verus::OnPath)), "tests/shim/test_shim.sh");
        assert_eq!(argv(&only(&Task::HookTests, Verus::OnPath)), "tests/hooks/test_hooks.sh");
    }

    #[test]
    fn trusted_base_counts_the_trusted_sources() {
        assert_eq!(only(&Task::TrustedBase, Verus::OnPath), Step::LineCount("crates/proob-trusted/src"));
    }

    #[test]
    fn declaration_mirrors_the_ci_job_step_by_step() {
        let steps = plan(&Task::Declaration, &tools(Verus::OnPath)).expect("plans");
        assert_eq!(steps.len(), 5);
        assert_eq!(argv(&steps[0]), "cargo build --locked -p proob");
        assert_eq!(argv(&steps[1]), "./target/debug/proob validate");
        assert_eq!(steps[2], Step::EnvUnset(&["GITHUB_TOKEN", "GH_TOKEN"]));
        assert_eq!(argv(&steps[3]), "./target/debug/proob status");
        assert_eq!(argv(&steps[4]), "./target/debug/proob status --json");
        let Step::Exec(status) = &steps[3] else { panic!("exec") };
        assert_eq!(status.env, vec![EnvChange::NoGitHubToken], "status runs with no token reachable");
        let Step::Capture { exec, expect } = &steps[4] else { panic!("capture") };
        assert_eq!(exec.env, vec![EnvChange::NoGitHubToken]);
        assert_eq!(
            *expect,
            vec![Expect::Field { key: "state", value: "unverified" }, Expect::NoField { key: "word", value: "holds" },]
        );
    }

    #[test]
    fn sim_builds_proob_and_itself_then_runs_the_named_scenarios() {
        let named = vec!["lifecycle".to_owned(), "honesty".to_owned()];
        let steps = plan(&Task::Sim(named), &tools(Verus::OnPath)).expect("plans");
        let argvs: Vec<String> = steps.iter().map(argv).collect();
        assert_eq!(argvs, ["cargo build --locked -p proob -p sim", "./target/debug/sim lifecycle honesty"]);
        let steps = plan(&Task::Sim(vec![]), &tools(Verus::OnPath)).expect("plans");
        assert_eq!(argv(&steps[1]), "./target/debug/sim", "no names: every scenario");
    }

    #[test]
    fn bench_soundness_builds_sim_then_scores() {
        let steps = plan(&Task::BenchSoundness, &tools(Verus::OnPath)).expect("plans");
        let argvs: Vec<String> = steps.iter().map(argv).collect();
        assert_eq!(argvs, ["cargo build --locked -p sim", "./target/debug/sim --score"]);
    }

    #[test]
    fn bench_replay_builds_proob_then_runs_the_replayer() {
        let steps = plan(&Task::BenchReplay(vec!["url-t1".to_owned()]), &tools(Verus::OnPath)).expect("plans");
        let argvs: Vec<String> = steps.iter().map(argv).collect();
        assert_eq!(argvs, ["cargo build --locked -p proob", "python3 -I bench/harness/replay.py run url-t1"]);
    }

    #[test]
    fn plugin_validate_uses_the_detected_claude() {
        let mut detected = tools(Verus::OnPath);
        detected.claude = "/home/me/.local/bin/claude".to_owned();
        let steps = plan(&Task::PluginValidate, &detected).expect("plans");
        assert_eq!(argv(&steps[0]), "/home/me/.local/bin/claude plugin validate --strict .");
    }

    #[test]
    fn ci_runs_the_jobs_of_ci_yml_in_order() {
        let names: Vec<&str> = CI_ORDER.iter().map(Task::name).collect();
        assert_eq!(
            names,
            [
                "fmt",
                "clippy",
                "commit-lint",
                "build",
                "test",
                "sim",
                "bench soundness",
                "verify",
                "trusted-base",
                "verify-negative",
                "crate-graph",
                "shim-tests",
                "hook-tests",
                "readme-sync",
                "declaration",
                "bench replay",
                "plugin-validate",
            ]
        );
    }

    #[test]
    fn fmt_and_clippy_are_the_exact_ci_commands() {
        assert_eq!(argv(&only(&Task::Fmt, Verus::OnPath)), "cargo fmt --check");
        assert_eq!(
            argv(&only(&Task::Clippy, Verus::OnPath)),
            "cargo clippy --workspace --all-targets --locked -- -D warnings"
        );
    }

    #[test]
    fn commit_lint_checks_the_range_or_the_title() {
        let step = |lint| only(&Task::CommitLint(lint), Verus::OnPath);
        assert_eq!(step(Lint::Default), Step::CommitLint("origin/master..HEAD".to_owned()));
        assert_eq!(step(Lint::Range("a..b".to_owned())), Step::CommitLint("a..b".to_owned()));
        assert_eq!(step(Lint::Title("feat: x".to_owned())), Step::TitleLint("feat: x".to_owned()));
    }

    #[test]
    fn every_ci_task_plans_when_verus_is_on_path() {
        for task in CI_ORDER.iter() {
            assert!(plan(task, &tools(Verus::OnPath)).is_ok(), "{} plans", task.name());
        }
    }

    #[test]
    fn field_matching_follows_the_grep_pattern() {
        let text = "{\n  \"state\": \"unverified\",\n  \"word\":\"unverified\"\n}\n";
        assert!(Expect::Field { key: "state", value: "unverified" }.holds(text));
        assert!(Expect::Field { key: "word", value: "unverified" }.holds(text), "no space after the colon");
        assert!(Expect::NoField { key: "word", value: "holds" }.holds(text));
        assert!(!Expect::Field { key: "state", value: "holds" }.holds(text));
        assert!(!Expect::Field { key: "state", value: "unverified" }.holds("\"state\":   \"verified\""));
        assert!(Expect::Field { key: "state", value: "unverified" }.holds("\"state\":   \"unverified\""));
    }
}
