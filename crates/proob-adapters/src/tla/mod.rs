//! The TLA+ adapter: an invariant or temporal property of a TLA+ spec as a
//! property's model, checked by TLC over the spec's own constants.
//!
//! # What a model claims
//!
//! A model check is about the design the spec states, not the code: a
//! `NoViolation` says TLC explored every state reachable under the approved
//! cfg's constants and the bound property held in each. Whether the code
//! still follows the spec is not checked; the core flags an approved model
//! for re-approval when any commit touches its scope (spec drift).
//!
//! # Binding convention
//!
//! A comment-only line holding exactly one token, `\* proob:<ULID>`, directly
//! above the definition of the invariant or property it checks (`Name ==
//! …`, no parameters), with only blank lines and `\*` comments between, in a
//! `.tla` module under the adapter root whose `.cfg` (the same base name)
//! sits beside it. A property bound here must declare `kind = "model"`. The
//! check reference is `<module path> <Name>`, the module root-relative.
//!
//! # Binding set
//!
//! The module, its `.cfg`, and every module it `EXTENDS` or `INSTANCE`s,
//! transitively, that is a file beside it: where TLC looks for it. A
//! standard module (`Naturals`, `Sequences`, `TLC`) is in the jar, not the
//! repository, and is left out; the jar's release is recorded instead.
//!
//! # Running
//!
//! `check` copies the binding set's modules into a temporary directory and
//! runs TLC there (`tools`): `-workers 1`, `-fp 0`, `-seed 0`, a fresh
//! `-metadir` per run, a cfg that checks only the bound name (`cfg`), and a
//! wall-clock limit under proob's memory budget, so nothing is written into
//! the repository. The output is classified by `proob_facts::parsers::tlc`,
//! failing closed (`check`).
//!
//! # Scoring
//!
//! `score` mutates the spec, not the code (`mutate`, `score`).
pub mod cfg;
pub mod check;
pub mod discover;
pub mod mutate;
pub mod scan;
pub mod score;
pub mod tools;

use crate::argv::Checked;
use crate::{Adapter, AdapterError, Binding, Budget, Check, Language, Readiness, RegistryError, Score, Seed};
use proob_facts::config::Adapter as AdapterConfig;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const RUNNER: &str = "tlc";
pub const MUTATION_TOOL: &str = "spec-mutants";
/// Mutants run per score unless config says otherwise.
pub const DEFAULT_MAX_MUTANTS: u32 = 30;

#[derive(Debug, Clone)]
pub struct TlaAdapter {
    /// The adapter root, relative to the repository (`.` for the root itself).
    root: PathBuf,
    /// Variables set on every tool process; `PATH` and `PROOB_TLA2TOOLS`
    /// here also steer where the tools are looked for. Empty in production.
    tool_env: Vec<(OsString, OsString)>,
    max_mutants: u32,
}

impl TlaAdapter {
    pub fn at(root: impl Into<PathBuf>) -> TlaAdapter {
        TlaAdapter { root: root.into(), tool_env: Vec::new(), max_mutants: DEFAULT_MAX_MUTANTS }
    }

    /// A tool environment override, for sandboxes and tests.
    pub fn with_tool_env(self, tool_env: Vec<(OsString, OsString)>) -> TlaAdapter {
        TlaAdapter { tool_env, ..self }
    }

    /// From a config entry whose root already passed the path grammar.
    pub fn configured(a: &AdapterConfig, root: Checked) -> Result<TlaAdapter, RegistryError> {
        let refuse = |option, message| RegistryError::UnsupportedOption { language: Language::Tla, option, message };
        if a.runner != RUNNER {
            return Err(RegistryError::UnsupportedRunner {
                language: Language::Tla,
                runner: a.runner.clone(),
                expected: RUNNER,
            });
        }
        if a.mutation_tool != MUTATION_TOOL {
            return Err(RegistryError::UnsupportedMutationTool {
                language: Language::Tla,
                tool: a.mutation_tool.clone(),
                expected: MUTATION_TOOL,
            });
        }
        if a.max_mutants == Some(0) {
            return Err(refuse("max_mutants", "must be at least 1: a score that runs no mutant measures nothing"));
        }
        if a.jobs.is_some() {
            return Err(refuse("jobs", "is not taken: TLC runs one mutant at a time, with one worker"));
        }
        Ok(TlaAdapter {
            root: PathBuf::from(root.as_str()),
            tool_env: Vec::new(),
            max_mutants: a.max_mutants.unwrap_or(DEFAULT_MAX_MUTANTS),
        })
    }

    pub fn tool_env(&self) -> &[(OsString, OsString)] {
        &self.tool_env
    }

    pub fn max_mutants(&self) -> u32 {
        self.max_mutants
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

/// The parts of a TLA+ check reference, each past its grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRef {
    /// The module, root-relative.
    pub module: Checked,
    /// The bound invariant or property.
    pub name: Checked,
}

/// `<module path> <Name>`.
pub fn check_ref(module: &Path, name: &str) -> String {
    format!("{} {name}", module.display())
}

pub fn parse_check_ref(text: &str) -> Result<CheckRef, AdapterError> {
    let bad = |message: &str| AdapterError::Binding { check_ref: text.to_string(), message: message.to_string() };
    let (module, name) = text.split_once(' ').ok_or_else(|| bad("expected `<module path> <Name>`"))?;
    let module = Checked::path(Path::new(module)).map_err(|e| bad(&format!("module: {e}")))?;
    if !module.as_str().ends_with(".tla") {
        return Err(bad("module: not a `.tla` file"));
    }
    let name = Checked::identifier(name).map_err(|e| bad(&format!("name: {e}")))?;
    Ok(CheckRef { module, name })
}

impl Adapter for TlaAdapter {
    fn language(&self) -> Language {
        Language::Tla
    }

    fn readiness(&self, _repo: &Path) -> Readiness {
        match tools::resolve(self.tool_env()) {
            tools::Resolution::Ready(t) => Readiness::Ready(t.versions),
            tools::Resolution::Missing { reason, versions } => Readiness::Missing { reason, versions },
        }
    }

    fn discover(&self, repo: &Path) -> Result<Vec<Binding>, AdapterError> {
        discover::discover(self, repo)
    }

    fn check(&self, repo: &Path, binding: &Binding, _budget: Budget, _seed: Seed) -> Result<Check, AdapterError> {
        check::run(self, repo, binding)
    }

    /// The scope files are the modelled code, which a model never runs: the
    /// spec is what is mutated.
    fn score(&self, repo: &Path, binding: &Binding, _scope_files: &[PathBuf]) -> Result<Score, AdapterError> {
        score::run(self, repo, binding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_refs_round_trip_through_their_grammars() {
        let text = check_ref(Path::new("specs/Mutex.tla"), "MutualExclusion");
        let r = parse_check_ref(&text).expect("parses");
        assert_eq!((r.module.as_str(), r.name.as_str()), ("specs/Mutex.tla", "MutualExclusion"));
        assert!(parse_check_ref("../Mutex.tla Inv").is_err());
        assert!(parse_check_ref("Mutex.cfg Inv").is_err());
        assert!(parse_check_ref("Mutex.tla a::b").is_err());
        assert!(parse_check_ref("Mutex.tla").is_err());
    }
}
