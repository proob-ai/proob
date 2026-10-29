//! The run-result record: what this PR's run, or a local run, measured.
//!
//! The same measurement shape as an evidence record, deliberately without a
//! run id or workflow blob: a result is not evidence. Adapters (U5) write one
//! JSON object per line to `results.jsonl`; the execute job uploads that file
//! raw, beside a `manifest.json` naming the run that produced it, and the
//! judge job hands it to `check --results`, where each line becomes a core
//! `RunResult` and reaches `derive` only as `Provisional`. No function turns a
//! core `RunResult` into a core `Evidence`. The one place a record becomes an
//! evidence record is the evidence workflow's judge job on the default branch
//! (U10), which stamps the provenance of its own run after checking that the
//! manifest names that run; every later reader re-checks that provenance
//! against GitHub's run metadata before anything becomes `Evidence`.
use crate::error::ParseError;
use crate::evidence::{Algo, EvidenceKind, OutcomeRecord, Replay};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The artifact the execute job uploads, and the files inside it.
pub const ARTIFACT_NAME: &str = "proob-results";
pub const RESULTS_FILE: &str = "results.jsonl";
pub const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunResultRecord {
    pub schema_version: u32,
    /// The property's ULID.
    pub id: String,
    pub kind: EvidenceKind,
    pub algo: Algo,
    /// Hex digests at the head the run checked out.
    pub content_hash: String,
    pub binding_hash: String,
    pub scope_hash: String,
    pub outcome: OutcomeRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<Replay>,
    #[serde(default)]
    pub tool_versions: BTreeMap<String, String>,
    /// Hex git id of the commit the run checked out, when the adapter knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
}

/// Parses one record.
pub fn parse_result(json: &str) -> Result<RunResultRecord, ParseError> {
    serde_json::from_str(json).map_err(|e| ParseError::Schema(e.to_string()))
}

/// Every non-blank line of a results file with its 1-based number.
pub fn parse_results(text: &str) -> impl Iterator<Item = (usize, Result<RunResultRecord, ParseError>)> + '_ {
    text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()).map(|(i, line)| (i + 1, parse_result(line.trim())))
}

// ---------------------------------------------------------------------------
// The manifest: which run wrote the results
// ---------------------------------------------------------------------------

/// The GitHub event that started the execute job, closed to the events the
/// two proob workflows run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestEvent {
    PullRequest,
    Push,
    Schedule,
    WorkflowDispatch,
}

/// Which mode the execute job ran in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestMode {
    Check,
    Evidence,
}

/// What was run for one property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attempted {
    Check,
    Score,
}

/// One run the execute job made, and how it came out (`OutcomeRecord::name`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub id: String,
    pub what: Attempted,
    pub outcome: String,
}

/// One bound property the execute job could not run, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skipped {
    pub id: String,
    pub what: Attempted,
    pub reason: String,
}

/// `manifest.json`, written by the execute job beside `results.jsonl`. The
/// judge job accepts the results only when `run_id` and `head_sha` are its
/// own run's; `run_attempt` is informational, since a re-run of the judge
/// job alone reads the artifact of an earlier attempt of the same run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub mode: ManifestMode,
    pub event: ManifestEvent,
    /// `owner/name`.
    pub repository: String,
    pub run_id: u64,
    pub run_attempt: u32,
    /// Hex git id of the commit the execute job checked out.
    pub head_sha: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    /// Seconds since the Unix epoch.
    pub created_at: u64,
    pub attempted: Vec<Attempt>,
    pub skipped: Vec<Skipped>,
    /// Why nothing (or less than everything) ran, for the judge's summary.
    #[serde(default)]
    pub notes: Vec<String>,
}

pub fn parse_manifest(json: &str) -> Result<Manifest, ParseError> {
    serde_json::from_str(json).map_err(|e| ParseError::Schema(e.to_string()))
}
