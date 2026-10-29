//! Conversions from file shapes to core types: the only place strings become
//! numbers. Every error names the field path it is about.
//!
//! The verified checkers are the authority on the grammar; the plain-Rust
//! rule finder here exists only to name the rule a rejected glob broke.
use crate::config::{Config, OutcomeName, PolicyTable};
use crate::date::{Date, SECS_PER_DAY};
use crate::error::{ConvertError, ConvertErrorKind};
use crate::evidence::{EvidenceKind, EvidenceRecord, OutcomeRecord};
use crate::github;
use crate::property::{KindName, PropertyFile, SeverityName, WaiverFile};
use crate::results::RunResultRecord;
use crate::{hex, ulid};
use proob_core::grammar::{glob_byte_ok_exec, glob_ok_exec, scope_ok_exec};
use proob_core::types::{
    Evidence, GateConfig, Kind, Measured, Oid, Outcome, Policy, PrimaryOutcome, Property, ReviewState, RunConclusion,
    RunEvent, RunFacts, RunResult, ScoreOutcome, Secs, Severity, UserKind, Waiver,
};
use std::fmt;

/// A property file as the core sees it. `at` is the instant the waiver window
/// is measured from (the clock locally; the approving merge when known), so
/// that `until - at` is bounded by `waiver_max_days`.
pub fn property(file: &PropertyFile, at: Secs, waiver_max_days: u32) -> Result<Property, ConvertError> {
    Ok(Property {
        id: ulid::decode(&file.id).map_err(|e| ConvertError::at("id", e))?,
        kind: file.kind.map(kind),
        scope: scope(&file.scope)?,
        severity: severity(file.severity),
        waiver: file.waiver.as_ref().map(|w| waiver(w, at, waiver_max_days)).transpose()?,
        reviewed: file.reviewed.map(Date::to_secs),
    })
}

fn scope(globs: &[String]) -> Result<Vec<Vec<u8>>, ConvertError> {
    if globs.is_empty() {
        return Err(ConvertError::at("scope", ConvertErrorKind::EmptyScope));
    }
    let scope = globs
        .iter()
        .enumerate()
        .map(|(i, g)| {
            glob(g.as_bytes())
                .map(|()| g.as_bytes().to_vec())
                .map_err(|rule| ConvertError::at(format!("scope[{i}]"), rule))
        })
        .collect::<Result<Vec<_>, _>>()?;
    // The verified checker decides the whole list; the per-glob pass above only
    // names rules.
    if scope_ok_exec(&scope) { Ok(scope) } else { Err(ConvertError::at("scope", GlobRule::Rejected)) }
}

fn waiver(w: &WaiverFile, at: Secs, max_days: u32) -> Result<Waiver, ConvertError> {
    let until = w.until.to_secs();
    let max_secs = u64::from(max_days) * SECS_PER_DAY;
    match until.checked_sub(at) {
        Some(window) if window > max_secs => Err(ConvertError::at(
            "waiver.until",
            ConvertErrorKind::WaiverTooLong { days: window.div_ceil(SECS_PER_DAY), max_days },
        )),
        _ => Ok(Waiver { until }),
    }
}

pub fn kind(k: KindName) -> Kind {
    match k {
        KindName::Test => Kind::Test,
        KindName::Proof => Kind::Proof,
        KindName::Model => Kind::Model,
    }
}

pub fn severity(s: SeverityName) -> Severity {
    match s {
        SeverityName::Low => Severity::Low,
        SeverityName::Medium => Severity::Medium,
        SeverityName::High => Severity::High,
        SeverityName::Critical => Severity::Critical,
    }
}

// ---------------------------------------------------------------------------
// Globs
// ---------------------------------------------------------------------------

/// The rule a rejected glob broke, in the order the grammar states them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobRule {
    Empty,
    LeadingSlash,
    LeadingDash,
    ParentSegment {
        at: usize,
    },
    Byte {
        at: usize,
        byte: u8,
    },
    /// The verified checker rejected the glob and no rule here explains it:
    /// the rule finder and the checker disagree, which is a bug to report.
    Rejected,
}

impl fmt::Display for GlobRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GlobRule::Empty => write!(f, "empty glob (a scope glob names at least one path)"),
            GlobRule::LeadingSlash => write!(f, "leading `/` (scope globs are root-relative)"),
            GlobRule::LeadingDash => write!(f, "leading `-` (a glob is never an option; it would steer the runner)"),
            GlobRule::ParentSegment { at } => {
                write!(f, "`..` segment at byte {at} (a glob never reaches above the root)")
            }
            GlobRule::Byte { at, byte } => write!(
                f,
                "byte {byte:#04x} at index {at} is outside the glob alphabet (ASCII letters, digits, and \
                 / . _ - * ? [ ] {{ }} , @ + ~)"
            ),
            GlobRule::Rejected => {
                write!(f, "rejected by the verified checker without a named rule (report this)")
            }
        }
    }
}

/// Accepts exactly the globs `proob_core::grammar::glob_ok` accepts, naming
/// the broken rule otherwise.
pub fn glob(bytes: &[u8]) -> Result<(), GlobRule> {
    if glob_ok_exec(bytes) { Ok(()) } else { Err(first_broken_rule(bytes).unwrap_or(GlobRule::Rejected)) }
}

fn first_broken_rule(bytes: &[u8]) -> Option<GlobRule> {
    match bytes.first() {
        None => Some(GlobRule::Empty),
        Some(b'/') => Some(GlobRule::LeadingSlash),
        Some(b'-') => Some(GlobRule::LeadingDash),
        Some(_) => bytes.iter().enumerate().find_map(|(at, &byte)| broken_at(bytes, at, byte)),
    }
}

fn broken_at(bytes: &[u8], at: usize, byte: u8) -> Option<GlobRule> {
    if !glob_byte_ok_exec(byte) {
        Some(GlobRule::Byte { at, byte })
    } else if parent_segment_at(bytes, at) {
        Some(GlobRule::ParentSegment { at })
    } else {
        None
    }
}

fn parent_segment_at(s: &[u8], i: usize) -> bool {
    s.get(i) == Some(&b'.')
        && s.get(i + 1) == Some(&b'.')
        && (i == 0 || s.get(i - 1) == Some(&b'/'))
        && (i + 2 == s.len() || s.get(i + 2) == Some(&b'/'))
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

pub fn outcome(o: OutcomeName) -> Outcome {
    match o {
        OutcomeName::Pass => Outcome::Pass,
        OutcomeName::Warn => Outcome::Warn,
        OutcomeName::Block => Outcome::Block,
    }
}

pub fn policy(t: &PolicyTable) -> Policy {
    Policy {
        holds: outcome(t.holds),
        checked: outcome(t.checked),
        proposed: outcome(t.proposed),
        waived: outcome(t.waived),
        not_checked_within_grace: outcome(t.not_checked_within_grace),
        not_checked_after_grace: outcome(t.not_checked_after_grace),
        re_approve: outcome(t.re_approve),
        broken_affected: outcome(t.broken_affected),
        broken_unaffected: outcome(t.broken_unaffected),
    }
}

fn days(n: u32) -> Secs {
    u64::from(n) * SECS_PER_DAY
}

/// The slice of config the verified core reads. `kill_floor` is zero only in a
/// hand-built `Config`; `parse_config` already refuses it.
pub fn gate_config(c: &Config) -> Result<GateConfig, ConvertError> {
    match c.kill_floor {
        0 => Err(ConvertError::at("kill_floor", ConvertErrorKind::FloorZero)),
        floor => Ok(GateConfig {
            policy: policy(&c.policy),
            grace_secs: days(c.grace_days),
            freshness_secs: days(c.freshness_days),
            floor,
            allow_self_signed: c.allow_self_signed,
        }),
    }
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

/// A record as the core sees it. Provenance is U9's to establish first; this
/// only checks that the record is coherent and its fields decode.
pub fn evidence(r: &EvidenceRecord) -> Result<Evidence, ConvertError> {
    Ok(Evidence {
        property: ulid::decode(&r.id).map_err(|e| ConvertError::at("id", e))?,
        content_hash: digest("content_hash", &r.content_hash)?,
        binding_hash: digest("binding_hash", &r.binding_hash)?,
        scope_hash: digest("scope_hash", &r.scope_hash)?,
        spec_hash: r.spec_hash.as_deref().map(|h| digest("spec_hash", h)).transpose()?,
        measured: measured(r.kind, &r.outcome)?,
        run_id: r.run_id,
        head_sha: hex::decode_oid(&r.head_sha).map_err(|e| ConvertError::at("head_sha", e))?,
        workflow_blob: hex::decode_oid(&r.workflow_blob).map_err(|e| ConvertError::at("workflow_blob", e))?,
        at: r.at,
    })
}

/// A run result as the core sees it: the same measurement, no provenance,
/// and a different type, so it can only ever be `Provisional`.
pub fn run_result(r: &RunResultRecord) -> Result<RunResult, ConvertError> {
    Ok(RunResult {
        property: ulid::decode(&r.id).map_err(|e| ConvertError::at("id", e))?,
        content_hash: digest("content_hash", &r.content_hash)?,
        binding_hash: digest("binding_hash", &r.binding_hash)?,
        scope_hash: digest("scope_hash", &r.scope_hash)?,
        measured: measured(r.kind, &r.outcome)?,
    })
}

fn digest(field: &'static str, text: &str) -> Result<[u8; 32], ConvertError> {
    hex::decode_digest(text).map_err(|e| ConvertError::at(field, e))
}

fn primary(kind: Kind, o: PrimaryOutcome) -> Result<Measured, ConvertError> {
    Ok(Measured::Primary(kind, o))
}

/// Pairs the record's kind with its outcome; any other pairing is incoherent.
fn measured(kind: EvidenceKind, o: &OutcomeRecord) -> Result<Measured, ConvertError> {
    use EvidenceKind as K;
    use OutcomeRecord as O;
    match (kind, o) {
        (K::Check, O::NoCounterexample { .. }) => primary(Kind::Test, PrimaryOutcome::NoCounterexample),
        (K::Check, O::Counterexample { .. }) => primary(Kind::Test, PrimaryOutcome::Counterexample),
        (K::Check, O::Errored { .. }) => primary(Kind::Test, PrimaryOutcome::Errored),
        (K::Proof, O::Verified { .. }) => primary(Kind::Proof, PrimaryOutcome::Verified),
        (K::Proof, O::ProofFailed { .. }) => primary(Kind::Proof, PrimaryOutcome::ProofFailed),
        (K::Proof, O::Errored { .. }) => primary(Kind::Proof, PrimaryOutcome::Errored),
        (K::Model, O::NoViolation) => primary(Kind::Model, PrimaryOutcome::NoViolation),
        (K::Model, O::Violation { .. }) => primary(Kind::Model, PrimaryOutcome::Violation),
        (K::Model, O::Errored { .. }) => primary(Kind::Model, PrimaryOutcome::Errored),
        (K::Score, O::Scored { killed, survived, timeout, unviable, .. }) => {
            Ok(Measured::Score(ScoreOutcome::Scored {
                killed: *killed,
                survived: *survived,
                timeout: *timeout,
                unviable: *unviable,
            }))
        }
        (K::Score, O::NoMutants) => Ok(Measured::Score(ScoreOutcome::NoMutants)),
        (K::Score, O::Errored { .. }) => Ok(Measured::Score(ScoreOutcome::Errored)),
        (kind, outcome) => Err(ConvertError::at(
            "outcome",
            ConvertErrorKind::Incoherent { kind: kind.name(), outcome: outcome.name() },
        )),
    }
}

// ---------------------------------------------------------------------------
// GitHub facts
// ---------------------------------------------------------------------------

/// What an account is to the eligibility rule: `Human` is `type: User` with
/// a login not ending in `[bot]`; a deleted account (`None`) or a type GitHub
/// may add is `Unknown` and never counts.
pub fn user_kind(user: Option<&github::User>) -> UserKind {
    match user {
        Some(github::User { kind: github::AccountType::User, login, .. }) if !login.ends_with("[bot]") => {
            UserKind::Human
        }
        Some(github::User { kind: github::AccountType::User | github::AccountType::Bot, .. }) => UserKind::Bot,
        Some(github::User { kind: github::AccountType::Organization | github::AccountType::Unknown, .. }) | None => {
            UserKind::Unknown
        }
    }
}

/// Write access now: `admin` or `write`, or a role GitHub reports as
/// `maintain` (whose `permission` is `write` already) or `admin`. `read`,
/// `triage`, `none`, and anything new fail closed.
pub fn has_write(p: &github::Permission) -> bool {
    matches!(p.permission, github::PermissionLevel::Admin | github::PermissionLevel::Write)
        || matches!(p.role_name.as_deref(), Some("admin" | "maintain" | "write"))
}

/// A review state the core knows; `None` for one GitHub may add, which the
/// facts layer skips (it can never approve).
pub fn review_state(s: github::ReviewState) -> Option<ReviewState> {
    match s {
        github::ReviewState::Approved => Some(ReviewState::Approved),
        github::ReviewState::ChangesRequested => Some(ReviewState::ChangesRequested),
        github::ReviewState::Commented => Some(ReviewState::Commented),
        github::ReviewState::Dismissed => Some(ReviewState::Dismissed),
        github::ReviewState::Pending => Some(ReviewState::Pending),
        github::ReviewState::Unknown => None,
    }
}

pub fn run_event(e: github::RunEvent) -> RunEvent {
    match e {
        github::RunEvent::Push => RunEvent::Push,
        github::RunEvent::Schedule => RunEvent::Schedule,
        github::RunEvent::WorkflowDispatch => RunEvent::WorkflowDispatch,
        github::RunEvent::PullRequest | github::RunEvent::PullRequestTarget => RunEvent::PullRequest,
        github::RunEvent::Other => RunEvent::Other,
    }
}

/// Only `success` succeeds; a run still in progress has no conclusion.
pub fn run_conclusion(c: Option<github::RunConclusion>) -> RunConclusion {
    match c {
        Some(github::RunConclusion::Success) => RunConclusion::Success,
        _ => RunConclusion::NotSuccess,
    }
}

/// A git object id from its hex text, naming the field on failure.
pub fn oid(field: &'static str, text: &str) -> Result<Oid, ConvertError> {
    hex::decode_oid(text).map_err(|e| ConvertError::at(field, e))
}

/// A run as `accept_run` sees it. `workflow_blob` is the workflow file's blob
/// at the run's head, read from history by the effects layer; `None` when
/// that commit cannot be read.
pub fn run_facts(run: &github::WorkflowRun, workflow_blob: Option<Oid>) -> Result<RunFacts, ConvertError> {
    Ok(RunFacts {
        run_id: run.id,
        workflow_path: run.path.as_bytes().to_vec(),
        head_branch: run.head_branch.as_ref().map(|b| b.as_bytes().to_vec()),
        head_sha: oid("head_sha", &run.head_sha)?,
        event: run_event(run.event),
        conclusion: run_conclusion(run.conclusion),
        workflow_blob,
    })
}
