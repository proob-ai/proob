//! The tools the Python adapter drives: proob's own environment
//! (`proob/.cache/python`) when it exists, else `python3` from `PATH`, and inside it
//! pytest, Hypothesis, and mutmut, located and version-probed before any run
//! so every result records what produced it and a missing tool is a closed
//! `Errored` with an install hint rather than a stack trace. The versions
//! proob is built for are the pins in `tools/python-tools.txt`, embedded
//! here: mutmut must match its pin exactly (the adapter reads its results
//! format, and mutmut's test selection and exit codes are the contract);
//! pytest and Hypothesis are recorded only. Versions are read with
//! `importlib.metadata` in one interpreter run, not by running the tools:
//! `mutmut --version` imports mutmut's command module, which loads a project
//! configuration from the working directory and fails without one.
use crate::ToolVersions;
use crate::argv::Command;
use crate::rust::tools::find_on_path;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `tools/python-tools.txt`: the one place the versions are written.
pub const PINS: &str = include_str!("../../../../tools/python-tools.txt");
pub const PYTHON: &str = "python3";
pub const PYTEST: &str = "pytest";
pub const HYPOTHESIS: &str = "hypothesis";
pub const MUTMUT: &str = "mutmut";
/// proob's own virtual environment, repo-relative and gitignored: installing
/// the tools there leaves the user's Python and their `.venv` untouched.
pub const OWN_ENV: &str = "proob/.cache/python";
const PROBE_TIMEOUT: Duration = Duration::from_secs(120);
/// Prints `<tool> <version>` or `<tool> missing` per line.
const PROBE: &str = "import importlib.metadata as m\nfor d in ('pytest', 'hypothesis', 'mutmut'):\n    try:\n        print(d, m.version(d))\n    except m.PackageNotFoundError:\n        print(d, 'missing')\n";

pub const PYTHON_HINT: &str = "python3 was not found on PATH: install Python 3.10 or newer (in CI: actions/setup-python) and activate the environment that holds pytest, hypothesis, and mutmut, so `python3` is that interpreter";

/// The `name==version` pins, in file order.
pub fn pins() -> Vec<(&'static str, &'static str)> {
    PINS.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once("=="))
        .collect()
}

pub fn pinned(tool: &str) -> Option<&'static str> {
    pins().into_iter().find(|(name, _)| *name == tool).map(|(_, version)| version)
}

/// Names the whole pinned set, so the command works from any repository.
/// Where the install commands of a hint go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// proob's own environment is the interpreter in use.
    pub own: bool,
    /// ` -e <dir>` or ` -r <dir>/requirements.txt`: the project, so its tests import.
    pub project: String,
}

impl Fix {
    pub fn of(repo: &Path, root: &Path) -> Fix {
        Fix { own: own_python(repo).is_some(), project: project_install(repo, root) }
    }

    /// One command, with the `python` it installs into: proob's environment,
    /// made first when it does not exist yet; never the user's own.
    fn command(&self, what: &str) -> String {
        let py = format!("{OWN_ENV}/bin/python");
        match self.own {
            true => format!("`{py} -m pip install {what}`"),
            false => format!("`python3 -m venv {OWN_ENV} && {py} -m pip install {what}{}`", self.project),
        }
    }
}

fn all_pins() -> String {
    pins().iter().map(|(name, v)| format!("{name}=={v}")).collect::<Vec<_>>().join(" ")
}

pub fn install_hint(tool: &str, fix: &Fix) -> String {
    match fix.own {
        true => format!("{tool} is not installed for proob's environment {OWN_ENV}: {}", fix.command(&all_pins())),
        false => format!(
            "{tool} is not installed for `python3`: to leave your own Python untouched, give proob its own environment, which it uses whenever it exists: {}, then install your tests' own dependencies with the same pip (in CI: `python3 -m pip install {}`)",
            fix.command(&all_pins()),
            all_pins()
        ),
    }
}

pub fn version_mismatch(tool: &str, expected: &str, found: &str, fix: &Fix) -> String {
    format!(
        "{tool} {found} is installed but this proob drives {tool} {expected} exactly (its results format and test selection are the contract the adapter reads): {}",
        fix.command(&format!("{tool}=={expected}"))
    )
}

/// The interpreter of proob's own environment, when it has been made.
pub fn own_python(repo: &Path) -> Option<PathBuf> {
    ["bin/python3", "Scripts/python.exe"].iter().map(|p| repo.join(OWN_ENV).join(p)).find(|p| p.is_file())
}

/// The pip arguments that install the project at `root`, by the file it is described in.
fn project_install(repo: &Path, root: &Path) -> String {
    let dir = match root.strip_prefix(repo).map(|r| r.display().to_string()) {
        Ok(r) if !r.is_empty() => r,
        _ => ".".to_string(),
    };
    match (
        root.join("pyproject.toml").is_file() || root.join("setup.py").is_file(),
        root.join("requirements.txt").is_file(),
    ) {
        (true, true) => format!(" -e {dir} -r {dir}/requirements.txt"),
        (true, false) => format!(" -e {dir}"),
        (false, true) => format!(" -r {dir}/requirements.txt"),
        (false, false) => String::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// pytest with the Hypothesis plugin.
    Pytest,
    Mutmut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub python: PathBuf,
    pub versions: ToolVersions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ready(Tools),
    /// Why a run cannot start, and what was learnt before that.
    Missing {
        reason: String,
        versions: ToolVersions,
    },
}

/// proob's own environment when it exists, else `python3` from `PATH` (the
/// override's when `tool_env` sets one); its version, then the distributions
/// `needs` require.
pub fn resolve(repo: &Path, root: &Path, needs: &[Need], tool_env: &[(OsString, OsString)]) -> Resolution {
    let fix = Fix::of(repo, root);
    let Some(python) = own_python(repo).or_else(|| find_on_path(PYTHON, tool_env)) else {
        return Resolution::Missing { reason: PYTHON_HINT.to_string(), versions: Vec::new() };
    };
    let mut versions = Vec::new();
    match probe(&python, root, &["--version"], tool_env) {
        Ok(text) => versions.push(("python".to_string(), second_word(&text))),
        Err(e) => return Resolution::Missing { reason: format!("python3 --version failed: {e}"), versions },
    }
    let found = match probe(&python, root, &["-c", PROBE], tool_env) {
        Ok(text) => parse_probe(&text),
        Err(e) => {
            return Resolution::Missing { reason: format!("python3 could not report its packages: {e}"), versions };
        }
    };
    for tool in required(needs) {
        match found.get(tool).cloned().flatten() {
            None => return Resolution::Missing { reason: install_hint(tool, &fix), versions },
            Some(version) => match accept(tool, &version, &fix) {
                Ok(()) => versions.push((tool.to_string(), version)),
                Err(reason) => return Resolution::Missing { reason, versions },
            },
        }
    }
    Resolution::Ready(Tools { python, versions })
}

/// The distributions behind `needs`, each once, in probe order.
fn required(needs: &[Need]) -> Vec<&'static str> {
    [PYTEST, HYPOTHESIS, MUTMUT]
        .into_iter()
        .filter(|tool| {
            needs.iter().any(|n| matches!((n, *tool), (Need::Pytest, PYTEST | HYPOTHESIS) | (Need::Mutmut, MUTMUT)))
        })
        .collect()
}

/// Whether an installed version may drive a run: mutmut only at its pin.
pub fn accept(tool: &str, found: &str, fix: &Fix) -> Result<(), String> {
    match (tool, pinned(tool)) {
        (MUTMUT, Some(expected)) if expected != found => Err(version_mismatch(tool, expected, found, fix)),
        _ => Ok(()),
    }
}

/// The probe's lines: `name version`, or `name missing` as `None`.
pub fn parse_probe(text: &str) -> BTreeMap<String, Option<String>> {
    text.lines()
        .filter_map(|l| l.trim().split_once(' '))
        .map(|(name, version)| (name.to_string(), (version != "missing").then(|| version.trim().to_string())))
        .collect()
}

fn probe(
    python: &Path,
    root: &Path,
    args: &[&'static str],
    tool_env: &[(OsString, OsString)],
) -> Result<String, String> {
    let mut command = Command::new(python, PROBE_TIMEOUT).cwd(root).envs(tool_env);
    for a in args {
        command = command.arg(a);
    }
    let out = command.run().map_err(|e| e.to_string())?;
    if !out.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    // Python 3 prints its version on stdout; the fallback covers interpreters that still use stderr.
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(if stdout.trim().is_empty() { String::from_utf8_lossy(&out.stderr).into_owned() } else { stdout.into_owned() })
}

/// `Python 3.13.2` → `3.13.2`.
fn second_word(text: &str) -> String {
    text.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("unknown").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pins_file_names_all_three_tools() {
        let names: Vec<&str> = pins().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec![PYTEST, HYPOTHESIS, MUTMUT]);
        assert!(pinned(MUTMUT).is_some_and(|v| v.starts_with("3.")));
    }

    #[test]
    fn only_mutmut_is_held_to_its_pin() {
        let pin = pinned(MUTMUT).expect("pinned");
        let fix = Fix { own: true, project: String::new() };
        assert_eq!(accept(MUTMUT, pin, &fix), Ok(()));
        let err = accept(MUTMUT, "3.0.0", &fix).expect_err("refused");
        assert!(err.contains("mutmut 3.0.0 is installed") && err.contains(pin), "{err}");
        assert!(err.ends_with(&format!("`{OWN_ENV}/bin/python -m pip install mutmut=={pin}`")), "{err}");
        assert_eq!(accept(PYTEST, "0.1", &fix), Ok(()));
    }

    #[test]
    fn probe_lines_and_needs() {
        let found = parse_probe("pytest 9.1.1\nhypothesis missing\nmutmut 3.8.0\n");
        assert_eq!(found.get("pytest").cloned().flatten().as_deref(), Some("9.1.1"));
        assert_eq!(found.get("hypothesis").cloned().flatten(), None);
        assert_eq!(required(&[Need::Pytest]), vec![PYTEST, HYPOTHESIS]);
        assert_eq!(required(&[Need::Mutmut, Need::Pytest]), vec![PYTEST, HYPOTHESIS, MUTMUT]);
        assert_eq!(second_word("Python 3.13.2\n"), "3.13.2");
    }

    #[test]
    fn an_empty_path_finds_no_interpreter() {
        let home = tempfile::tempdir().expect("tempdir");
        let env = vec![(OsString::from("PATH"), home.path().join("nothing").into_os_string())];
        assert!(
            matches!(resolve(home.path(), home.path(), &[Need::Pytest], &env), Resolution::Missing { reason, .. } if reason == PYTHON_HINT)
        );
    }

    /// The fix never installs into the user's Python: it makes proob's own
    /// environment and installs the project into it beside the pins.
    #[test]
    fn a_missing_tool_is_installed_into_proobs_own_environment() {
        let repo = tempfile::tempdir().expect("tempdir");
        let root = repo.path().join("svc");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(root.join("pyproject.toml"), "").expect("write");
        let hint = install_hint(PYTEST, &Fix::of(repo.path(), &root));
        let made = format!("`python3 -m venv {OWN_ENV} && {OWN_ENV}/bin/python -m pip install {} -e svc`", all_pins());
        assert!(hint.contains(&made), "{hint}");
        assert_eq!(hint.matches("python3 -m pip install").count(), 1, "the only bare pip is CI's: {hint}");
        std::fs::write(root.join("requirements.txt"), "").expect("write");
        assert_eq!(project_install(repo.path(), &root), " -e svc -r svc/requirements.txt");
        assert_eq!(project_install(repo.path(), repo.path()), "");
    }

    /// With proob's environment made, it is the interpreter even when `PATH` has none.
    #[cfg(unix)]
    #[test]
    fn proobs_own_environment_is_used_whenever_it_exists() {
        use std::os::unix::fs::PermissionsExt;
        let repo = tempfile::tempdir().expect("tempdir");
        let bin = repo.path().join(OWN_ENV).join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        let shim = bin.join("python3");
        std::fs::write(&shim, "#!/bin/sh\ncase \"$1\" in\n  --version) echo \"Python 3.12.0\" ;;\n  *) printf 'pytest missing\\nhypothesis missing\\nmutmut missing\\n' ;;\nesac\n").expect("write");
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let env = vec![(OsString::from("PATH"), repo.path().join("nothing").into_os_string())];
        match resolve(repo.path(), repo.path(), &[Need::Pytest], &env) {
            Resolution::Missing { reason, versions } => {
                assert_eq!(versions, vec![("python".to_string(), "3.12.0".to_string())]);
                assert!(reason.starts_with(&format!("pytest is not installed for proob's environment {OWN_ENV}: `{OWN_ENV}/bin/python -m pip install pytest==")), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }
}
