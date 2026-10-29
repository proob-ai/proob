//! `score` for a Python binding: `python3 -m mutmut run` in a temporary copy
//! of the adapter root, configured by a `[tool.mutmut]` table proob writes
//! into the copy's `pyproject.toml` (replacing a table of that name, keeping
//! everything else, so the repository's pytest configuration still applies;
//! the repository itself is never written to). The counts come from the
//! `.meta` file mutmut writes for each scope file under `mutants/`.
//!
//! # What mutmut 3.8.0 lets proob restrict, exactly
//!
//! Mutation: `only_mutate` is a list of path patterns a source file must
//! match to be mutated; proob lists the scope's `.py` files literally (the
//! path grammar admits no glob character), so exactly those files are
//! mutated. `source_paths` names every top-level directory under the root
//! that holds Python, and every root-level module, so the copy mutmut
//! imports from (`mutants/`) is the whole project with only the scope files
//! changed. Test selection: `pytest_add_cli_args_test_selection` is appended
//! to every pytest run mutmut makes without an explicit test list, which are
//! the stats run that maps tests to the functions they execute, the clean
//! run, and the forced-fail run, and each mutant is then run only against the
//! tests the stats run recorded for its function; with the one node id there,
//! no other test can kill a mutant. mutmut runs pytest in process from
//! `mutants/`, where the node id resolves to the copied test file.
//!
//! What it does not let proob do: run a mutant no selected test reaches.
//! Such a mutant is recorded `no tests` (exit code 33) without a run, and
//! when the bound test reaches no mutated function at all mutmut stops before
//! any mutant, exit 1, saying it `could not find any test case for any
//! mutant`. Both count as survivors here, since the bound check cannot tell
//! them from the original, which is how a vacuous test scores
//! `Scored { killed: 0, survived: n, .. }`. The mapping onto the score
//! shape: killed → `killed`; survived and no tests → `survived`; timeout →
//! `timeout`; skipped, suspicious, segfault, caught by type check (no type
//! checker is configured), interrupted, and not checked → `unviable`,
//! excluded from the denominator as a mutant cargo-mutants cannot build is.
//! The baseline is mutmut's own stats and clean
//! runs: a bound test that fails before any mutation stops the run (`failed
//! to collect stats`, `Failed to run clean test`) and nothing is scored; a
//! test mutmut's forced-fail run cannot make fail is an error too. Those
//! messages are mutmut 3.8.0's, which is why the version is pinned.
//!
//! # Sample and jobs
//!
//! mutmut 3.8.0 has no listing command, so a score is two `mutmut run`s in
//! the same copy. The first names one mutant that cannot exist
//! (`LIST_ONLY`): mutmut generates every mutant, runs the stats run (so a
//! baseline failure or an untraced test ends the score here, as above), then
//! stops on its own assertion that nothing matches the name
//! (`NOTHING_MATCHES`), having run no mutant. The `.meta` files then hold
//! every mutant in scope, unrun. When more than `max_mutants` are listed,
//! `mutation::sample` chooses that many by name and the second run names
//! exactly those (`mutmut run <name>...`; a name with no glob character is
//! matched exactly), reusing the stats; otherwise the second run names
//! none and runs them all. Only the chosen names' results are counted, so a
//! mutant left unrun by the sample is `unsampled`, never `not checked`.
//! Every count, and so the kill floor, is the sample's. Both runs take
//! `--max-children` (`mutation::Limits::jobs_for` on the temporary
//! directory).
//!
//! mutmut forks a worker per mutant (`os.fork`), so this runs on Linux and
//! macOS only; on Windows the outcome is a runner error saying so. The copy
//! leaves out hidden directories, caches, build output, and virtual
//! environments (`discover::skip_dir`); a test that reads files outside the
//! copied tree fails in it.
use crate::argv::{Checked, Command};
use crate::mutation::sample;
use crate::python::check::{self, write_plugin};
use crate::python::discover::{entries, skip_dir};
use crate::python::hypothesis::{PLUGIN_MODULE, PROFILE, PROFILE_OPTION, SEED_OPTION};
use crate::python::tools::{Need, Resolution, resolve};
use crate::python::{CheckRef, PythonAdapter, last, parse_check_ref};
use crate::rust::score::Outside;
use crate::{AdapterError, Binding, Budget, Score, ScoreOutcome, Seed, ToolVersions, exit_text};
use proob_facts::error::ParseError;
use proob_facts::parsers::mutmut::{MetaFile, MutmutCounts, counts, parse_meta};
use proob_spawn::Output;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A mutation run tests once per mutant; hours, not minutes.
pub const TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);
pub const COPY_DIR: &str = "copy";
pub const MUTANTS_DIR: &str = "mutants";
pub const PYPROJECT: &str = "pyproject.toml";
pub const META_SUFFIX: &str = ".meta";
/// Root-level files pytest may read that mutmut does not copy by itself.
pub const ROOT_ALSO_COPY: [&str; 3] = ["pytest.ini", ".pytest.ini", "tox.ini"];
/// mutmut 3.8.0's messages, on stdout, for the runs it stops with exit 1.
pub const STOP_NO_COVERAGE: &str = "could not find any test case for any mutant";
pub const STATS_FAILED: &str = "failed to collect stats";
pub const CLEAN_FAILED: &str = "Failed to run clean test";
pub const FORCED_FAIL_FAILED: &str = "Unable to force test failures";
pub const FORK_HINT: &str = "mutmut needs os.fork, which Windows lacks: score Python properties on Linux or macOS (the evidence workflow runs on ubuntu)";
/// The mutant name the listing run asks for: no mutant is named with `-`.
pub const LIST_ONLY: &str = "proob-list-only";
/// mutmut 3.8.0's assertion when no mutant matches the names asked for.
pub const NOTHING_MATCHES: &str = "Filtered for specific mutants, but nothing matches";
pub const BASELINE_FAILED: &str =
    "baseline failed: the bound test does not pass before any mutation, so nothing can be scored";

/// Scope files as root-relative checked paths; only Python sources can be
/// mutated, and a file outside the adapter root cannot be reached.
pub fn mutable_files(adapter: &PythonAdapter, scope_files: &[PathBuf]) -> Result<Vec<Checked>, Outside> {
    scope_files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "py"))
        .map(|f| {
            let under_root = f.strip_prefix(adapter.root()).map_err(|_| Outside::Root(f.clone()))?;
            Checked::path(under_root).map_err(|rule| Outside::Grammar(f.clone(), rule))
        })
        .collect()
}

/// The top-level entries of `root` that hold Python: directories with a
/// `.py` file anywhere below them (junk skipped) and root-level modules.
pub fn source_paths(root: &Path) -> Result<Vec<String>, AdapterError> {
    let mut found = Vec::new();
    for path in entries(root)? {
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
        let holds = if path.is_dir() { !skip_dir(&path) && holds_python(&path)? } else { name.ends_with(".py") };
        if holds {
            found.push(name);
        }
    }
    Ok(found)
}

fn holds_python(dir: &Path) -> Result<bool, AdapterError> {
    for path in entries(dir)? {
        let hit = if path.is_dir() {
            !skip_dir(&path) && holds_python(&path)?
        } else {
            path.extension().is_some_and(|e| e == "py")
        };
        if hit {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The root-level files to copy beside mutmut's own defaults, when present.
pub fn also_copy(root: &Path) -> Vec<String> {
    ROOT_ALSO_COPY.into_iter().filter(|f| root.join(f).is_file()).map(str::to_string).collect()
}

fn strings<I: IntoIterator<Item = String>>(items: I) -> toml::Value {
    toml::Value::Array(items.into_iter().map(toml::Value::String).collect())
}

/// The `[tool.mutmut]` table for one score. Pure.
pub fn mutmut_table(r: &CheckRef, files: &[Checked], sources: &[String], also: &[String], seed: Seed) -> toml::Table {
    let pytest_args = ["-p", "no:cacheprovider", "-p", PLUGIN_MODULE, PROFILE_OPTION, PROFILE, SEED_OPTION]
        .map(str::to_string)
        .into_iter()
        .chain([seed.0.to_string()]);
    let mut t = toml::Table::new();
    t.insert("source_paths".into(), strings(sources.iter().cloned()));
    t.insert("only_mutate".into(), strings(files.iter().map(|f| f.as_str().to_string())));
    t.insert("pytest_add_cli_args_test_selection".into(), strings([r.node_id().as_str().to_string()]));
    t.insert("pytest_add_cli_args".into(), strings(pytest_args));
    t.insert("also_copy".into(), strings(also.iter().cloned()));
    t.insert("use_git_change_detection".into(), toml::Value::Boolean(false));
    t.insert("debug".into(), toml::Value::Boolean(false));
    t
}

/// The copy's `pyproject.toml`: the repository's, if any, with `tool.mutmut`
/// replaced by proob's table. Pure.
pub fn merged_pyproject(existing: Option<&str>, mutmut: toml::Table) -> Result<String, String> {
    let mut root = match existing {
        Some(text) => text.parse::<toml::Table>().map_err(|e| e.to_string())?,
        None => toml::Table::new(),
    };
    match root.entry("tool").or_insert_with(|| toml::Value::Table(toml::Table::new())) {
        toml::Value::Table(tool) => {
            tool.insert("mutmut".into(), toml::Value::Table(mutmut));
        }
        _ => return Err("`tool` is not a table".to_string()),
    }
    Ok(format!(
        "# [tool.mutmut] written by proob for one `run score`, in a temporary copy; never read from the repository.\n{root}"
    ))
}

/// The argv for one `mutmut run` in the copy, naming `names` (none: every
/// mutant), with the same environment a check gets. Pure.
pub fn plan(
    python: &Path,
    copy: &Path,
    tmp: &Path,
    tool_env: &[(OsString, OsString)],
    names: &[Checked],
    jobs: u32,
) -> Command {
    let command = Command::new(python, TIMEOUT)
        .arg("-m")
        .arg("mutmut")
        .arg("run")
        .opt("--max-children", &Checked::number(u64::from(jobs)));
    names.iter().fold(command, |c, n| c.tail(n)).cwd(copy).envs(tool_env).envs(&check::environment(tmp, None, tool_env)) /* mutmut puts its mutated tree first itself; the unmutated src here would hide every mutant */
}

/// The listing run: generation and stats, then mutmut's own stop on `LIST_ONLY`. Pure.
pub fn list_plan(python: &Path, copy: &Path, tmp: &Path, tool_env: &[(OsString, OsString)], jobs: u32) -> Command {
    Command::new(python, TIMEOUT)
        .arg("-m")
        .arg("mutmut")
        .arg("run")
        .opt("--max-children", &Checked::number(u64::from(jobs)))
        .arg(LIST_ONLY)
        .cwd(copy)
        .envs(tool_env)
        .envs(&check::environment(tmp, None, tool_env))
}

/// Whether the listing run stopped where it should: on the name, after the stats.
pub fn listed(out: &Output) -> bool {
    out.code == Some(1) && String::from_utf8_lossy(&out.stderr).contains(NOTHING_MATCHES)
}

/// The scope files' `.meta` results in the copy, one per file (absent: `None`).
fn metas(copy: &Path, files: &[Checked]) -> Vec<Option<Result<MetaFile, ParseError>>> {
    files
        .iter()
        .map(|f| {
            std::fs::read_to_string(copy.join(MUTANTS_DIR).join(format!("{}{META_SUFFIX}", f.as_str())))
                .ok()
                .map(|t| parse_meta(&t))
        })
        .collect()
}

/// Every mutant name in the `.meta` files, file by file, each file's sorted.
pub fn names(metas: &[Option<Result<MetaFile, ParseError>>]) -> Vec<String> {
    metas.iter().flatten().flatten().flat_map(|m| m.exit_code_by_key.keys().cloned()).collect()
}

/// The counts of the `chosen` names only (`None`: every name). Pure.
pub fn counted(
    metas: &[Option<Result<MetaFile, ParseError>>],
    chosen: Option<&BTreeSet<String>>,
) -> Vec<Option<Result<MutmutCounts, ParseError>>> {
    let keep = |m: &MetaFile| MetaFile {
        exit_code_by_key: m
            .exit_code_by_key
            .iter()
            .filter(|(k, _)| chosen.is_none_or(|c| c.contains(*k)))
            .map(|(k, v)| (k.clone(), *v))
            .collect(),
    };
    metas.iter().map(|m| m.as_ref().map(|r| r.as_ref().map(|m| counts(&keep(m))).map_err(Clone::clone))).collect()
}

/// Copies a tree, leaving out what `skip_dir` names and `.git`.
pub fn copy_tree(from: &Path, to: &Path) -> Result<(), AdapterError> {
    let io = |path: &Path, e: std::io::Error| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() };
    for src in entries(from)? {
        let Some(name) = src.file_name() else { continue };
        let dst = to.join(name);
        if src.is_dir() {
            if !skip_dir(&src) && name != ".git" {
                std::fs::create_dir_all(&dst).map_err(|e| io(&dst, e))?;
                copy_tree(&src, &dst)?;
            }
        } else {
            std::fs::copy(&src, &dst).map_err(|e| io(&src, e))?;
        }
    }
    Ok(())
}

pub fn run(
    adapter: &PythonAdapter,
    repo: &Path,
    binding: &Binding,
    scope_files: &[PathBuf],
) -> Result<Score, AdapterError> {
    let r = parse_check_ref(&binding.check_ref)?;
    let root = adapter.root_in(repo);
    if cfg!(windows) {
        return Ok(errored(FORK_HINT.to_string(), Vec::new()));
    }
    let tools = match resolve(&root, &[Need::Pytest, Need::Mutmut], adapter.tool_env()) {
        Resolution::Ready(tools) => tools,
        Resolution::Missing { reason, versions } => return Ok(errored(reason, versions)),
    };
    let files = match mutable_files(adapter, scope_files) {
        Ok(files) => files,
        Err(Outside::Root(f)) => {
            let reason = format!(
                "scope file {} lies outside the adapter root `{}`; this adapter cannot mutate it",
                f.display(),
                adapter.root().display()
            );
            return Ok(errored(reason, tools.versions));
        }
        Err(Outside::Grammar(f, rule)) => return Err(AdapterError::Unsafe { value: f.display().to_string(), rule }),
    };
    if files.is_empty() {
        return Ok(Score { outcome: ScoreOutcome::NoMutants, tool_versions: tools.versions, jobs: None });
    }
    let tmp = tempfile::tempdir().map_err(|e| AdapterError::Temp { message: e.to_string() })?;
    let copy = tmp.path().join(COPY_DIR);
    std::fs::create_dir_all(&copy).map_err(|e| AdapterError::Io { path: copy.clone(), message: e.to_string() })?;
    copy_tree(&root, &copy)?;
    write_plugin(tmp.path(), Budget::DEFAULT)?;
    let table = mutmut_table(&r, &files, &source_paths(&root)?, &also_copy(&root), Seed::for_scoring(binding.id));
    let pyproject = copy.join(PYPROJECT);
    let text = match merged_pyproject(std::fs::read_to_string(&pyproject).ok().as_deref(), table) {
        Ok(text) => text,
        Err(e) => return Ok(errored(format!("{PYPROJECT} could not be read as TOML: {e}"), tools.versions)),
    };
    std::fs::write(&pyproject, text)
        .map_err(|e| AdapterError::Io { path: pyproject.clone(), message: e.to_string() })?;
    // shortcut: mutmut runs in this one copy and forks per job, so no copy size is measured and the 1.5 GB default bounds jobs; measure if that bound proves wrong.
    let sized = adapter.limits().jobs_for(&std::env::temp_dir(), None);
    let jobs = sized.count();
    let listing = match list_plan(&tools.python, &copy, tmp.path(), adapter.tool_env(), jobs).run() {
        Ok(out) => out,
        Err(e) => return Ok(errored(format!("mutmut did not run: {e}"), tools.versions)),
    };
    let generated = metas(&copy, &files);
    if !listed(&listing) {
        return Ok(Score {
            outcome: not_listed(outcome_of(&listing, &counted(&generated, None))),
            tool_versions: tools.versions,
            jobs: None,
        });
    }
    let all = names(&generated);
    if all.is_empty() {
        return Ok(Score { outcome: ScoreOutcome::NoMutants, tool_versions: tools.versions, jobs: None });
    }
    let chosen: Vec<String> = sample(&all, adapter.limits().max_mutants, Seed::for_scoring(binding.id))
        .into_iter()
        .map(|i| all[i].clone())
        .collect();
    let capped = chosen.len() < all.len();
    let args = if capped {
        chosen
            .iter()
            .map(|n| Checked::mutant_name(n).map_err(|rule| AdapterError::Unsafe { value: n.clone(), rule }))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    let out = match plan(&tools.python, &copy, tmp.path(), adapter.tool_env(), &args, jobs).run() {
        Ok(out) => out,
        Err(e) => return Ok(errored(format!("mutmut did not run: {e}"), tools.versions)),
    };
    let set: BTreeSet<String> = chosen.into_iter().collect();
    let unsampled = u32::try_from(all.len() - set.len()).unwrap_or(u32::MAX);
    let outcome = with_unsampled(outcome_of(&out, &counted(&metas(&copy, &files), Some(&set))), unsampled);
    Ok(Score { outcome, tool_versions: tools.versions, jobs: Some(sized) })
}

/// A listing run that did not stop on the name: its own outcome, unless it
/// would read as a score, which a run that ran no chosen mutant cannot be.
pub fn not_listed(o: ScoreOutcome) -> ScoreOutcome {
    match o {
        ScoreOutcome::Scored { .. } => ScoreOutcome::Errored {
            reason: format!(
                "mutmut did not stop after listing the mutants (expected `{NOTHING_MATCHES}`); nothing was scored"
            ),
        },
        other => other,
    }
}

/// A score's outcome with the mutants the sample left out.
pub fn with_unsampled(o: ScoreOutcome, unsampled: u32) -> ScoreOutcome {
    match o {
        ScoreOutcome::Scored { killed, survived, timeout, unviable, .. } => {
            ScoreOutcome::Scored { killed, survived, timeout, unviable, unsampled }
        }
        other => other,
    }
}

fn errored(reason: String, tool_versions: ToolVersions) -> Score {
    Score { outcome: ScoreOutcome::Errored { reason }, tool_versions, jobs: None }
}

/// mutmut's output without its spinner frames and carriage returns.
pub fn tail(out: &Output) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout).replace('\r', "\n");
    let lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with(|c: char| ('\u{2800}'..='\u{28FF}').contains(&c)))
        .collect();
    let stderr = String::from_utf8_lossy(&out.stderr);
    last(format!("{}\n{}", lines.join("\n"), stderr.trim()).trim(), check::TAIL)
}

/// Exit code, output, and the scope files' `.meta` results to an outcome;
/// anything incomplete is `Errored`.
pub fn outcome_of(out: &Output, metas: &[Option<Result<MutmutCounts, ParseError>>]) -> ScoreOutcome {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let total = metas.iter().flatten().try_fold(MutmutCounts::default(), |acc, m| m.as_ref().map(|c| acc.plus(*c)));
    match (out.code, total) {
        (Some(1), _) if stdout.contains(STATS_FAILED) || stdout.contains(CLEAN_FAILED) => ScoreOutcome::Errored { reason: BASELINE_FAILED.to_string() },
        (Some(1), _) if stdout.contains(FORCED_FAIL_FAILED) => ScoreOutcome::Errored {
            reason: "the bound test cannot be made to fail: mutmut's forced-fail run passed, so no mutant could be killed by it (does the test swallow exceptions from the code it exercises?)".to_string(),
        },
        (_, Err(e)) => ScoreOutcome::Errored { reason: format!("mutmut's .meta results could not be read: {e}") },
        (Some(0), Ok(c)) => scored(c),
        (Some(1), Ok(c)) if stdout.contains(STOP_NO_COVERAGE) => unreached(c),
        (code, _) => ScoreOutcome::Errored { reason: format!("mutmut failed (exit {}); last output: {}", exit_text(code), tail(out)) },
    }
}

/// Why a run where mutmut traced the bound test into no mutated function is
/// not a score: either the test never runs the code in scope, or mutmut
/// imported the package before mutating it (as when mutmut itself depends on
/// the package under test), and the two cannot be told apart from here.
pub const NOT_TRACED: &str = "not scored: mutmut traced the bound test into none of the code in scope (the test may not run this code, or mutmut loaded the package before mutating it, as it does for packages it depends on itself)";

/// A complete run's counts on the score shape (see the module doc). When no
/// mutant was traced to the bound test at all, there is no score.
pub fn scored(c: MutmutCounts) -> ScoreOutcome {
    let traced = c.killed + c.survived + c.timeout;
    match c.total() {
        0 => ScoreOutcome::NoMutants,
        _ if traced == 0 && c.no_tests > 0 => ScoreOutcome::Errored { reason: NOT_TRACED.to_string() },
        _ => ScoreOutcome::Scored {
            killed: c.killed,
            survived: c.survived + c.no_tests,
            timeout: c.timeout,
            unviable: c.skipped + c.suspicious + c.segfault + c.caught_by_type_check + c.interrupted + c.not_checked,
            unsampled: 0,
        },
    }
}

/// mutmut stopped because the bound test reached no mutated function.
pub fn unreached(c: MutmutCounts) -> ScoreOutcome {
    match c.total() {
        0 => ScoreOutcome::NoMutants,
        _ => ScoreOutcome::Errored { reason: NOT_TRACED.to_string() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.as_bytes().to_vec(), stderr: Vec::new() }
    }

    fn c(killed: u32, survived: u32, no_tests: u32, not_checked: u32) -> MutmutCounts {
        MutmutCounts { killed, survived, no_tests, not_checked, ..MutmutCounts::default() }
    }

    #[test]
    fn outcomes_from_exit_codes_and_messages() {
        let complete = [Some(Ok(c(3, 1, 2, 0))), None];
        assert_eq!(
            outcome_of(&out(0, ""), &complete),
            ScoreOutcome::Scored { killed: 3, survived: 3, timeout: 0, unviable: 0, unsampled: 0 }
        );
        let generated = [Some(Ok(c(0, 0, 0, 4)))];
        let stopped =
            out(1, &format!("⠴ Running stats\nStopping early, because we {STOP_NO_COVERAGE}. It seems that…\n"));
        assert_eq!(
            outcome_of(&stopped, &generated),
            ScoreOutcome::Errored { reason: NOT_TRACED.to_string() },
            "no mutant traced: not a score"
        );
        assert_eq!(outcome_of(&out(0, ""), &[None]), ScoreOutcome::NoMutants);
        assert!(
            matches!(outcome_of(&out(1, &format!("…\n{STATS_FAILED}. runner returned 1\n")), &generated), ScoreOutcome::Errored { reason } if reason == BASELINE_FAILED)
        );
        assert!(
            matches!(outcome_of(&out(1, &format!("FAILED: {FORCED_FAIL_FAILED}\n")), &generated), ScoreOutcome::Errored { reason } if reason.contains("forced-fail"))
        );
        assert!(
            matches!(outcome_of(&out(2, "Usage: mutmut run\n"), &generated), ScoreOutcome::Errored { reason } if reason.contains("exit code 2") && reason.contains("Usage"))
        );
        let bad = [Some(Err(ParseError::Schema("x".into())))];
        assert!(matches!(outcome_of(&out(0, ""), &bad), ScoreOutcome::Errored { reason } if reason.contains(".meta")));
    }

    #[test]
    fn unviable_statuses_leave_the_denominator() {
        let all = MutmutCounts {
            killed: 1,
            survived: 1,
            no_tests: 1,
            skipped: 1,
            suspicious: 1,
            timeout: 1,
            caught_by_type_check: 1,
            interrupted: 1,
            not_checked: 1,
            segfault: 1,
        };
        assert_eq!(scored(all), ScoreOutcome::Scored { killed: 1, survived: 2, timeout: 1, unviable: 6, unsampled: 0 });
    }

    #[test]
    fn the_table_restricts_mutation_and_selection() {
        let r = parse_check_ref("tests/test_props.py::test_merge").expect("check ref");
        let files = vec![Checked::path(Path::new("interval/merge.py")).expect("path")];
        let t = mutmut_table(&r, &files, &["interval".into(), "tests".into()], &["pytest.ini".into()], Seed(7));
        let text = merged_pyproject(Some("[project]\nname = \"x\"\n[tool.mutmut]\nsource_paths = [\"src\"]\n[tool.pytest.ini_options]\ntestpaths = [\"tests\"]\n"), t).expect("merges");
        assert!(text.contains("only_mutate = [\"interval/merge.py\"]"), "{text}");
        assert!(text.contains("pytest_add_cli_args_test_selection = [\"tests/test_props.py::test_merge\"]"), "{text}");
        assert!(text.contains("\"--hypothesis-seed\", \"7\"]"), "{text}");
        assert!(
            text.contains("source_paths = [\"interval\", \"tests\"]") && !text.contains("\"src\""),
            "the repository's table is replaced: {text}"
        );
        assert!(text.contains("testpaths = [\"tests\"]") && text.contains("name = \"x\""), "the rest is kept: {text}");
        assert!(merged_pyproject(Some("not = toml = at all"), toml::Table::new()).is_err());
        assert!(merged_pyproject(None, toml::Table::new()).expect("empty").contains("[tool.mutmut]"));
    }

    #[test]
    fn spinner_frames_are_dropped_from_the_tail() {
        let o = Output {
            code: Some(1),
            stdout: "⠴ Running stats\r⠦ Running stats\rFailed to run clean test\n".into(),
            stderr: "warning: x\n".into(),
        };
        assert_eq!(tail(&o), "Failed to run clean test\nwarning: x");
    }
}
