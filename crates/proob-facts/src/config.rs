//! The config file: `proob/config.toml`.
//!
//! Governs the gate only when the file itself is approved; otherwise
//! `Config::defaults()` applies and the headline says `Re-approve (config
//! amended)`. The policy table is total: every verdict row is a required key,
//! so a missing row fails to parse naming the row. The kill floor has a schema
//! minimum of one that config cannot lower. Only `broken_affected` may say
//! `block` (R10: a PR is blocked only by a broken property it affects); any
//! other row set to `block` fails to parse naming the row, and the verified
//! `policy` caps such a row at `warn` regardless.
use crate::error::ParseError;
use serde::{Deserialize, Serialize};

/// The defaults, as one table so `init` and the fallback agree.
pub mod defaults {
    pub const SCHEMA_VERSION: u32 = 1;
    pub const GRACE_DAYS: u32 = 7;
    pub const FRESHNESS_DAYS: u32 = 14;
    pub const KILL_FLOOR: u32 = 1;
    pub const PROPOSAL_CAP: u32 = 5;
    pub const DRIFT_THRESHOLD: u32 = 10;
    pub const CALL_BUDGET: u32 = 200;
    pub const ALLOW_SELF_SIGNED: bool = true;
    pub const WAIVER_MAX_DAYS: u32 = 90;
    /// Cases per property-based test in the scheduled evidence run; the gate
    /// and the push run use the adapter's own default.
    pub const SCHEDULED_CASES: u32 = 2000;

    pub(super) fn grace_days() -> u32 {
        GRACE_DAYS
    }
    pub(super) fn freshness_days() -> u32 {
        FRESHNESS_DAYS
    }
    pub(super) fn kill_floor() -> u32 {
        KILL_FLOOR
    }
    pub(super) fn proposal_cap() -> u32 {
        PROPOSAL_CAP
    }
    pub(super) fn drift_threshold() -> u32 {
        DRIFT_THRESHOLD
    }
    pub(super) fn call_budget() -> u32 {
        CALL_BUDGET
    }
    pub(super) fn allow_self_signed() -> bool {
        ALLOW_SELF_SIGNED
    }
    pub(super) fn waiver_max_days() -> u32 {
        WAIVER_MAX_DAYS
    }
    pub(super) fn scheduled_cases() -> u32 {
        SCHEDULED_CASES
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    #[serde(default)]
    pub adapters: Vec<Adapter>,
    /// Required in full; see `PolicyTable`.
    pub policy: PolicyTable,
    /// Days after approval during which an unchecked property stays silent.
    #[serde(default = "defaults::grace_days")]
    pub grace_days: u32,
    /// Age beyond which score evidence is stale.
    #[serde(default = "defaults::freshness_days")]
    pub freshness_days: u32,
    /// Minimum kills for a check to count as able to fail; at least one.
    #[serde(default = "defaults::kill_floor")]
    pub kill_floor: u32,
    /// Most proposals declare writes per PR.
    #[serde(default = "defaults::proposal_cap")]
    pub proposal_cap: u32,
    /// Scope commits since approval above which guard proposes re-affirmation.
    #[serde(default = "defaults::drift_threshold")]
    pub drift_threshold: u32,
    /// GitHub calls per run; files beyond it render `Unverified (rate limited)`.
    #[serde(default = "defaults::call_budget")]
    pub call_budget: u32,
    /// Whether a merge without an eligible review counts as `SelfSigned`.
    #[serde(default = "defaults::allow_self_signed")]
    pub allow_self_signed: bool,
    #[serde(default)]
    pub witness: Witness,
    #[serde(default = "defaults::waiver_max_days")]
    pub waiver_max_days: u32,
    /// Cases per property-based test in the scheduled evidence run (the large
    /// budget); the gate and the push run use the adapter's default.
    #[serde(default = "defaults::scheduled_cases")]
    pub scheduled_cases: u32,
    /// Scope globs of changed source files that need no property (generated
    /// or vendored code); `check` leaves them out of its "new code without a
    /// property" line. Informational only: it never changes a verdict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncovered_ignore: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    /// Root of the project the adapter runs in, relative to the repository.
    pub root: String,
    pub language: Language,
    /// The test runner the adapter drives (U5 fixes the names).
    pub runner: String,
    /// The mutation tool the adapter drives (U5 fixes the names).
    pub mutation_tool: String,
    /// The most mutants one score run tests; the rest are reported as
    /// unsampled. Default 25 for verus (mutation-under-proof, a prefix by
    /// operator kind then position) and 30 for rust and python (a sample
    /// chosen from the property id). At least 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_mutants: Option<u32>,
    /// rust and python: mutants tested in parallel. Default: half the CPUs,
    /// bounded by 80% of the temporary directory's free space over the bytes
    /// one copy of the tree needs (rust: measured; else 1.5 GB), at least 1.
    /// At least 1; verus rejects it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jobs: Option<u32>,
    /// rust only: cargo features passed as `--features <name>` to every cargo
    /// invocation the adapter makes (check, mutant listing, mutation run).
    /// Each a cargo feature name (`feature_name`); other languages reject it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// rust only: `--all-features` on every cargo invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_features: Option<bool>,
    /// rust only: `--no-default-features` on every cargo invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_default_features: Option<bool>,
}

impl Adapter {
    /// Whether any of the cargo feature options is set (only rust takes them).
    pub fn sets_cargo_features(&self) -> bool {
        !self.features.is_empty() || self.all_features.is_some() || self.no_default_features.is_some()
    }
}

/// A feature name as `cargo --features` takes it: `name` or `package/name`,
/// each part ASCII letters, digits, `_`, `-`, `+`, `.`, starting with a
/// letter, digit, or `_`, so it is never read as an option or a list. Pure.
pub fn feature_name(text: &str) -> bool {
    let part = |p: &str| {
        p.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
            && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+' | b'.'))
    };
    match text.split_once('/') {
        Some((package, name)) => part(package) && part(name),
        None => part(text),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    Verus,
    Python,
    Tla,
}

/// How much of a counterexample evidence records carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Witness {
    #[default]
    Full,
    SeedOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutcomeName {
    Pass,
    Warn,
    Block,
}

/// The gate policy, one row per verdict, split where the default table
/// distinguishes cases: `NotChecked` by the grace window, `Broken` by whether
/// this PR affects the property. All nine rows are required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyTable {
    pub holds: OutcomeName,
    pub checked: OutcomeName,
    pub proposed: OutcomeName,
    pub waived: OutcomeName,
    pub not_checked_within_grace: OutcomeName,
    pub not_checked_after_grace: OutcomeName,
    pub re_approve: OutcomeName,
    pub broken_affected: OutcomeName,
    pub broken_unaffected: OutcomeName,
}

impl PolicyTable {
    /// The plan's "Gate policy defaults" table.
    pub const DEFAULTS: PolicyTable = PolicyTable {
        holds: OutcomeName::Pass,
        checked: OutcomeName::Pass,
        proposed: OutcomeName::Pass,
        waived: OutcomeName::Pass,
        not_checked_within_grace: OutcomeName::Pass,
        not_checked_after_grace: OutcomeName::Warn,
        re_approve: OutcomeName::Warn,
        broken_affected: OutcomeName::Block,
        broken_unaffected: OutcomeName::Pass,
    };
}

impl Config {
    /// The built-in configuration: what `init` writes (less adapters) and what
    /// check mode applies when the base ref's config is not approved.
    pub fn defaults() -> Config {
        Config {
            schema_version: defaults::SCHEMA_VERSION,
            adapters: Vec::new(),
            policy: PolicyTable::DEFAULTS,
            grace_days: defaults::GRACE_DAYS,
            freshness_days: defaults::FRESHNESS_DAYS,
            kill_floor: defaults::KILL_FLOOR,
            proposal_cap: defaults::PROPOSAL_CAP,
            drift_threshold: defaults::DRIFT_THRESHOLD,
            call_budget: defaults::CALL_BUDGET,
            allow_self_signed: defaults::ALLOW_SELF_SIGNED,
            witness: Witness::Full,
            waiver_max_days: defaults::WAIVER_MAX_DAYS,
            scheduled_cases: defaults::SCHEDULED_CASES,
            uncovered_ignore: Vec::new(),
        }
    }
}

/// Parses a config file and checks the ranges the schema fixes.
pub fn parse_config(text: &str) -> Result<Config, ParseError> {
    let config: Config = toml::from_str(text).map_err(|e| ParseError::Schema(e.to_string()))?;
    check_ranges(&config)?;
    check_uncovered_ignore(&config.uncovered_ignore)?;
    check_features(&config.adapters)?;
    Ok(config)
}

/// Every adapter's `features` entry is a cargo feature name.
fn check_features(adapters: &[Adapter]) -> Result<(), ParseError> {
    adapters.iter().enumerate().try_for_each(|(i, a)| match a.features.iter().position(|f| !feature_name(f)) {
        Some(j) => Err(ParseError::Schema(format!(
            "adapters[{i}].features[{j}]: {:?} is not a cargo feature name (`name` or `package/name`: letters, digits, `_`, `-`, `+`, `.`)",
            a.features[j]
        ))),
        None => Ok(()),
    })
}

/// Every `uncovered_ignore` entry is a scope glob, by the same grammar.
fn check_uncovered_ignore(globs: &[String]) -> Result<(), ParseError> {
    globs.iter().enumerate().try_for_each(|(i, g)| {
        crate::convert::glob(g.as_bytes())
            .map_err(|rule| ParseError::Schema(format!("uncovered_ignore[{i}]: not a scope glob: {rule}")))
    })
}

fn check_ranges(config: &Config) -> Result<(), ParseError> {
    match config {
        Config { schema_version, .. } if *schema_version != defaults::SCHEMA_VERSION => Err(ParseError::Invalid {
            field: "schema_version",
            message: "unsupported schema version; this proob reads schema_version 1",
        }),
        Config { kill_floor: 0, .. } => Err(ParseError::Invalid {
            field: "kill_floor",
            message: "must be at least 1; the schema minimum cannot be lowered",
        }),
        Config { policy, .. } => match blocking_row(policy) {
            Some(field) => Err(ParseError::Invalid {
                field,
                message: "only broken_affected may block: a PR is blocked only by a broken property it affects \
                          (R10); use \"warn\" or \"pass\"",
            }),
            None => Ok(()),
        },
    }
}

/// The first row other than `broken_affected` that says `block`, by name.
fn blocking_row(p: &PolicyTable) -> Option<&'static str> {
    [
        ("policy.holds", p.holds),
        ("policy.checked", p.checked),
        ("policy.proposed", p.proposed),
        ("policy.waived", p.waived),
        ("policy.not_checked_within_grace", p.not_checked_within_grace),
        ("policy.not_checked_after_grace", p.not_checked_after_grace),
        ("policy.re_approve", p.re_approve),
        ("policy.broken_unaffected", p.broken_unaffected),
    ]
    .into_iter()
    .find(|(_, outcome)| *outcome == OutcomeName::Block)
    .map(|(row, _)| row)
}
