//! The Kani adapter: a Kani harness in an ordinary Rust crate as a property's
//! proof, verified within the harness's bounds.
//!
//! # Binding convention
//!
//! A harness is a `fn` marked `#[kani::proof]`, bare or under
//! `#[cfg_attr(kani, …)]`, in a library or binary crate (usually inside a
//! `#[cfg(kani)]` module). It is bound by a comment-only line holding exactly
//! one token, `// proob:<ULID>`, directly above it, with only blank lines,
//! comments, and attributes between; a harness that also carries `#[test]` is
//! still Kani's (`rust::scan`). Contract harnesses
//! (`#[kani::proof_for_contract(…)]`) and harnesses in integration tests bind
//! nothing and are reported `uncovered` with why. A property bound here must
//! declare `kind = "proof"`. The check reference is `<package>
//! <module::path::harness>`, the harness's fully qualified name inside its
//! crate, the one `--harness` with `--exact` takes.
//!
//! # Binding set
//!
//! The harness's file and the files reached from it through `mod`
//! declarations under `#[cfg(kani)]`, the package's `Cargo.toml`, the
//! workspace root `Cargo.toml` when the adapter root is a workspace, and
//! `Cargo.lock` at the adapter root when there is one. Unlike Verus, not the
//! whole crate: a property would otherwise leave `Holds` on every merge to
//! its crate. The code under proof is the property's scope, which discovery
//! does not see (it takes no property); the scope's own hash covers it, as for
//! the Rust adapter. The Kani release is not a file of the repository: every
//! result records `kani`, `cbmc`, and `nightly` in `tool_versions`.
//!
//! # Running
//!
//! `check` resolves the pinned toolchain (`tools`) and runs `cargo kani -p
//! <package> --tests --harness <path> --exact --sarif <file>` in its own
//! build directory under a wall-clock limit, never with `--quiet` (a failing
//! harness would exit 0) or `--no-unwinding-checks`. `--tests` is there
//! because a harness that is also a `#[test]` is compiled only in test mode.
//! The output is classified by `proob_facts::parsers::kani`, failing closed:
//! only a `SUCCESSFUL` run with nothing undetermined, every cover satisfied,
//! and at least one check proved is `Verified`; a failed assertion or Kani
//! safety check is `ProofFailed`; anything else is `Errored` naming why.
//!
//! # Scoring
//!
//! `score` lists the scope's mutants with cargo-mutants, drops those in
//! harness code, and re-runs the harness on each sampled mutant in a
//! temporary copy; only a failed assertion of the harness's own is a kill
//! (`score`).
//!
//! # The build probe
//!
//! `probe` asks once whether Kani can build the crate at all, so a crate it
//! cannot build is never declared full of proofs.
//!
//! # Cargo features
//!
//! As for the Rust adapter (`rust` module doc, "Cargo features"): the config
//! row's `features`, `all_features`, and `no_default_features`
//! (`rust::Features`) go on every cargo invocation the adapter makes,
//! identically: `cargo kani` for the check, the probe, the score's baseline
//! and each mutant, and `cargo mutants --list`. A crate whose `build.rs`
//! refuses to build without a feature cannot be probed, checked, or scored
//! without them. They are a config amendment, never a binding-set member.
pub mod check;
pub mod discover;
pub mod probe;
pub mod score;
pub mod tools;

use crate::argv::Checked;
use crate::rust::Features;
use crate::{
    Adapter, AdapterError, Binding, Budget, Check, Language, Readiness, RegistryError, Score, Seed, Uncovered,
};
use proob_facts::config::Adapter as AdapterConfig;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const RUNNER: &str = "kani";
pub const MUTATION_TOOL: &str = "cargo-mutants";
/// Mutants run per score unless config says otherwise.
pub const DEFAULT_MAX_MUTANTS: u32 = 30;

#[derive(Debug, Clone)]
pub struct KaniAdapter {
    /// The adapter root, relative to the repository (`.` for the root itself).
    root: PathBuf,
    /// Variables set on every tool process; `PATH`, `KANI_HOME`, and `HOME`
    /// here also steer where the toolchain is looked for. In production,
    /// proob's own tool directory and Kani home when they exist (else empty).
    tool_env: Vec<(OsString, OsString)>,
    max_mutants: u32,
    features: Features,
}

impl KaniAdapter {
    pub fn at(root: impl Into<PathBuf>) -> KaniAdapter {
        KaniAdapter {
            root: root.into(),
            tool_env: crate::rust::tools::own_tools_env(),
            max_mutants: DEFAULT_MAX_MUTANTS,
            features: Features::default(),
        }
    }

    /// A tool environment override, for sandboxes and tests.
    pub fn with_tool_env(self, tool_env: Vec<(OsString, OsString)>) -> KaniAdapter {
        KaniAdapter { tool_env, ..self }
    }

    /// From a config entry whose root already passed the path grammar.
    pub fn configured(a: &AdapterConfig, root: Checked) -> Result<KaniAdapter, RegistryError> {
        if a.runner != RUNNER {
            return Err(RegistryError::UnsupportedRunner {
                language: Language::Kani,
                runner: a.runner.clone(),
                expected: RUNNER,
            });
        }
        if a.mutation_tool != MUTATION_TOOL {
            return Err(RegistryError::UnsupportedMutationTool {
                language: Language::Kani,
                tool: a.mutation_tool.clone(),
                expected: MUTATION_TOOL,
            });
        }
        if a.max_mutants == Some(0) {
            return Err(RegistryError::UnsupportedOption {
                language: Language::Kani,
                option: "max_mutants",
                message: "must be at least 1: a score that runs no mutant measures nothing",
            });
        }
        if a.jobs.is_some() {
            return Err(RegistryError::UnsupportedOption {
                language: Language::Kani,
                option: "jobs",
                message: "is not taken: each mutant is a full `cargo kani` run, one at a time",
            });
        }
        Ok(KaniAdapter {
            root: PathBuf::from(root.as_str()),
            tool_env: crate::rust::tools::own_tools_env(),
            max_mutants: a.max_mutants.unwrap_or(DEFAULT_MAX_MUTANTS),
            features: Features::configured(a)?,
        })
    }

    /// Another cargo feature selection, for tests and benchmarks.
    pub fn with_features(self, features: Features) -> KaniAdapter {
        KaniAdapter { features, ..self }
    }

    pub fn features(&self) -> &Features {
        &self.features
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

/// The parts of a Kani check reference, each past its grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRef {
    pub package: Checked,
    /// The harness's fully qualified name inside its crate.
    pub harness: Checked,
}

/// `<package> <module::path::harness>`.
pub fn check_ref(package: &str, path: &[String]) -> String {
    format!("{package} {}", path.join("::"))
}

pub fn parse_check_ref(text: &str) -> Result<CheckRef, AdapterError> {
    let bad = |message: &str| AdapterError::Binding { check_ref: text.to_string(), message: message.to_string() };
    let (package, path) = text.split_once(' ').ok_or_else(|| bad("expected `<package> <module::path::harness>`"))?;
    let package = Checked::package(package).map_err(|e| bad(&format!("package: {e}")))?;
    let harness = Checked::rust_path(path).map_err(|e| bad(&format!("harness: {e}")))?;
    Ok(CheckRef { package, harness })
}

impl Adapter for KaniAdapter {
    fn language(&self) -> Language {
        Language::Kani
    }

    fn readiness(&self, repo: &Path) -> Readiness {
        // A score also lists mutants with cargo-mutants.
        let tc = match tools::resolve(self.tool_env()) {
            tools::Resolution::Ready(tc) => tc,
            tools::Resolution::Missing { reason, versions } => return Readiness::Missing { reason, versions },
        };
        match crate::rust::tools::resolve(&self.root_in(repo), &[crate::rust::tools::Need::Mutants], self.tool_env()) {
            crate::rust::tools::Resolution::Ready(t) => Readiness::Ready([tc.versions, t.versions].concat()),
            crate::rust::tools::Resolution::Missing { reason, .. } => {
                Readiness::Missing { reason, versions: tc.versions }
            }
        }
    }

    fn discover(&self, repo: &Path) -> Result<Vec<Binding>, AdapterError> {
        discover::discover(self, repo)
    }

    fn check(&self, repo: &Path, binding: &Binding, _budget: Budget, _seed: Seed) -> Result<Check, AdapterError> {
        check::run(self, repo, binding)
    }

    fn score(&self, repo: &Path, binding: &Binding, scope_files: &[PathBuf]) -> Result<Score, AdapterError> {
        score::run(self, repo, binding, scope_files)
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
        let text = check_ref("kani-crate", &["verification".to_string(), "clamp_ok".to_string()]);
        assert_eq!(text, "kani-crate verification::clamp_ok");
        let r = parse_check_ref(&text).expect("parses");
        assert_eq!((r.package.as_str(), r.harness.as_str()), ("kani-crate", "verification::clamp_ok"));
        assert_eq!(parse_check_ref("p at_root").expect("parses").harness.as_str(), "at_root");
        assert!(parse_check_ref("p verification::*").is_err(), "a wildcard never reaches --harness");
        assert!(parse_check_ref("--all h").is_err());
        assert!(parse_check_ref("p a b").is_err());
        assert!(parse_check_ref("p").is_err());
    }
}
