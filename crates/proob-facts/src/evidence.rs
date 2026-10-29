//! The evidence record: one JSON object per (property, kind) in the artifact
//! of an evidence-workflow run.
//!
//! The record describes what was measured and at which hashes. Its provenance
//! (that the run was on the default branch, from the approved workflow, and
//! succeeded) is not in the record: U9 establishes it from the run's metadata
//! before any record becomes core `Evidence`. The same shape produced by a PR
//! run or a local run is a `RunResult` and never an `Evidence`.
use crate::error::ParseError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The artifact the evidence workflow uploads, and the file inside it: one
/// JSON record per line. Named once so U9's retrieval and U10's evidence job
/// agree.
pub const ARTIFACT_NAME: &str = "proob-evidence";
pub const RECORDS_FILE: &str = "evidence.jsonl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    pub schema_version: u32,
    /// The property's ULID.
    pub id: String,
    pub kind: EvidenceKind,
    /// The digest algorithm behind every hash field.
    pub algo: Algo,
    /// Hex digests (`proob-content-v1`, `proob-binding-v1`, `proob-scope-v1`).
    pub content_hash: String,
    pub binding_hash: String,
    pub scope_hash: String,
    /// Model-backed only: the hash of the spec that was checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_hash: Option<String>,
    pub outcome: OutcomeRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<Replay>,
    /// Tool name to version, as the adapter recorded them.
    #[serde(default)]
    pub tool_versions: BTreeMap<String, String>,
    /// The workflow run that produced the artifact.
    pub run_id: u64,
    /// Hex git ids: the commit the run checked out and the workflow file's
    /// blob at that commit.
    pub head_sha: String,
    pub workflow_blob: String,
    /// Seconds since the Unix epoch when the run recorded the outcome.
    pub at: u64,
}

/// What the record measures: `check` is a test's primary evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceKind {
    Check,
    Proof,
    Model,
    Score,
}

impl EvidenceKind {
    pub fn name(self) -> &'static str {
        match self {
            EvidenceKind::Check => "check",
            EvidenceKind::Proof => "proof",
            EvidenceKind::Model => "model",
            EvidenceKind::Score => "score",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Algo {
    Sha256,
}

/// The outcome, tagged by `result`. Payloads are what a reader needs to act:
/// the witness, the diagnostics, the trace, the counts, the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum OutcomeRecord {
    NoCounterexample {
        budget: u64,
    },
    Counterexample {
        witness: String,
    },
    Verified {
        rlimit: u32,
    },
    ProofFailed {
        diagnostics: String,
    },
    NoViolation,
    Violation {
        trace: String,
    },
    Scored {
        killed: u32,
        survived: u32,
        timeout: u32,
        unviable: u32,
        /// Mutants enumerated beyond the run's cap and not verified; they are
        /// in no count above (mutation-under-proof reports them).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unsampled: Option<u32>,
    },
    NoMutants,
    Errored {
        reason: String,
    },
}

impl OutcomeRecord {
    pub fn name(&self) -> &'static str {
        match self {
            OutcomeRecord::NoCounterexample { .. } => "no_counterexample",
            OutcomeRecord::Counterexample { .. } => "counterexample",
            OutcomeRecord::Verified { .. } => "verified",
            OutcomeRecord::ProofFailed { .. } => "proof_failed",
            OutcomeRecord::NoViolation => "no_violation",
            OutcomeRecord::Violation { .. } => "violation",
            OutcomeRecord::Scored { .. } => "scored",
            OutcomeRecord::NoMutants => "no_mutants",
            OutcomeRecord::Errored { .. } => "errored",
        }
    }
}

/// How to reproduce a test run: the adapter's seed and case budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replay {
    pub seed: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cases: Option<u64>,
}

pub fn parse_evidence(json: &str) -> Result<EvidenceRecord, ParseError> {
    serde_json::from_str(json).map_err(|e| ParseError::Schema(e.to_string()))
}

/// One line of the records file. A kind this proob does not know is
/// tolerated (a newer proob wrote it) rather than refused; a malformed line is
/// refused by its number.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "one line's parse, consumed at once; a Box would allocate per line for nothing"
)]
pub enum Line {
    Record(EvidenceRecord),
    UnknownKind(String),
    Blank,
}

const KNOWN_KINDS: [&str; 4] = ["check", "proof", "model", "score"];

/// Parses one line of a records file.
pub fn parse_line(text: &str) -> Result<Line, ParseError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Line::Blank);
    }
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| ParseError::Schema(e.to_string()))?;
    match value.get("kind").and_then(serde_json::Value::as_str) {
        Some(kind) if !KNOWN_KINDS.contains(&kind) => Ok(Line::UnknownKind(kind.to_string())),
        _ => serde_json::from_value(value).map(Line::Record).map_err(|e| ParseError::Schema(e.to_string())),
    }
}

/// Every line of a records file with its 1-based number.
pub fn parse_lines(text: &str) -> impl Iterator<Item = (usize, Result<Line, ParseError>)> + '_ {
    text.lines().enumerate().map(|(i, line)| (i + 1, parse_line(line)))
}
