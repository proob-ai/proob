//! `check` for a Python binding: one `python3 -m pytest` selecting exactly
//! the bound node id, with the adapter's seed and example budget reaching
//! Hypothesis through the `proob` settings profile (registered by a plugin
//! module proob writes to a temporary directory and puts first on
//! `PYTHONPATH`) and `--hypothesis-seed`, and a JUnit report at a temporary
//! path; the report is the only thing read. `--rootdir` is the adapter root,
//! so the report names the case by the node id's module path
//! (`tests.test_props`) whatever pytest's own rootdir rule would pick, and
//! the node id follows `--`, where pytest reads it as a path.
use crate::argv::{Checked, Command};
use crate::python::hypothesis::{
    self, NO_BYTECODE_ENV, PLUGIN_FILE, PLUGIN_MODULE, PROFILE, PROFILE_OPTION, SEED_OPTION, STORAGE_DIR_ENV,
};
use crate::python::tools::{Need, Resolution, resolve};
use crate::python::{CheckRef, PythonAdapter, last, parse_check_ref};
use crate::{
    AdapterError, Binding, Budget, Check, CheckOutcome, Seed, ToolVersions, Witness, WitnessPolicy, exit_text,
};
use proob_facts::parsers::junit::{CaseStatus, JUnitReport, TestCase, parse_junit};
use proob_spawn::Output;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Collection plus one property run, with room for a slow import.
pub const TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub const JUNIT_FILE: &str = "junit.xml";
/// Under the temporary directory: the plugin module's directory and Hypothesis's storage.
pub const PLUGIN_DIR: &str = "plugin";
pub const STORAGE_DIR: &str = "hypothesis";
/// How much of a runner's output an error carries.
pub const TAIL: usize = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub command: Command,
    pub junit: PathBuf,
    pub plugin: PathBuf,
}

/// Writes the profile plugin under `tmp` and returns its path.
pub fn write_plugin(tmp: &Path, budget: Budget) -> Result<PathBuf, AdapterError> {
    let io = |path: &Path, e: std::io::Error| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() };
    let dir = tmp.join(PLUGIN_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    let file = dir.join(PLUGIN_FILE);
    std::fs::write(&file, hypothesis::plugin_text(budget)).map_err(|e| io(&file, e))?;
    Ok(file)
}

/// The override's value, else the process's.
fn var(tool_env: &[(OsString, OsString)], key: &str) -> Option<OsString> {
    tool_env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).or_else(|| std::env::var_os(key))
}

/// What every Python run gets on top of the override: the plugin directory
/// first on `PYTHONPATH`, then `src` (the `src/` directory of a src-layout
/// project, so the copy under test is imported even when an editable install
/// points at another checkout of it), no bytecode written beside the sources,
/// Hypothesis's storage under `tmp`, and no colour.
pub fn environment(tmp: &Path, src: Option<&Path>, tool_env: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    let plugin = tmp.join(PLUGIN_DIR).into_os_string();
    let lead: Vec<OsString> = std::iter::once(plugin.clone()).chain(src.map(|p| p.as_os_str().to_owned())).collect();
    let rest: Vec<OsString> = var(tool_env, "PYTHONPATH")
        .filter(|v| !v.is_empty())
        .map(|existing| std::env::split_paths(&existing).map(PathBuf::into_os_string).collect())
        .unwrap_or_default();
    let pythonpath = std::env::join_paths(lead.into_iter().chain(rest)).unwrap_or(plugin);
    vec![
        (OsString::from("PYTHONPATH"), pythonpath),
        (OsString::from(NO_BYTECODE_ENV), OsString::from("1")),
        (OsString::from(STORAGE_DIR_ENV), tmp.join(STORAGE_DIR).into_os_string()),
        (OsString::from("PY_COLORS"), OsString::from("0")),
    ]
}

/// The argv for one check, given the temporary directory the plugin and the
/// report live in. Pure apart from writing the plugin module.
pub fn plan(
    python: &Path,
    root: &Path,
    r: &CheckRef,
    tmp: &Path,
    budget: Budget,
    seed: Seed,
    tool_env: &[(OsString, OsString)],
) -> Result<Plan, AdapterError> {
    let plugin = write_plugin(tmp, budget)?;
    let junit = tmp.join(JUNIT_FILE);
    let command = Command::new(python, TIMEOUT)
        .arg("-m")
        .arg("pytest")
        .own("--rootdir", root)
        .arg("-p")
        .arg("no:cacheprovider")
        .arg("-p")
        .arg(PLUGIN_MODULE)
        .arg(PROFILE_OPTION)
        .arg(PROFILE)
        .opt(SEED_OPTION, &Checked::number(seed.0))
        .own("--junitxml", &junit)
        .arg("-q")
        .tail(&r.node_id())
        .cwd(root)
        .envs(tool_env)
        .envs(&environment(tmp, src_layout(root).as_deref(), tool_env));
    Ok(Plan { command, junit, plugin })
}

/// `root/src` when the project keeps its package there (the src layout).
pub fn src_layout(root: &Path) -> Option<PathBuf> {
    Some(root.join("src")).filter(|p| p.is_dir())
}

pub fn run(
    adapter: &PythonAdapter,
    repo: &Path,
    binding: &Binding,
    budget: Budget,
    seed: Seed,
) -> Result<Check, AdapterError> {
    let r = parse_check_ref(&binding.check_ref)?;
    let root = adapter.root_in(repo);
    let tools = match resolve(repo, &root, &[Need::Pytest], adapter.tool_env()) {
        Resolution::Ready(tools) => tools,
        Resolution::Missing { reason, versions } => return Ok(errored(reason, versions)),
    };
    let tmp = tempfile::tempdir().map_err(|e| AdapterError::Temp { message: e.to_string() })?;
    let plan = plan(&tools.python, &root, &r, tmp.path(), budget, seed, adapter.tool_env())?;
    let out = match plan.command.run() {
        Ok(out) => out,
        Err(e) => return Ok(errored(format!("pytest did not run: {e}"), tools.versions)),
    };
    let report = match std::fs::read_to_string(&plan.junit) {
        Ok(text) => text,
        Err(_) => return Ok(errored(no_report(&out), tools.versions)),
    };
    let outcome = match parse_junit(&report) {
        Ok(parsed) => outcome_of(&parsed, &r, budget, seed, adapter.witness(), &out),
        Err(e) => CheckOutcome::Errored { reason: format!("pytest's JUnit report could not be read: {e}") },
    };
    Ok(Check { outcome, tool_versions: tools.versions })
}

/// Why pytest wrote no report. A configuration whose `addopts` names a
/// plugin's options (`--cov`, `-n auto`) that this environment lacks is the
/// common case, so it gets its own one-line fix.
pub fn no_report(out: &Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    match stderr.lines().find(|l| l.contains("unrecognized arguments:")) {
        Some(line) => format!(
            "pytest refused its own configuration ({}): the project's pytest settings use a plugin that is not installed here; install it in this environment (for --cov, pytest-cov), or set PYTEST_ADDOPTS=\"-o addopts=\" to run without those settings",
            line.trim().trim_start_matches("ERROR: ").trim_start_matches("error: ")
        ),
        None => format!("pytest produced no report (exit {}); last output: {}", exit_text(out.code), tail(out)),
    }
}

fn errored(reason: String, tool_versions: ToolVersions) -> Check {
    Check { outcome: CheckOutcome::Errored { reason }, tool_versions }
}

/// The end of what the runner printed: stderr when it said anything there, else stdout.
pub fn tail(out: &Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let text = if stderr.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { stderr };
    last(text.trim(), TAIL)
}

/// pytest's `classname` for a node id's file: the path without `.py`, dotted.
pub fn classname(file: &Checked) -> String {
    file.as_str().strip_suffix(".py").unwrap_or(file.as_str()).replace('/', ".")
}

/// The bound test's case in the report, mapped to an outcome.
pub fn outcome_of(
    report: &JUnitReport,
    r: &CheckRef,
    budget: Budget,
    seed: Seed,
    policy: WitnessPolicy,
    out: &Output,
) -> CheckOutcome {
    match report.case(&classname(&r.file), r.function.as_str()).map(|c| &c.status) {
        Some(CaseStatus::Passed) => CheckOutcome::NoCounterexample { budget, replay: seed },
        Some(CaseStatus::Failed { message, body }) => CheckOutcome::Counterexample {
            witness: Witness::new(policy, &hypothesis::witness_text(message, body)),
            replay: seed,
        },
        Some(CaseStatus::Skipped) => {
            CheckOutcome::Errored { reason: format!("bound test `{}` was skipped by pytest", r.node_id().as_str()) }
        }
        None => CheckOutcome::Errored { reason: not_found(report, r, out) },
    }
}

/// Why the bound case is absent: a collection error pytest reported as a
/// case of its own, or no such test.
fn not_found(report: &JUnitReport, r: &CheckRef, out: &Output) -> String {
    match report.cases.iter().find_map(collection_error) {
        Some((name, reason)) => format!("pytest could not collect `{name}`: {reason}"),
        None => format!(
            "bound test not found: pytest ran no test `{}` (exit {}); was it renamed or removed? last output: {}",
            r.node_id().as_str(),
            exit_text(out.code),
            tail(out)
        ),
    }
}

/// A case pytest emits for a module it could not import: no classname, an
/// `error` child; the reason is the last `E ` line of the traceback.
fn collection_error(case: &TestCase) -> Option<(String, String)> {
    match &case.status {
        CaseStatus::Failed { message, body } if case.classname.is_empty() || message == "collection failure" => {
            let reason = body
                .lines()
                .rev()
                .map(str::trim)
                .find_map(|l| l.strip_prefix("E "))
                .map(str::trim)
                .unwrap_or(message.as_str());
            Some((case.name.clone(), last(reason, TAIL)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classnames_follow_the_node_id() {
        let file = Checked::path(Path::new("tests/unit/test_props.py")).expect("path");
        assert_eq!(classname(&file), "tests.unit.test_props");
        assert_eq!(classname(&Checked::path(Path::new("test_x.py")).expect("path")), "test_x");
    }

    #[test]
    fn a_missing_pytest_plugin_is_named_with_its_fix() {
        let out = Output { code: Some(4), stdout: Vec::new(), stderr: b"ERROR: usage: __main__.py [options] [file_or_dir]\n__main__.py: error: unrecognized arguments: --cov=semver --cov-report=term\n".to_vec() };
        let why = no_report(&out);
        assert!(
            why.contains("unrecognized arguments: --cov=semver")
                && why.contains("pytest-cov")
                && why.contains("PYTEST_ADDOPTS"),
            "{why}"
        );
        let other = Output { code: Some(2), stdout: b"collected 0 items".to_vec(), stderr: Vec::new() };
        assert!(
            no_report(&other).starts_with("pytest produced no report (exit"),
            "other failures keep the generic reason"
        );
    }

    #[test]
    fn the_plugin_directory_leads_pythonpath() {
        let tmp = Path::new("/tmp/proob-x");
        let env = environment(tmp, None, &[(OsString::from("PYTHONPATH"), OsString::from("/a:/b"))]);
        let pythonpath =
            env.iter().find(|(k, _)| k == "PYTHONPATH").map(|(_, v)| v.to_string_lossy().into_owned()).expect("set");
        assert_eq!(pythonpath, "/tmp/proob-x/plugin:/a:/b");
        let env = environment(tmp, Some(Path::new("/w/src")), &[(OsString::from("PYTHONPATH"), OsString::from("/a"))]);
        let pythonpath =
            env.iter().find(|(k, _)| k == "PYTHONPATH").map(|(_, v)| v.to_string_lossy().into_owned()).expect("set");
        assert_eq!(pythonpath, "/tmp/proob-x/plugin:/w/src:/a", "the copy's src comes before anything installed");
        assert!(env.iter().any(|(k, v)| k == STORAGE_DIR_ENV && v == tmp.join(STORAGE_DIR).as_os_str()));
    }

    #[test]
    fn a_collection_error_is_named() {
        let case = TestCase {
            classname: String::new(),
            name: "tests.test_broken".into(),
            status: CaseStatus::Failed {
                message: "collection failure".into(),
                body: "ImportError while importing\nE   ModuleNotFoundError: No module named 'x'\n".into(),
            },
        };
        assert_eq!(
            collection_error(&case),
            Some(("tests.test_broken".into(), "ModuleNotFoundError: No module named 'x'".into()))
        );
    }
}
