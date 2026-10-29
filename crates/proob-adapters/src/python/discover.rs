//! Discovery: every `.py` file under the adapter root is scanned for tokens
//! (skipping hidden directories, `__pycache__/`, `mutants/`, `node_modules/`,
//! `target/`, and virtual environments, recognised by their `pyvenv.cfg`),
//! and each bound test's binding set is computed by the rule in the module
//! doc: the test file, the `conftest.py` files on its directory path, and
//! the pytest configuration file that applies to it.
use crate::python::scan::{Bound, scan};
use crate::python::{PythonAdapter, check_ref};
use crate::rust::discover::hash_of;
use crate::{AdapterError, Binding, Location, TokenProblem};
use proob_core::types::Kind;
use proob_facts::ulid;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SKIPPED_DIRS: [&str; 4] = ["__pycache__", "mutants", "node_modules", "target"];
pub const VENV_MARKER: &str = "pyvenv.cfg";
pub const CONFTEST: &str = "conftest.py";
/// pytest's configuration files, in its order of precedence within one directory.
pub const CONFIG_FILES: [&str; 5] = ["pytest.ini", ".pytest.ini", "pyproject.toml", "tox.ini", "setup.cfg"];

/// pytest's default `python_files`: `test_*.py` and `*_test.py`.
pub fn is_test_file(name: &str) -> bool {
    name.ends_with(".py") && (name.starts_with("test_") || name.ends_with("_test.py"))
}

/// A directory discovery and the score copy leave alone.
pub fn skip_dir(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.starts_with('.') || SKIPPED_DIRS.contains(&name) || path.join(VENV_MARKER).is_file()
}

pub fn discover(adapter: &PythonAdapter, repo: &Path) -> Result<Vec<Binding>, AdapterError> {
    let root = adapter.root_in(repo);
    let mut bindings: Vec<Binding> = Vec::new();
    for rel in python_files(&root)? {
        let text = read(&root.join(&rel))?;
        let test_file = rel.file_name().and_then(|n| n.to_str()).is_some_and(is_test_file);
        for token in scan(&text).tokens {
            let at = Location { path: adapter.repo_relative(&rel), line: token.line };
            match token.bound {
                Bound::Problem(problem) => return Err(AdapterError::Token { at, problem }),
                Bound::Test { .. } if !test_file => {
                    return Err(AdapterError::Token { at, problem: TokenProblem::NotATestFile });
                }
                Bound::Test { id, function } => bindings.push(binding(adapter, repo, &rel, id, &function, at)?),
            }
        }
    }
    check_distinct(&bindings)?;
    bindings.sort_by(|a, b| (&a.token.path, a.token.line).cmp(&(&b.token.path, b.token.line)));
    Ok(bindings)
}

fn binding(
    adapter: &PythonAdapter,
    repo: &Path,
    rel: &Path,
    id: u128,
    function: &str,
    token: Location,
) -> Result<Binding, AdapterError> {
    let mut files: Vec<PathBuf> =
        binding_files(&adapter.root_in(repo), rel).iter().map(|f| adapter.repo_relative(f)).collect();
    files.sort();
    files.dedup();
    let binding_hash = hash_of(repo, &files)?;
    Ok(Binding { id, kind: Kind::Test, check_ref: check_ref(rel, function), files, binding_hash, token })
}

fn check_distinct(bindings: &[Binding]) -> Result<(), AdapterError> {
    let mut seen: BTreeMap<u128, &Location> = BTreeMap::new();
    for b in bindings {
        if let Some(first) = seen.insert(b.id, &b.token) {
            return Err(AdapterError::Duplicate {
                id: ulid::encode(b.id),
                first: first.clone(),
                second: b.token.clone(),
            });
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<String, AdapterError> {
    std::fs::read_to_string(path).map_err(|e| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() })
}

/// Every `.py` file under `root`, root-relative, sorted by path.
pub fn python_files(root: &Path) -> Result<Vec<PathBuf>, AdapterError> {
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), AdapterError> {
    for path in entries(dir)? {
        if path.is_dir() {
            if !skip_dir(&path) {
                walk(root, &path, out)?;
            }
        } else if path.extension().is_some_and(|e| e == "py") {
            out.push(path.strip_prefix(root).map(Path::to_path_buf).unwrap_or(path));
        }
    }
    Ok(())
}

/// The entries of `dir`, sorted by name.
pub fn entries(dir: &Path) -> Result<Vec<PathBuf>, AdapterError> {
    let read =
        std::fs::read_dir(dir).map_err(|e| AdapterError::Io { path: dir.to_path_buf(), message: e.to_string() })?;
    let mut paths: Vec<PathBuf> = read.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    Ok(paths)
}

/// The files that give the test in `test_file` (root-relative) its meaning,
/// root-relative and unsorted: the file, its conftests, its pytest config.
pub fn binding_files(root: &Path, test_file: &Path) -> Vec<PathBuf> {
    [test_file.to_path_buf()]
        .into_iter()
        .chain(conftests(root, test_file))
        .chain(pytest_config(root, test_file))
        .collect()
}

/// The directories from the root down to the test file's, root-relative
/// (the first is empty).
fn lineage(test_file: &Path) -> Vec<PathBuf> {
    let parent = test_file.parent().unwrap_or(Path::new(""));
    std::iter::once(PathBuf::new())
        .chain(parent.components().scan(PathBuf::new(), |acc, c| {
            acc.push(c);
            Some(acc.clone())
        }))
        .collect()
}

/// Every `conftest.py` pytest imports for the test, root down.
pub fn conftests(root: &Path, test_file: &Path) -> Vec<PathBuf> {
    lineage(test_file).into_iter().map(|d| d.join(CONFTEST)).filter(|c| root.join(c).is_file()).collect()
}

/// The configuration file pytest reads for the test: from its directory
/// upward, the first directory holding one, by pytest's precedence there.
pub fn pytest_config(root: &Path, test_file: &Path) -> Option<PathBuf> {
    lineage(test_file).into_iter().rev().find_map(|d| config_in(&root.join(&d)).map(|name| d.join(name)))
}

/// The pytest configuration file in one directory, if any.
pub fn config_in(dir: &Path) -> Option<&'static str> {
    CONFIG_FILES.into_iter().find(|name| {
        let path = dir.join(name);
        match *name {
            "pyproject.toml" => std::fs::read_to_string(&path).is_ok_and(|t| has_pytest_table(&t)),
            "tox.ini" => has_section(&path, "[pytest]"),
            "setup.cfg" => has_section(&path, "[tool:pytest]"),
            _ => path.is_file(),
        }
    })
}

/// `[tool.pytest]` or `[tool.pytest.ini_options]`: the table that makes
/// pytest read a `pyproject.toml`.
fn has_pytest_table(text: &str) -> bool {
    match text.parse::<toml::Table>() {
        Ok(t) => t.get("tool").and_then(|tool| tool.get("pytest")).is_some(),
        Err(_) => text.lines().any(|l| l.trim_start().starts_with("[tool.pytest")),
    }
}

fn has_section(path: &Path, header: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|t| t.lines().any(|l| l.trim() == header))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_names_and_lineage() {
        assert!(is_test_file("test_props.py") && is_test_file("props_test.py"));
        assert!(!is_test_file("conftest.py") && !is_test_file("merge.py") && !is_test_file("test_data.json"));
        let dirs = lineage(Path::new("tests/unit/test_x.py"));
        assert_eq!(dirs, vec![PathBuf::new(), PathBuf::from("tests"), PathBuf::from("tests/unit")]);
        assert_eq!(lineage(Path::new("test_x.py")), vec![PathBuf::new()]);
    }

    #[test]
    fn pyproject_counts_only_with_a_pytest_table() {
        assert!(has_pytest_table("[tool.pytest.ini_options]\ntestpaths = [\"tests\"]\n"));
        assert!(has_pytest_table("[tool.pytest]\n"));
        assert!(!has_pytest_table("[project]\nname = \"x\"\n"));
        assert!(
            has_pytest_table("[tool.pytest.ini_options\nbroken = [\n"),
            "an unparsable file with the table still counts"
        );
    }

    #[test]
    fn config_precedence_within_a_directory_and_upward() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("tests/unit")).expect("mkdir");
        std::fs::write(root.join("pyproject.toml"), "[tool.pytest.ini_options]\n").expect("write");
        std::fs::write(root.join("tests/unit/test_x.py"), "").expect("write");
        assert_eq!(pytest_config(root, Path::new("tests/unit/test_x.py")), Some(PathBuf::from("pyproject.toml")));
        std::fs::write(root.join("tests/tox.ini"), "[tox]\n").expect("write");
        assert_eq!(
            pytest_config(root, Path::new("tests/unit/test_x.py")),
            Some(PathBuf::from("pyproject.toml")),
            "a tox.ini without [pytest] is not pytest's"
        );
        std::fs::write(root.join("tests/tox.ini"), "[pytest]\n").expect("write");
        assert_eq!(
            pytest_config(root, Path::new("tests/unit/test_x.py")),
            Some(PathBuf::from("tests/tox.ini")),
            "the nearest directory wins"
        );
        std::fs::write(root.join("tests/pytest.ini"), "").expect("write");
        assert_eq!(
            pytest_config(root, Path::new("tests/unit/test_x.py")),
            Some(PathBuf::from("tests/pytest.ini")),
            "pytest.ini wins even when empty"
        );
        std::fs::write(root.join("conftest.py"), "").expect("write");
        std::fs::write(root.join("tests/unit/conftest.py"), "").expect("write");
        assert_eq!(
            conftests(root, Path::new("tests/unit/test_x.py")),
            vec![PathBuf::from("conftest.py"), PathBuf::from("tests/unit/conftest.py")]
        );
        assert!(skip_dir(&root.join(".venv")) && skip_dir(&root.join("__pycache__")));
        std::fs::write(root.join("tests/unit/pyvenv.cfg"), "").expect("write");
        assert!(skip_dir(&root.join("tests/unit")), "a pyvenv.cfg marks a virtual environment");
    }
}
