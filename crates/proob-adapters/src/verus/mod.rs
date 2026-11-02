//! The Verus adapter: a proof as a property's primary evidence, and
//! mutation-under-proof as the measure of the specification's strength.
//!
//! # Binding convention
//!
//! A verified crate is one whose manifest says `[package.metadata.verus]
//! verify = true`. Inside it, an item is bound by a comment-only line holding
//! exactly one token, `// proob:<ULID>`, directly above the item, inside a
//! `verus!` block:
//!
//! ```text
//! verus! {
//!     /// `x` brought into `[lo, hi]`.
//!     // proob:01K8V5NQ4A0F3R7Y2M9D6P1XVA
//!     pub fn clamp(x: i32, lo: i32, hi: i32) -> (r: i32)
//!         requires lo <= hi,
//!         ensures lo <= r <= hi,
//!     { … }
//! }
//! ```
//!
//! Only blank lines, other comments, and attributes may stand between the
//! token and the item; the item is a `fn` (exec, the usual case: its
//! `ensures` is the statement the proof discharges), a `proof fn`, or a
//! `spec fn`. A token above anything else, above a `fn` outside every
//! `verus!` block, or in a crate without `verify = true` binds nothing: the
//! first two are errors `validate` reports, the third renders `Not checked
//! (no adapter)` (`uncovered`). What follows `proob:` must be one ULID. Each
//! property binds at most one item; a property bound here must declare
//! `kind = "proof"`, since its primary evidence is `Verified`, not
//! `NoCounterexample`. The check reference is `<package> <path>`, where
//! `<path>` is the item's module path inside the crate and its name
//! (`clamp clamp`, `proob-core scope_hash::combine_sorted`): the two things
//! `--verify-only-module` (or `--verify-root`) and `--verify-function` take.
//!
//! # Binding set
//!
//! Every source file the verified crate compiles (its lib and bin roots and
//! the files their `mod` declarations reach), the crate's `Cargo.toml`, the
//! workspace root `Cargo.toml` when the crate is a workspace member (it
//! usually pins `vstd`), `rust-toolchain.toml` at the adapter root (the
//! channel the Verus release requires), and the same files of every path
//! dependency that is itself a verified crate (a trusted base such as
//! `proob-trusted`, whose `external_body` specifications any proof in the
//! crate may rest on). Any spec in a verified crate can weaken any proof in
//! it, so the crate is the unit of approval, as the plan says; `Cargo.lock`
//! is not a member. The Verus binary, its commit, and the Z3 version are not
//! files of the repository and are not in the hash: every result records
//! them in `tool_versions`, so evidence says which verifier produced it and a
//! toolchain bump shows up in evidence rather than re-opening every
//! binding's approval (A15 pins the three together with
//! `rust-toolchain.toml` and the `vstd` version, both members).
//!
//! # Running
//!
//! `check` locates the toolchain (`tools`), runs `cargo verus verify` on the
//! whole crate under `--no-cheating` with the fixed `--rlimit`, and, when the
//! crate verifies, attributes the bound item with `cargo verus focus` and
//! `--verify-function`, so the outcome names the item. Both read Verus's
//! `--output-json`; a failure's diagnostics come from stderr. `score` is
//! mutation-under-proof (`mutate`, `score`): the operator set
//! `mutants-under-proof-v2` (operator swaps, and the structural arm swap,
//! branch swap, return drop, and result negation that reach dispatch
//! bodies) applied to the bound item's executable body in a temporary copy
//! of the tree, each mutant verified the same way; a mutant the verifier
//! rejects is killed, one it accepts survived, one rustc or Verus cannot
//! even check is unviable and excluded, and one that exhausts the resource
//! limit timed out. At most `max_mutants` (config; default 25) are verified
//! per run, by operator kind then position, and the rest are reported as
//! unsampled. This runs on schedule only.
pub mod check;
pub mod discover;
pub mod mutate;
pub mod scan;
pub mod score;
pub mod tools;

use crate::argv::Checked;
use crate::{
    Adapter, AdapterError, Binding, Budget, Check, Language, Readiness, RegistryError, Score, Seed, Uncovered,
};
use proob_facts::config::Adapter as AdapterConfig;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const RUNNER: &str = "cargo-verus";
pub const MUTATION_TOOL: &str = "mutants-under-proof";
/// Mutants verified per score run unless config says otherwise.
pub const DEFAULT_MAX_MUTANTS: u32 = 25;
/// The SMT resource limit every run passes: the CI verify job's.
pub const RLIMIT: u32 = 10;

#[derive(Debug, Clone)]
pub struct VerusAdapter {
    /// The adapter root, relative to the repository (`.` for the root itself).
    root: PathBuf,
    /// Variables set on every tool process; `PATH`, `PROOB_VERUS_DIR`, and
    /// `HOME` here also steer where the toolchain is looked for. Empty in
    /// production: the process environment.
    tool_env: Vec<(OsString, OsString)>,
    max_mutants: u32,
}

impl VerusAdapter {
    pub fn at(root: impl Into<PathBuf>) -> VerusAdapter {
        VerusAdapter { root: root.into(), tool_env: Vec::new(), max_mutants: DEFAULT_MAX_MUTANTS }
    }

    /// A tool environment override, for sandboxes and tests.
    pub fn with_tool_env(self, tool_env: Vec<(OsString, OsString)>) -> VerusAdapter {
        VerusAdapter { tool_env, ..self }
    }

    pub fn with_max_mutants(self, max_mutants: u32) -> VerusAdapter {
        VerusAdapter { max_mutants, ..self }
    }

    /// From a config entry whose root already passed the path grammar.
    pub fn configured(a: &AdapterConfig, root: Checked) -> Result<VerusAdapter, RegistryError> {
        if a.runner != RUNNER {
            return Err(RegistryError::UnsupportedRunner {
                language: Language::Verus,
                runner: a.runner.clone(),
                expected: RUNNER,
            });
        }
        if a.mutation_tool != MUTATION_TOOL {
            return Err(RegistryError::UnsupportedMutationTool {
                language: Language::Verus,
                tool: a.mutation_tool.clone(),
                expected: MUTATION_TOOL,
            });
        }
        if a.max_mutants == Some(0) {
            return Err(RegistryError::UnsupportedOption {
                language: Language::Verus,
                option: "max_mutants",
                message: "must be at least 1: a score that verifies no mutant measures nothing",
            });
        }
        if a.jobs.is_some() {
            return Err(RegistryError::UnsupportedOption {
                language: Language::Verus,
                option: "jobs",
                message: "is not taken: mutation-under-proof verifies one mutant at a time, each a full `cargo verus focus` run",
            });
        }
        Ok(VerusAdapter {
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

    pub fn rlimit(&self) -> u32 {
        RLIMIT
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

/// The parts of a Verus check reference, each past its grammar: the package,
/// the item's module path inside the crate (`None` at the crate root), and
/// the item's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRef {
    pub package: Checked,
    pub module: Option<Checked>,
    pub function: Checked,
}

/// `<package> <module::path::function>`.
pub fn check_ref(package: &str, module_path: &[String], function: &str) -> String {
    let path: Vec<&str> = module_path.iter().map(String::as_str).chain([function]).collect();
    format!("{package} {}", path.join("::"))
}

pub fn parse_check_ref(text: &str) -> Result<CheckRef, AdapterError> {
    let bad = |message: &str| AdapterError::Binding { check_ref: text.to_string(), message: message.to_string() };
    let (package, path) = text.split_once(' ').ok_or_else(|| bad("expected `<package> <module::path::function>`"))?;
    let package = Checked::package(package).map_err(|e| bad(&format!("package: {e}")))?;
    let (module, function) = path.rsplit_once("::").map_or((None, path), |(m, f)| (Some(m), f));
    let module = module.map(Checked::rust_path).transpose().map_err(|e| bad(&format!("module path: {e}")))?;
    let function = Checked::identifier(function).map_err(|e| bad(&format!("function: {e}")))?;
    Ok(CheckRef { package, module, function })
}

impl Adapter for VerusAdapter {
    fn language(&self) -> Language {
        Language::Verus
    }

    fn readiness(&self, _repo: &Path) -> Readiness {
        match tools::resolve(self.tool_env()) {
            tools::Resolution::Ready(tc) => Readiness::Ready(tc.versions),
            tools::Resolution::Missing { reason, versions } => Readiness::Missing { reason, versions },
        }
    }

    fn discover(&self, repo: &Path) -> Result<Vec<Binding>, AdapterError> {
        discover::discover(self, repo)
    }

    fn check(&self, repo: &Path, binding: &Binding, _budget: Budget, _seed: Seed) -> Result<Check, AdapterError> {
        check::run(self, repo, binding)
    }

    fn score(&self, repo: &Path, binding: &Binding, _scope_files: &[PathBuf]) -> Result<Score, AdapterError> {
        score::run(self, repo, binding)
    }

    fn uncovered(&self, repo: &Path) -> Result<Vec<Uncovered>, AdapterError> {
        discover::uncovered(self, repo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_refs_round_trip_through_their_grammars() {
        let text = check_ref("proob-core", &["scope_hash".to_string()], "combine_sorted");
        assert_eq!(text, "proob-core scope_hash::combine_sorted");
        let r = parse_check_ref(&text).expect("parses");
        assert_eq!(r.module.as_ref().map(Checked::as_str), Some("scope_hash"));
        assert_eq!(r.function.as_str(), "combine_sorted");
        let root = parse_check_ref("clamp clamp").expect("parses");
        assert!(root.module.is_none());
        assert!(parse_check_ref("clamp clamp*").is_err(), "a wildcard never reaches --verify-function");
        assert!(parse_check_ref("--all clamp").is_err());
        assert!(parse_check_ref("clamp a::b c").is_err());
    }
}
