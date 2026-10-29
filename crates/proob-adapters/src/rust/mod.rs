//! The Rust adapter: proptest tests through cargo-nextest for `check`,
//! cargo-mutants for `score`, a source scan for `discover`.
//!
//! # Binding convention
//!
//! A test is bound by a line comment holding exactly one token,
//! `// proob:<ULID>`, directly above the test's attributes:
//!
//! ```text
//! proptest! {
//!     // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHA
//!     #[test]
//!     fn sorted_preserves_length(v in small_vec()) { … }
//! }
//! ```
//!
//! Only blank lines, other comments, and attributes may stand between the
//! token and the `fn`; the attributes must include `#[test]` (or
//! `#[…::test]`, `#[proptest]`, `#[…::proptest]`). What follows `proob:` must
//! be one ULID and nothing else, so `proob:<ULID>:v2` and a truncated id are
//! errors that `validate` reports, and a token above anything but a test fn
//! is an error too. Each property binds at most one test. The check reference
//! is nextest's own name for the test, `<binary id> <test path>`, such as
//! `rust-repo::props sorted_preserves_length`.
//!
//! # Binding set
//!
//! For a test in an integration-test crate (`tests/x.rs`, `tests/x/main.rs`):
//! every file of that crate, the root and the files its `mod` declarations
//! reach (`x.rs`, `x/mod.rs`, `#[path = "…"]`), transitively. For a test in a
//! `src/` file: that file plus the files reached from it through `mod`
//! declarations under `#[cfg(test)]`. Plus `.config/nextest.toml` and
//! `.cargo/mutants.toml` at the adapter root when they exist. Repo-relative,
//! sorted, deduplicated, hashed as `proob-binding-v1` over each member's
//! normalised content. `Cargo.toml` and `Cargo.lock` are deliberately not
//! members: a dependency bump would otherwise re-open every binding's
//! approval. `include!` and `[lib] path` outside `src/` are not followed.
//! A crate whose manifest says `[package.metadata.verus] verify = true` is
//! the Verus adapter's: its lib and bin roots hold proofs, not tests, and are
//! not scanned here; its integration tests are plain Rust and still are.
//!
//! # Running
//!
//! `check` runs `cargo nextest run` for the one bound test with the adapter's
//! `PROPTEST_RNG_SEED` and `PROPTEST_CASES`, failure persistence disabled,
//! and a tool-specific nextest profile that writes a JUnit report to a
//! temporary path; the report is the only thing read. `score` runs
//! `cargo mutants` over the scope files with nextest as the test tool and the
//! same one-test filter forwarded, so no other test can kill a mutant; the
//! counts come from `outcomes.json`. Both record `cargo`, `cargo-nextest`,
//! `cargo-mutants` (score), and `proptest` (from `Cargo.lock`) versions.
//!
//! # Cargo features
//!
//! The config row's `features`, `all_features`, and `no_default_features`
//! (`Features`) go on every cargo invocation the adapter makes, identically:
//! the check's `cargo nextest run`, `cargo mutants --list`, and the
//! `cargo mutants` run (which passes them to its baseline and every mutant's
//! build and test). Each feature name is its own argv element after its own
//! `--features`. They live in `proob/config.toml`, which is governed as a
//! whole and is not a binding-set member: changing them is a config
//! amendment, so the headline reads `Re-approve (config amended)` until the
//! amended config is approved, and no binding hash moves.
pub mod check;
pub mod discover;
pub mod manifest;
pub mod proptest;
pub mod scan;
pub mod score;
pub mod tools;

use crate::argv::{Checked, Command};
use crate::mutation::{self, Limits};
use crate::{Adapter, AdapterError, Binding, Budget, Check, Language, RegistryError, Score, Seed, WitnessPolicy};
use proob_facts::config::Adapter as AdapterConfig;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const RUNNER: &str = "nextest";
pub const MUTATION_TOOL: &str = "cargo-mutants";

#[derive(Debug, Clone)]
pub struct RustAdapter {
    /// The adapter root, relative to the repository (`.` for the root itself).
    root: PathBuf,
    witness: WitnessPolicy,
    /// Variables set on every tool process; `PATH` here also decides where
    /// `cargo` is looked for. Empty in production: the process environment.
    tool_env: Vec<(OsString, OsString)>,
    /// The score's mutant cap and parallelism (`mutation`).
    limits: Limits,
    features: Features,
}

/// The cargo feature selection every cargo invocation of the adapter carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Features {
    pub names: Vec<Checked>,
    pub all: bool,
    pub no_default: bool,
}

impl Features {
    /// From a config row; a name outside `Checked::feature` is refused.
    pub fn configured(a: &AdapterConfig) -> Result<Features, RegistryError> {
        let names = a.features.iter().map(|f| Checked::feature(f)).collect::<Result<Vec<_>, _>>().map_err(|_| {
            RegistryError::UnsupportedOption {
                language: Language::Rust,
                option: "features",
                message: "must each be a cargo feature name (`name` or `package/name`)",
            }
        })?;
        Ok(Features { names, all: a.all_features.unwrap_or(false), no_default: a.no_default_features.unwrap_or(false) })
    }

    /// `command` with the flags appended, each name after its own `--features`. Pure.
    pub fn apply(&self, command: Command) -> Command {
        let command = if self.all { command.arg("--all-features") } else { command };
        let command = if self.no_default { command.arg("--no-default-features") } else { command };
        self.names.iter().fold(command, |c, name| c.opt("--features", name))
    }
}

impl RustAdapter {
    /// An adapter at `root` (repo-relative) with full witnesses.
    pub fn at(root: impl Into<PathBuf>) -> RustAdapter {
        RustAdapter {
            root: root.into(),
            witness: WitnessPolicy::Full,
            tool_env: Vec::new(),
            limits: Limits::DEFAULT,
            features: Features::default(),
        }
    }

    pub fn with_witness(self, witness: WitnessPolicy) -> RustAdapter {
        RustAdapter { witness, ..self }
    }

    /// A tool environment override, for sandboxes and tests (a `PATH` without
    /// a runner, a `CARGO_HOME` without its binaries).
    pub fn with_tool_env(self, tool_env: Vec<(OsString, OsString)>) -> RustAdapter {
        RustAdapter { tool_env, ..self }
    }

    pub fn tool_env(&self) -> &[(OsString, OsString)] {
        &self.tool_env
    }

    /// From a config entry whose root already passed the path grammar.
    pub fn configured(a: &AdapterConfig, root: Checked, witness: WitnessPolicy) -> Result<RustAdapter, RegistryError> {
        if a.runner != RUNNER {
            return Err(RegistryError::UnsupportedRunner {
                language: Language::Rust,
                runner: a.runner.clone(),
                expected: RUNNER,
            });
        }
        if a.mutation_tool != MUTATION_TOOL {
            return Err(RegistryError::UnsupportedMutationTool {
                language: Language::Rust,
                tool: a.mutation_tool.clone(),
                expected: MUTATION_TOOL,
            });
        }
        let limits = mutation::limits(a, Language::Rust)?;
        let features = Features::configured(a)?;
        Ok(RustAdapter { root: PathBuf::from(root.as_str()), witness, tool_env: Vec::new(), limits, features })
    }

    /// Another cargo feature selection, for tests and benchmarks.
    pub fn with_features(self, features: Features) -> RustAdapter {
        RustAdapter { features, ..self }
    }

    pub fn features(&self) -> &Features {
        &self.features
    }

    /// Another mutant cap and parallelism, for tests and benchmarks.
    pub fn with_limits(self, limits: Limits) -> RustAdapter {
        RustAdapter { limits, ..self }
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

/// The parts of a Rust check reference, each past its grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRef {
    pub package: Checked,
    pub binary_id: Checked,
    pub test_path: Checked,
}

/// `<binary id> <test path>`.
pub fn check_ref(binary_id: &str, test_path: &str) -> String {
    format!("{binary_id} {test_path}")
}

pub fn parse_check_ref(text: &str) -> Result<CheckRef, AdapterError> {
    let bad = |message: &str| AdapterError::Binding { check_ref: text.to_string(), message: message.to_string() };
    let (binary, test) = text.split_once(' ').ok_or_else(|| bad("expected `<binary id> <test path>`"))?;
    let binary_id = Checked::binary_id(binary).map_err(|e| bad(&format!("binary id: {e}")))?;
    let test_path = Checked::rust_path(test).map_err(|e| bad(&format!("test path: {e}")))?;
    let package =
        Checked::package(binary.split("::").next().unwrap_or(binary)).map_err(|e| bad(&format!("package: {e}")))?;
    Ok(CheckRef { package, binary_id, test_path })
}

impl Adapter for RustAdapter {
    fn language(&self) -> Language {
        Language::Rust
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
