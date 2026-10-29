//! Evidence retrieval: the records of accepted evidence-workflow runs on the
//! default branch, newest first, within the freshness window.
//!
//! For each run the evidence workflow produced on the default branch, newest
//! first: the workflow file's blob at the run's head is read from history,
//! the run is put to the verified `accept_run`, and only an accepted run's
//! `proob-evidence` artifact is downloaded and read. The artifact is a zip
//! holding `evidence.jsonl`; each line is a record (facts crate) and becomes
//! core `Evidence` only if it names the run it came from (run id, head sha,
//! workflow blob). A kind this proob does not know is skipped; a malformed
//! line is refused by number. The walk stops when every wanted (property,
//! kind) has a primary record and a score record, or the window or the run
//! list ends. `latest_run_head` is the head of the newest accepted run, which
//! `derive` compares with the default head to tell `Pending` from `Stale`.
use crate::error::EvidenceError;
use crate::git::Repo;
use crate::github::{GitHubApi, RunFilter};
use proob_core::evidence_accept::accept_run;
use proob_core::types::{Evidence, Kind, Measured, Oid, PropertyId, RunExpectation, Secs};
use proob_facts::convert;
use proob_facts::date::parse_timestamp;
use proob_facts::evidence::{ARTIFACT_NAME, Line, RECORDS_FILE, parse_lines};
use proob_facts::github::WorkflowRun;
use proob_facts::hex;
use std::collections::BTreeSet;
use std::io::Read;

/// What to retrieve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceQuery<'a> {
    pub expectation: &'a RunExpectation,
    pub now: Secs,
    pub freshness_secs: Secs,
    /// The properties whose records are wanted, with the kind each is checked by.
    pub wanted: &'a [(PropertyId, Kind)],
    /// Most runs to list.
    pub max_runs: usize,
}

/// What was retrieved. `notes` say which runs were skipped and why, for the footer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EvidenceSet {
    pub records: Vec<Evidence>,
    pub latest_run_head: Option<Oid>,
    pub runs_accepted: usize,
    pub notes: Vec<String>,
}

impl EvidenceSet {
    /// The records for one property, as `derive` wants them.
    pub fn records_for(&self, id: PropertyId) -> Vec<Evidence> {
        self.records.iter().filter(|r| r.property == id).copied().collect()
    }
}

/// The workflow file name GitHub's runs endpoint takes: the path's last segment.
fn workflow_file(path: &[u8]) -> String {
    let text = String::from_utf8_lossy(path);
    text.rsplit('/').next().unwrap_or(&text).to_string()
}

/// Which (property, kind, score?) cells are still wanted.
struct Coverage {
    missing: BTreeSet<(PropertyId, bool)>,
}

impl Coverage {
    fn new(wanted: &[(PropertyId, Kind)]) -> Coverage {
        Coverage { missing: wanted.iter().flat_map(|(id, _)| [(*id, false), (*id, true)]).collect() }
    }

    fn note(&mut self, record: &Evidence, wanted: &[(PropertyId, Kind)]) {
        match record.measured {
            Measured::Score(_) => {
                self.missing.remove(&(record.property, true));
            }
            Measured::Primary(kind, _) => {
                if wanted.contains(&(record.property, kind)) {
                    self.missing.remove(&(record.property, false));
                }
            }
        }
    }

    fn complete(&self) -> bool {
        self.missing.is_empty()
    }
}

/// Retrieves evidence as described in the module comment.
pub fn retrieve(api: &dyn GitHubApi, repo: &dyn Repo, q: &EvidenceQuery<'_>) -> Result<EvidenceSet, EvidenceError> {
    let branch = String::from_utf8_lossy(&q.expectation.default_branch).into_owned();
    let filter = RunFilter { branch: Some(&branch), event: None, status: Some("success"), max_runs: q.max_runs };
    let mut runs = api.workflow_runs(&workflow_file(&q.expectation.workflow_path), &filter)?;
    runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let wanted_ids: BTreeSet<PropertyId> = q.wanted.iter().map(|(id, _)| *id).collect();
    let mut coverage = Coverage::new(q.wanted);
    let mut set = EvidenceSet::default();
    for run in &runs {
        if coverage.complete() {
            break;
        }
        if !within_window(run, q.now, q.freshness_secs) {
            set.notes.push(format!("run {}: older than the freshness window", run.id));
            continue;
        }
        let workflow_blob = blob_of_workflow(repo, run, &q.expectation.workflow_path);
        let facts = convert::run_facts(run, workflow_blob)?;
        if !accept_run(&facts, q.expectation) {
            set.notes.push(format!("run {}: not an accepted evidence run", run.id));
            continue;
        }
        set.runs_accepted += 1;
        set.latest_run_head.get_or_insert(facts.head_sha);
        for record in records_of(api, run, workflow_blob, &wanted_ids)? {
            coverage.note(&record, q.wanted);
            set.records.push(record);
        }
    }
    Ok(set)
}

fn within_window(run: &WorkflowRun, now: Secs, freshness: Secs) -> bool {
    match parse_timestamp(&run.created_at) {
        Ok(created) => created <= now && now - created <= freshness,
        Err(_) => false,
    }
}

/// The workflow file's blob at the run's head; a commit history cannot read is `None`.
fn blob_of_workflow(repo: &dyn Repo, run: &WorkflowRun, workflow_path: &[u8]) -> Option<Oid> {
    let path = String::from_utf8_lossy(workflow_path);
    repo.blob_at(&run.head_sha, &path).ok().flatten()
}

/// The wanted records in an accepted run's evidence artifacts.
fn records_of(
    api: &dyn GitHubApi,
    run: &WorkflowRun,
    workflow_blob: Option<Oid>,
    wanted: &BTreeSet<PropertyId>,
) -> Result<Vec<Evidence>, EvidenceError> {
    let mut out = Vec::new();
    for artifact in api.artifacts(run.id)?.iter().filter(|a| a.name == ARTIFACT_NAME && !a.expired) {
        let bytes = api.download_artifact(run.id, artifact.id)?;
        let text = records_file(run.id, artifact.id, &bytes)?;
        for (number, line) in parse_lines(&text) {
            let malformed = |message: String| EvidenceError::MalformedLine {
                run_id: run.id,
                artifact: artifact.id,
                line: number,
                message,
            };
            match line.map_err(|e| malformed(e.to_string()))? {
                Line::Blank | Line::UnknownKind(_) => {}
                Line::Record(record) => {
                    let evidence = convert::evidence(&record).map_err(|e| malformed(e.to_string()))?;
                    if !wanted.contains(&evidence.property) {
                        continue;
                    }
                    if let Some(message) = provenance_mismatch(&evidence, run, workflow_blob) {
                        return Err(malformed(message));
                    }
                    out.push(evidence);
                }
            }
        }
    }
    Ok(out)
}

/// A record must name the run it came from.
fn provenance_mismatch(record: &Evidence, run: &WorkflowRun, workflow_blob: Option<Oid>) -> Option<String> {
    if record.run_id != run.id {
        return Some(format!("record names run {} but came from run {}", record.run_id, run.id));
    }
    if hex::encode_oid(&record.head_sha) != run.head_sha {
        return Some(format!(
            "record names head {} but the run's head is {}",
            hex::encode_oid(&record.head_sha),
            run.head_sha
        ));
    }
    if Some(record.workflow_blob) != workflow_blob {
        return Some("record names a workflow blob other than the one the run executed".to_string());
    }
    None
}

/// `evidence.jsonl` out of the artifact zip.
fn records_file(run_id: u64, artifact: u64, bytes: &[u8]) -> Result<String, EvidenceError> {
    let archive_error = |message: String| EvidenceError::Archive { run_id, artifact, message };
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| archive_error(e.to_string()))?;
    let mut file = archive.by_name(RECORDS_FILE).map_err(|e| archive_error(format!("{RECORDS_FILE}: {e}")))?;
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|e| archive_error(format!("{RECORDS_FILE}: {e}")))?;
    Ok(text)
}
