//! The Python adapter: Hypothesis tests through pytest for `check`, mutmut
//! for `score`, a source scan for `discover`.
//!
//! # Binding convention
//!
//! A test is bound by a comment-only line holding exactly one token,
//! `# proob:<ULID>`, directly above the test function, above its decorators:
//!
//! ```text
//! # proob:01K8V5NQ4A0F3R7Y2M9D6P1XPA
//! @given(st.lists(st.from_type(Interval)))
//! def test_merge_yields_disjoint_sorted_intervals(intervals):
//!     …
//! ```
//!
//! Only blank lines, other comments, and decorators (multi-line ones
//! included) may stand between the token and the `def`. The `def` must be at
//! module level (a method or a nested function is an error: node ids inside
//! classes are not supported), its name must start with `test` (pytest's
//! default `python_functions`), and the file must be one pytest collects by
//! default, `test_*.py` or `*_test.py` (`python_files` overrides are not
//! read). What follows `proob:` must be one ULID and nothing else, so
//! `proob:<ULID>:v2` and a truncated id are errors that `validate` reports.
//! Each property binds at most one test. The check reference is pytest's
//! node id for the test, `<file>::<function>` with the file relative to the
//! adapter root, such as `tests/test_props.py::test_bucket_never_goes_negative`.
//!
//! # Binding set
//!
//! The test file; every `conftest.py` on the directory path from the adapter
//! root down to the test file's directory (pytest imports each of them for
//! that test); and the configuration file pytest reads for it: walking from
//! the test file's directory up to the adapter root, the first directory
//! holding one of `pytest.ini`, `.pytest.ini`, `pyproject.toml` (when it has
//! a `[tool.pytest…]` table), `tox.ini` (with a `[pytest]` section), or
//! `setup.cfg` (with `[tool:pytest]`), in pytest's own order within a
//! directory. Repo-relative, sorted, deduplicated, hashed as
//! `proob-binding-v1` over each member's normalised content. Python imports
//! are deliberately not followed: a helper module a test imports is not a
//! member unless it is a `conftest.py`, which is where shared strategies
//! belong (the fixture's `tests/conftest.py` registers them with
//! `st.register_type_strategy`). Dependency pins (`requirements*.txt`, lock
//! files) are not members, as `Cargo.lock` is not for Rust. The settings
//! profile plugin proob writes for a run is proob's, versioned by the proob
//! binary, and is not a member either.
//!
//! # Running
//!
//! `check` runs `python3 -m pytest` from the adapter root for the one node
//! id: `--rootdir` fixed to the root, the cache provider off, a plugin module
//! proob writes to a temporary directory and puts first on `PYTHONPATH`
//! (`hypothesis`) that registers the `proob` settings profile (the adapter's
//! example budget, no example database, no deadline, the reproduction blob
//! printed), `--hypothesis-profile proob`, `--hypothesis-seed` set to the
//! adapter's seed, and a JUnit report at a temporary path, the only thing
//! read. `HYPOTHESIS_STORAGE_DIRECTORY` and `PYTHONDONTWRITEBYTECODE` keep
//! Hypothesis's `.hypothesis/` and Python's `__pycache__/` out of the tree. A
//! test's own `@settings(...)` still overrides the profile, as a proptest
//! config inside a test overrides proptest's environment. `score` copies the
//! adapter root to a temporary directory, writes a `[tool.mutmut]` table into
//! the copy's `pyproject.toml`, and runs `python3 -m mutmut run` there
//! (`score` says exactly what mutmut lets proob restrict); the counts come
//! from the `.meta` file mutmut writes for each scope file. `python3` is the
//! first on `PATH`, so activate the environment that holds the tools before
//! running proob; the mutmut version is pinned in `tools/python-tools.txt`
//! and any other is refused. mutmut needs `os.fork`: on Windows `score`
//! returns a runner error saying so. Both record the `python`, `pytest`,
//! `hypothesis`, and (score) `mutmut` versions.
pub mod check;
pub mod discover;
pub mod hypothesis;
pub mod scan;
pub mod score;
pub mod tools;

use crate::argv::Checked;
use crate::mutation::{self, Limits};
use crate::{Adapter, AdapterError, Binding, Budget, Check, Language, RegistryError, Score, Seed, WitnessPolicy};
use proob_facts::config::Adapter as AdapterConfig;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const RUNNER: &str = "pytest";
pub const MUTATION_TOOL: &str = "mutmut";

#[derive(Debug, Clone)]
pub struct PythonAdapter {
    /// The adapter root, relative to the repository (`.` for the root itself).
    root: PathBuf,
    witness: WitnessPolicy,
    /// Variables set on every tool process; `PATH` here also decides which
    /// `python3` runs. Empty in production: the process environment.
    tool_env: Vec<(OsString, OsString)>,
    /// The score's mutant cap and parallelism (`mutation`).
    limits: Limits,
}

impl PythonAdapter {
    /// An adapter at `root` (repo-relative) with full witnesses.
    pub fn at(root: impl Into<PathBuf>) -> PythonAdapter {
        PythonAdapter { root: root.into(), witness: WitnessPolicy::Full, tool_env: Vec::new(), limits: Limits::DEFAULT }
    }

    pub fn with_witness(self, witness: WitnessPolicy) -> PythonAdapter {
        PythonAdapter { witness, ..self }
    }

    /// A tool environment override, for sandboxes and tests (a `PATH` with
    /// another `python3`, or none).
    pub fn with_tool_env(self, tool_env: Vec<(OsString, OsString)>) -> PythonAdapter {
        PythonAdapter { tool_env, ..self }
    }

    pub fn tool_env(&self) -> &[(OsString, OsString)] {
        &self.tool_env
    }

    /// From a config entry whose root already passed the path grammar.
    pub fn configured(
        a: &AdapterConfig,
        root: Checked,
        witness: WitnessPolicy,
    ) -> Result<PythonAdapter, RegistryError> {
        if a.runner != RUNNER {
            return Err(RegistryError::UnsupportedRunner {
                language: Language::Python,
                runner: a.runner.clone(),
                expected: RUNNER,
            });
        }
        if a.mutation_tool != MUTATION_TOOL {
            return Err(RegistryError::UnsupportedMutationTool {
                language: Language::Python,
                tool: a.mutation_tool.clone(),
                expected: MUTATION_TOOL,
            });
        }
        let limits = mutation::limits(a, Language::Python)?;
        Ok(PythonAdapter { root: PathBuf::from(root.as_str()), witness, tool_env: Vec::new(), limits })
    }

    /// Another mutant cap and parallelism, for tests and benchmarks.
    pub fn with_limits(self, limits: Limits) -> PythonAdapter {
        PythonAdapter { limits, ..self }
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn witness(&self) -> WitnessPolicy {
        self.witness
    }

    /// The root, repo-relative; empty for the repository root.
    pub fn root(&self) -> &Path {
        if self.root == Path::new(".") { Path::new("") } else { &self.root }
    }

    /// The root as a directory on disk.
    pub fn root_in(&self, repo: &Path) -> PathBuf {
        repo.join(self.root())
    }

    /// A root-relative path made repo-relative.
    pub fn repo_relative(&self, under_root: &Path) -> PathBuf {
        self.root().join(under_root)
    }
}

/// The parts of a Python check reference, each past its grammar: the test
/// file relative to the adapter root, and the module-level function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRef {
    pub file: Checked,
    pub function: Checked,
}

impl CheckRef {
    /// pytest's node id for the test.
    pub fn node_id(&self) -> Checked {
        Checked::node_id(&self.file, &self.function)
    }
}

/// `<file>::<function>`.
pub fn check_ref(file: &Path, function: &str) -> String {
    format!("{}::{function}", file.display())
}

pub fn parse_check_ref(text: &str) -> Result<CheckRef, AdapterError> {
    let bad = |message: &str| AdapterError::Binding { check_ref: text.to_string(), message: message.to_string() };
    let (file, function) = text.rsplit_once("::").ok_or_else(|| bad("expected `<file>::<function>`"))?;
    let file = Checked::path(Path::new(file)).map_err(|e| bad(&format!("file: {e}")))?;
    let function = Checked::identifier(function).map_err(|e| bad(&format!("function: {e}")))?;
    Ok(CheckRef { file, function })
}

/// The last `max` bytes of a tool's output, on a character boundary, marked
/// when cut: the end is where a runner says what went wrong.
pub fn last(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max.saturating_sub(1);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}

impl Adapter for PythonAdapter {
    fn language(&self) -> Language {
        Language::Python
    }

    fn discover(&self, repo: &Path) -> Result<Vec<Binding>, AdapterError> {
        discover::discover(self, repo)
    }

    fn check(&self, repo: &Path, binding: &Binding, budget: Budget, seed: Seed) -> Result<Check, AdapterError> {
        check::run(self, repo, binding, budget, seed)
    }

    fn score(&self, repo: &Path, binding: &Binding, scope_files: &[PathBuf]) -> Result<Score, AdapterError> {
        score::run(self, repo, binding, scope_files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_refs_round_trip_through_their_grammars() {
        let text = check_ref(Path::new("tests/test_props.py"), "test_merge");
        assert_eq!(text, "tests/test_props.py::test_merge");
        let r = parse_check_ref(&text).expect("parses");
        assert_eq!(r.node_id().as_str(), "tests/test_props.py::test_merge");
        assert!(parse_check_ref("tests/test_props.py").is_err(), "no function");
        assert!(parse_check_ref("-x.py::test_a").is_err(), "a leading dash never reaches pytest");
        assert!(parse_check_ref("tests/test_[a].py::test_a").is_err(), "no glob character");
        assert!(parse_check_ref("tests/test_a.py::Class::test_a").is_err(), "no class node ids");
        assert!(parse_check_ref("tests/test_a.py::test a").is_err());
    }

    #[test]
    fn last_keeps_the_end() {
        assert_eq!(last("abcdef", 10), "abcdef");
        assert_eq!(last("abcdef", 4), "…def");
    }
}
