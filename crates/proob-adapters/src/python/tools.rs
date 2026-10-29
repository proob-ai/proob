//! The tools the Python adapter drives: `python3` from `PATH`, and inside it
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

pub fn install_hint(tool: &str) -> String {
    format!(
        "{tool} is not installed for `python3`: `python3 -m pip install -r tools/python-tools.txt` (it pins {tool}=={})",
        pinned(tool).unwrap_or("?")
    )
}

pub fn version_mismatch(tool: &str, expected: &str, found: &str) -> String {
    format!(
        "{tool} {found} is installed but this proob drives {tool} {expected} exactly (its results format and test selection are the contract the adapter reads): `python3 -m pip install {tool}=={expected}`"
    )
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

/// `python3` from `PATH` (the override's when `tool_env` sets one), its
/// version, then the distributions `needs` require.
pub fn resolve(root: &Path, needs: &[Need], tool_env: &[(OsString, OsString)]) -> Resolution {
    let Some(python) = find_on_path(PYTHON, tool_env) else {
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
            None => return Resolution::Missing { reason: install_hint(tool), versions },
            Some(version) => match accept(tool, &version) {
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
pub fn accept(tool: &str, found: &str) -> Result<(), String> {
    match (tool, pinned(tool)) {
        (MUTMUT, Some(expected)) if expected != found => Err(version_mismatch(tool, expected, found)),
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
        assert_eq!(accept(MUTMUT, pin), Ok(()));
        let err = accept(MUTMUT, "3.0.0").expect_err("refused");
        assert!(err.contains("mutmut 3.0.0 is installed") && err.contains(pin), "{err}");
        assert_eq!(accept(PYTEST, "0.1"), Ok(()));
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
            matches!(resolve(home.path(), &[Need::Pytest], &env), Resolution::Missing { reason, .. } if reason == PYTHON_HINT)
        );
    }
}
