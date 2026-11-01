//! proob's adapters: the kind-indexed contract between the CLI and a
//! language's own tools, and its implementations.
//!
//! An adapter does three things and nothing else. `discover` finds the checks
//! that carry a `proob:<ULID>` binding token and computes each check's binding
//! set, the files whose content gives the check its meaning. `check` runs the
//! kind's primary check for one binding with a seed and a budget the adapter
//! chose. `score` runs the kind's strength measure with only the bound check,
//! against mutants inside the property's scope files. Every result is a value
//! of a closed type: an adapter maps a runner's output to one of them and
//! records the tool versions, and never interprets what the value means for
//! a verdict. That interpretation is the verified core's, through the facts
//! the CLI builds from these values.
//!
//! Adapters never build a shell string. A child process is started only
//! through `argv::Command`, whose arguments are fixed literals the adapter
//! wrote, adapter-owned paths, or `argv::Checked` values that passed a
//! closed grammar first (`argv.rs` is the one place both are built).
//!
//! The Rust adapter (`rust`), the Verus adapter (`verus`), the Kani adapter
//! (`kani`), the Python adapter (`python`), and the TLA+ adapter (`tla`) are
//! here. A language without an adapter yet would be left out by
//! `adapters_for`, so every property in such a repository renders `Not
//! checked (no adapter)` rather than failing or faking.
pub mod argv;
pub mod kani;
pub mod mutation;
pub mod python;
pub mod rust;
pub mod tla;
pub mod verus;

use proob_core::types::Kind;
pub use proob_facts::config::Language;
use proob_facts::config::{Adapter as AdapterConfig, Config, Witness as WitnessName};
use proob_facts::ulid::UlidError;
use std::fmt;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// The number of generated cases a property-based test runs: the adapter's
/// budget, never the test's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget(pub u32);

impl Budget {
    /// proptest's own default, so an unconfigured run matches a plain `cargo test`.
    pub const DEFAULT: Budget = Budget(256);
}

/// The RNG seed the adapter hands the runner, and that replays the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seed(pub u64);

impl Seed {
    /// The fixed seed a score runs under: a function of the property id, so
    /// the same property scores the same way on every machine.
    pub fn for_scoring(id: u128) -> Seed {
        Seed((id as u64) ^ ((id >> 64) as u64))
    }
}

/// Where a binding token was found, repo-relative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub line: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.path.display(), self.line)
    }
}

/// A check bound to a property.
///
/// `check_ref` is the adapter's own name for the check (for Rust, nextest's
/// `<binary id> <test path>`); `files` is the binding set, repo-relative,
/// sorted, deduplicated; `binding_hash` is `proob-binding-v1` over the
/// normalised content of those files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub id: u128,
    pub kind: Kind,
    pub check_ref: String,
    pub files: Vec<PathBuf>,
    pub binding_hash: [u8; 32],
    pub token: Location,
}

/// How much of a counterexample leaves the adapter (`witness` in config).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessPolicy {
    Full,
    SeedOnly,
}

impl From<WitnessName> for WitnessPolicy {
    fn from(w: WitnessName) -> Self {
        match w {
            WitnessName::Full => WitnessPolicy::Full,
            WitnessName::SeedOnly => WitnessPolicy::SeedOnly,
        }
    }
}

/// The longest witness text an adapter records, in bytes.
pub const WITNESS_CAP: usize = 2048;

/// A counterexample as the policy lets it leave the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Witness {
    /// The shrunk failing input and the reason, capped at `WITNESS_CAP`.
    Full(String),
    /// Withheld by policy; the seed replays it.
    SeedOnly,
}

impl Witness {
    pub fn new(policy: WitnessPolicy, text: &str) -> Witness {
        match policy {
            WitnessPolicy::Full => Witness::Full(cap(text, WITNESS_CAP)),
            WitnessPolicy::SeedOnly => Witness::SeedOnly,
        }
    }

    /// The text a record carries.
    pub fn render(&self) -> String {
        match self {
            Witness::Full(text) => text.clone(),
            Witness::SeedOnly => {
                "(witness withheld by policy witness = \"seed_only\"; replay with the seed)".to_string()
            }
        }
    }
}

/// An exit status for a message: the code, or `a signal` when there is none.
pub fn exit_text(code: Option<i32>) -> String {
    code.map_or_else(|| "a signal".to_string(), |c| format!("code {c}"))
}

/// `text` cut to at most `max` bytes on a character boundary, marked when cut.
pub fn cap(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max.saturating_sub(1);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    NoCounterexample {
        budget: Budget,
        replay: Seed,
    },
    Counterexample {
        witness: Witness,
        replay: Seed,
    },
    /// A proof: the verifier accepted the bound item; the measure says what
    /// the acceptance was bounded by.
    Verified {
        measure: ProofMeasure,
    },
    /// A proof: the verifier rejected the crate; the diagnostics say where.
    ProofFailed {
        diagnostics: String,
    },
    /// A model: the model checker explored every reachable state of the
    /// spec under its constants and the bound property held in all of them.
    NoViolation,
    /// A model: the bound property fails; the trace's first and last states.
    Violation {
        trace: String,
    },
    Errored {
        reason: String,
    },
}

/// What bounds a proof, by verifier: closed, so a proof without one cannot be
/// built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofMeasure {
    /// Verus: the SMT resource limit the crate verified under.
    Verus { rlimit: u32 },
    /// Kani: the checks the harness passed, within its bounds.
    Kani { checks: u32 },
}

/// Tool name and version, as the adapter probed them.
pub type ToolVersions = Vec<(String, String)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub outcome: CheckOutcome,
    pub tool_versions: ToolVersions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreOutcome {
    /// `unsampled` mutants were listed in scope beyond the run's cap
    /// (`max_mutants`) and not run; they are in no other count, and the
    /// other counts are the sample's.
    Scored {
        killed: u32,
        survived: u32,
        timeout: u32,
        unviable: u32,
        unsampled: u32,
    },
    NoMutants,
    Errored {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Score {
    pub outcome: ScoreOutcome,
    pub tool_versions: ToolVersions,
    /// How many mutants ran at once and why, when the tool runs them in
    /// parallel; for text output only, never evidence.
    pub jobs: Option<mutation::Jobs>,
}

// ---------------------------------------------------------------------------
// Errors: the adapter could not even attempt the operation
// ---------------------------------------------------------------------------

/// Why a `// proob:…` line binds nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenProblem {
    /// What follows `proob:` is not one ULID (a version suffix, a truncation).
    NotUlid { text: String, error: UlidError },
    /// No `fn` follows the token before the next item.
    NoFunction,
    /// A `fn` follows, but no `#[test]` (or `#[…::test]`, `#[proptest]`) is between.
    NotATest { function: String },
    /// No `fn` follows the token inside a `verus!` block before the next item.
    NoVerusItem,
    /// A `fn` follows, but outside every `verus!` block, where Verus verifies nothing.
    OutsideVerus { function: String },
    /// Python: no `def` follows the token before the next statement.
    NoDef,
    /// Python: a `def` follows, but its name does not start with `test`, so pytest collects nothing.
    NotATestDef { function: String },
    /// Python: the `def` is indented (a method, or a nested function); only module-level tests are bound.
    NestedDef { function: String },
    /// Python: the function's name is not an ASCII identifier, which a node id argument cannot carry.
    NonAsciiName { function: String },
    /// Python: the token is in a file pytest does not collect (`test_*.py`, `*_test.py`).
    NotATestFile,
    /// TLA+: no definition without parameters (`Name == …`) follows the token.
    NoTlaDefinition,
    /// TLA+: the module has no `.cfg` of the same base name beside it.
    NoTlaConfig { config: String },
}

impl fmt::Display for TokenProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenProblem::NotUlid { text, error } => {
                write!(f, "binding token `proob:{text}` is not `proob:<ULID>`: {error}")
            }
            TokenProblem::NoFunction => {
                write!(f, "binding token binds nothing: write it on the line directly above a `#[test]` fn")
            }
            TokenProblem::NotATest { function } => {
                write!(f, "binding token sits above `fn {function}`, which is not a `#[test]`")
            }
            TokenProblem::NoVerusItem => write!(
                f,
                "binding token binds nothing: write it on the line directly above a `fn`, `proof fn`, or `spec fn` inside a `verus!` block"
            ),
            TokenProblem::OutsideVerus { function } => {
                write!(
                    f,
                    "binding token sits above `fn {function}`, which is outside every `verus!` block; Verus verifies nothing there"
                )
            }
            TokenProblem::NoDef => {
                write!(
                    f,
                    "binding token binds nothing: write it on the line directly above a module-level `def test_…` (above its decorators)"
                )
            }
            TokenProblem::NotATestDef { function } => {
                write!(
                    f,
                    "binding token sits above `def {function}`, which pytest does not collect (test functions start with `test`)"
                )
            }
            TokenProblem::NestedDef { function } => write!(
                f,
                "binding token sits above `def {function}`, which is indented inside a class or function; bind a module-level test function (methods are not supported)"
            ),
            TokenProblem::NonAsciiName { function } => {
                write!(
                    f,
                    "binding token sits above `def {function}`, whose name is not an ASCII identifier; a node id argument cannot carry it"
                )
            }
            TokenProblem::NotATestFile => {
                write!(
                    f,
                    "binding token is in a file pytest does not collect; test files are named `test_*.py` or `*_test.py`"
                )
            }
            TokenProblem::NoTlaDefinition => write!(
                f,
                "binding token binds nothing: write `\\* proob:<ULID>` on the line directly above the definition of the invariant or property it checks (`Name == …`, no parameters)"
            ),
            TokenProblem::NoTlaConfig { config } => {
                write!(f, "binding token is in a module without its model: TLC needs `{config}` beside it")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    Io {
        path: PathBuf,
        message: String,
    },
    Manifest {
        path: PathBuf,
        message: String,
    },
    Token {
        at: Location,
        problem: TokenProblem,
    },
    /// Two tests carry the same property's token.
    Duplicate {
        id: String,
        first: Location,
        second: Location,
    },
    /// The binding was not produced by this adapter, or names something it
    /// cannot pass to a tool safely.
    Binding {
        check_ref: String,
        message: String,
    },
    /// A value that would reach a tool failed the grammar.
    Unsafe {
        value: String,
        rule: String,
    },
    Temp {
        message: String,
    },
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdapterError::Io { path, message } => write!(f, "{}: {message}", path.display()),
            AdapterError::Manifest { path, message } => write!(f, "{}: {message}", path.display()),
            AdapterError::Token { at, problem } => write!(f, "{at}: {problem}"),
            AdapterError::Duplicate { id, first, second } => {
                write!(
                    f,
                    "{second}: binding token `proob:{id}` is already bound at {first}; a property binds one check"
                )
            }
            AdapterError::Binding { check_ref, message } => write!(f, "binding `{check_ref}`: {message}"),
            AdapterError::Unsafe { value, rule } => write!(f, "`{value}` cannot be passed to a tool: {rule}"),
            AdapterError::Temp { message } => write!(f, "cannot create a temporary directory: {message}"),
        }
    }
}

impl std::error::Error for AdapterError {}

// ---------------------------------------------------------------------------
// The contract
// ---------------------------------------------------------------------------

/// A token an adapter saw and cannot bind because of where it is: a
/// `// proob:<ULID>` above a `verus!` item in a crate that is not verified.
/// Unless another adapter binds the id, the property renders `Not checked
/// (no adapter)`, and `reason` says what would cover it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncovered {
    pub id: u128,
    pub token: Location,
    pub reason: String,
}

pub trait Adapter {
    fn language(&self) -> Language;
    /// Every bound check under the adapter's root, with its binding set.
    fn discover(&self, repo: &Path) -> Result<Vec<Binding>, AdapterError>;
    /// The kind's primary check for one binding, under the adapter's seed and budget.
    fn check(&self, repo: &Path, binding: &Binding, budget: Budget, seed: Seed) -> Result<Check, AdapterError>;
    /// The kind's strength measure: only the bound check, against mutants in `scope_files`.
    fn score(&self, repo: &Path, binding: &Binding, scope_files: &[PathBuf]) -> Result<Score, AdapterError>;
    /// Tokens under the root this adapter saw and binds nothing to, with why.
    fn uncovered(&self, _repo: &Path) -> Result<Vec<Uncovered>, AdapterError> {
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------

/// Whether a language has an adapter in this proob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// Not yet: the unit that brings it.
    Planned(&'static str),
}

pub fn availability(language: Language) -> Availability {
    match language {
        Language::Rust | Language::Verus | Language::Python | Language::Kani | Language::Tla => Availability::Available,
    }
}

pub fn language_name(l: Language) -> &'static str {
    match l {
        Language::Rust => "rust",
        Language::Verus => "verus",
        Language::Python => "python",
        Language::Tla => "tla",
        Language::Kani => "kani",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    UnsupportedRunner {
        language: Language,
        runner: String,
        expected: &'static str,
    },
    UnsupportedMutationTool {
        language: Language,
        tool: String,
        expected: &'static str,
    },
    /// An adapter option this language's adapter does not take, or a value it cannot take.
    UnsupportedOption {
        language: Language,
        option: &'static str,
        message: &'static str,
    },
    UnsafeRoot {
        root: String,
        rule: String,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::UnsupportedRunner { language, runner, expected } => write!(
                f,
                "proob/config.toml: adapter {}: runner `{runner}` is not supported; use `{expected}`",
                language_name(*language)
            ),
            RegistryError::UnsupportedMutationTool { language, tool, expected } => write!(
                f,
                "proob/config.toml: adapter {}: mutation_tool `{tool}` is not supported; use `{expected}`",
                language_name(*language)
            ),
            RegistryError::UnsupportedOption { language, option, message } => {
                write!(f, "proob/config.toml: adapter {}: `{option}` {message}", language_name(*language))
            }
            RegistryError::UnsafeRoot { root, rule } => {
                write!(f, "proob/config.toml: adapter root `{root}` is not a root-relative path: {rule}")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

/// The adapters a config lists and this proob has, in config order. A
/// language without an adapter yet is left out (`availability` says which);
/// a listed runner or mutation tool the adapter does not drive is an error.
pub fn adapters_for(config: &Config) -> Result<Vec<Box<dyn Adapter>>, RegistryError> {
    config.adapters.iter().filter_map(|a| adapter_for(a, config.witness.into()).transpose()).collect()
}

fn adapter_for(a: &AdapterConfig, witness: WitnessPolicy) -> Result<Option<Box<dyn Adapter>>, RegistryError> {
    let root = argv::Checked::path(Path::new(&a.root))
        .map_err(|rule| RegistryError::UnsafeRoot { root: a.root.clone(), rule })?;
    if a.language != Language::Rust && a.sets_cargo_features() {
        return Err(RegistryError::UnsupportedOption {
            language: a.language,
            option: "features",
            message: "(and `all_features`, `no_default_features`) are cargo options: only a rust adapter takes them",
        });
    }
    match a.language {
        Language::Rust => {
            rust::RustAdapter::configured(a, root, witness).map(|r| Some(Box::new(r) as Box<dyn Adapter>))
        }
        Language::Verus => verus::VerusAdapter::configured(a, root).map(|v| Some(Box::new(v) as Box<dyn Adapter>)),
        Language::Python => {
            python::PythonAdapter::configured(a, root, witness).map(|p| Some(Box::new(p) as Box<dyn Adapter>))
        }
        Language::Kani => kani::KaniAdapter::configured(a, root).map(|k| Some(Box::new(k) as Box<dyn Adapter>)),
        Language::Tla => tla::TlaAdapter::configured(a, root).map(|t| Some(Box::new(t) as Box<dyn Adapter>)),
    }
}
