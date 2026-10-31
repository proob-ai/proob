//! `proob run <check|score> <id>`: one property's bound check or strength
//! score through its adapter, rendered as a local run, unverified.
//!
//! The property is found, the registry's adapters discover the bindings, the
//! one carrying this id is run with the adapter's seed and budget (or the
//! flags), and the result is a `RunResultRecord`: the same measurement shape
//! as evidence with no run id and no workflow blob, so nothing can mistake
//! it for evidence. `--json` prints that record on one line; `--record`
//! appends the same line to `proob/.cache/results/results.jsonl`, the file
//! the Action's execute job uploads and its judge job hands to `check
//! --results`. Exit 0 for a positive or neutral outcome, 1 for a
//! counterexample, 2 when the run errored.
use crate::cli::Exit;
use crate::cli::adapters::{Discovered, discover};
use crate::cli::backend::with_offline_backend;
use crate::cli::error::CliError;
use crate::cli::hash::{Hashes, find, hashes};
use crate::cli::workspace::{LoadedProperty, Workspace};
use proob_adapters::{
    Availability, Binding, Budget, CheckOutcome, ProofMeasure, ScoreOutcome, Seed, availability, language_name,
};
use proob_core::types::Kind;
use proob_effects::git::{Git, Repo};
use proob_facts::config::Config;
use proob_facts::convert;
use proob_facts::evidence::{Algo, EvidenceKind, OutcomeRecord, Replay};
use proob_facts::hex;
use proob_facts::paths::RESULTS_DIR;
use proob_facts::results::{RESULTS_FILE, RunResultRecord};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum What {
    /// The kind's primary evidence: the property-based test, proof, or model.
    Check,
    /// The strength measure: mutation scoring with only the bound check.
    Score,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub budget: Option<u32>,
    pub seed: Option<u64>,
    pub record: bool,
    pub json: bool,
}

/// What one run produced, for rendering.
pub struct Outcome {
    pub record: RunResultRecord,
    pub property_path: String,
    pub title: String,
    pub check_ref: String,
    pub token: String,
    pub scope_files: usize,
    pub recorded: Option<PathBuf>,
    /// A score's parallel jobs and why (`Jobs::line`); not part of the record.
    pub jobs: Option<String>,
}

pub fn run(
    ws: &Workspace,
    config: &Config,
    properties: &[LoadedProperty],
    what: What,
    id: &str,
    opts: Options,
) -> Result<Exit, CliError> {
    let property = find(properties, id)?;
    let discovered = discover(ws, config)?.ok_or_else(|| no_adapter(config, property))?;
    let out = perform(ws, config, &discovered, property, what, opts)?;
    print(&out, opts.json)?;
    Ok(exit_of(&out.record.outcome))
}

/// One run of `what` for `property` through the adapter that bound it, as
/// `run` performs it: the same code path the Action's execute job calls in
/// process, once per property, with `record` set. Prints nothing.
pub fn perform(
    ws: &Workspace,
    config: &Config,
    discovered: &Discovered,
    property: &LoadedProperty,
    what: What,
    opts: Options,
) -> Result<Outcome, CliError> {
    let (adapter, binding) = discovered.binding(property.id).ok_or_else(|| unbound(property, discovered))?;
    check_kind(property, binding)?;
    let h = property_hashes(ws, config, discovered, property)?;
    let (outcome, replay, tool_versions, kind, jobs) = match what {
        What::Check => {
            let budget = Budget(opts.budget.unwrap_or(Budget::DEFAULT.0));
            let seed = Seed(opts.seed.unwrap_or_else(seed_from_clock));
            let check = adapter.check(&ws.root, binding, budget, seed).map_err(crate::cli::adapters::adapter_error)?;
            let (outcome, replay) = check_record(check.outcome);
            (outcome, replay, check.tool_versions, evidence_kind(binding.kind), None)
        }
        What::Score => {
            let scope: Vec<PathBuf> = h.scope_files.iter().map(PathBuf::from).collect();
            let score = adapter.score(&ws.root, binding, &scope).map_err(crate::cli::adapters::adapter_error)?;
            (score_record(score.outcome), None, score.tool_versions, EvidenceKind::Score, score.jobs.map(|j| j.line()))
        }
    };
    let record = RunResultRecord {
        schema_version: 1,
        id: property.file.id.clone(),
        kind,
        algo: Algo::Sha256,
        content_hash: h.content_hash.clone(),
        binding_hash: h.binding_hash.clone().unwrap_or_default(),
        scope_hash: h.scope_hash.clone().unwrap_or_default(),
        outcome,
        replay,
        tool_versions: tool_versions.into_iter().collect::<BTreeMap<_, _>>(),
        head_sha: head_sha(&ws.root),
    };
    let recorded = if opts.record { Some(append(&ws.root, &record)?) } else { None };
    Ok(Outcome {
        record,
        property_path: property.path.clone(),
        title: property.file.title.clone(),
        check_ref: binding.check_ref.clone(),
        token: binding.token.to_string(),
        scope_files: h.scope_files.len(),
        recorded,
        jobs,
    })
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

fn no_adapter(config: &Config, p: &LoadedProperty) -> CliError {
    match config.adapters.first() {
        None => CliError::Usage(format!(
            "cannot run {}: proob/config.toml lists no adapters; run `proob init` to detect one",
            p.file.id
        )),
        Some(a) => match availability(a.language) {
            Availability::Planned(unit) => CliError::Usage(format!(
                "cannot run {}: no adapter for language {} yet (it arrives in {unit}); the property renders Not checked (no adapter)",
                p.file.id,
                language_name(a.language)
            )),
            Availability::Available => {
                CliError::Usage(format!("cannot run {}: the adapter list resolved to nothing", p.file.id))
            }
        },
    }
}

fn unbound(p: &LoadedProperty, d: &Discovered) -> CliError {
    match d.uncovered.get(&p.id) {
        Some(u) => CliError::Usage(format!(
            "`proob:{}` at {} binds nothing: {}; the property is Not checked (no adapter)",
            p.file.id, u.token, u.reason
        )),
        None => CliError::Usage(format!(
            "no check carries `proob:{}`: the property is Not checked (no check); bind a test by writing `// proob:{}` on the line above its #[test] \
             (Rust) or `# proob:{}` on the line above a module-level pytest test function (Python), or a proof by writing it above an item in a \
             verus! block of a verified crate",
            p.file.id, p.file.id, p.file.id
        )),
    }
}

/// A property that names a kind binds only a check of that kind.
fn check_kind(p: &LoadedProperty, b: &Binding) -> Result<(), CliError> {
    match p.file.kind.map(convert::kind) {
        Some(declared) if declared != b.kind => Err(CliError::invalid(
            &p.path,
            format!("kind = {} but the bound check at {} is a {}", kind_name(declared), b.token, kind_name(b.kind)),
        )),
        _ => Ok(()),
    }
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Test => "test",
        Kind::Proof => "proof",
        Kind::Model => "model",
    }
}

fn evidence_kind(k: Kind) -> EvidenceKind {
    match k {
        Kind::Test => EvidenceKind::Check,
        Kind::Proof => EvidenceKind::Proof,
        Kind::Model => EvidenceKind::Model,
    }
}

/// The hashes a run is about, from git and the tree alone: a run never needs
/// GitHub, so it never looks for a token.
fn property_hashes(ws: &Workspace, config: &Config, d: &Discovered, p: &LoadedProperty) -> Result<Hashes, CliError> {
    let bindings = d.bindings();
    let h = with_offline_backend(ws, Some(&bindings), |backend| hashes(ws, backend, p, config.waiver_max_days))?;
    if h.scope_hash.is_none() {
        return Err(CliError::Usage(format!(
            "{}: the scope matches no tracked file, so no run can be about anything (the property is Broken: scope matches nothing)",
            p.path
        )));
    }
    Ok(h)
}

/// A seed for a run nobody asked to replay: the clock and the process id.
fn seed_from_clock() -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    nanos ^ u64::from(std::process::id()).rotate_left(32)
}

fn head_sha(root: &Path) -> Option<String> {
    Git::open(root).head("HEAD").ok().map(|oid| hex::encode_oid(&oid))
}

// ---------------------------------------------------------------------------
// Outcomes to records
// ---------------------------------------------------------------------------

fn check_record(o: CheckOutcome) -> (OutcomeRecord, Option<Replay>) {
    match o {
        CheckOutcome::NoCounterexample { budget, replay } => (
            OutcomeRecord::NoCounterexample { budget: u64::from(budget.0) },
            Some(Replay { seed: replay.0.to_string(), cases: Some(u64::from(budget.0)) }),
        ),
        CheckOutcome::Counterexample { witness, replay } => (
            OutcomeRecord::Counterexample { witness: witness.render() },
            Some(Replay { seed: replay.0.to_string(), cases: None }),
        ),
        CheckOutcome::Verified { measure: ProofMeasure::Verus { rlimit } } => {
            (OutcomeRecord::Verified { rlimit: Some(rlimit), checks: None }, None)
        }
        CheckOutcome::Verified { measure: ProofMeasure::Kani { checks } } => {
            (OutcomeRecord::Verified { rlimit: None, checks: Some(checks) }, None)
        }
        CheckOutcome::ProofFailed { diagnostics } => (OutcomeRecord::ProofFailed { diagnostics }, None),
        CheckOutcome::Errored { reason } => (OutcomeRecord::Errored { reason }, None),
    }
}

fn score_record(o: ScoreOutcome) -> OutcomeRecord {
    match o {
        ScoreOutcome::Scored { killed, survived, timeout, unviable, unsampled } => OutcomeRecord::Scored {
            killed,
            survived,
            timeout,
            unviable,
            unsampled: (unsampled > 0).then_some(unsampled),
        },
        ScoreOutcome::NoMutants => OutcomeRecord::NoMutants,
        ScoreOutcome::Errored { reason } => OutcomeRecord::Errored { reason },
    }
}

fn exit_of(o: &OutcomeRecord) -> Exit {
    match o {
        OutcomeRecord::Counterexample { .. } | OutcomeRecord::ProofFailed { .. } | OutcomeRecord::Violation { .. } => {
            Exit::Block
        }
        OutcomeRecord::Errored { .. } => Exit::Usage,
        _ => Exit::Pass,
    }
}

// ---------------------------------------------------------------------------
// Recording and rendering
// ---------------------------------------------------------------------------

/// Appends the record as one line to `proob/.cache/results/results.jsonl`.
fn append(root: &Path, record: &RunResultRecord) -> Result<PathBuf, CliError> {
    let dir = root.join(RESULTS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| CliError::failed(format!("cannot create {}: {e}", dir.display())))?;
    let path = dir.join(RESULTS_FILE);
    let line = serde_json::to_string(record).map_err(|e| CliError::failed(format!("cannot encode the record: {e}")))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| CliError::failed(format!("cannot open {}: {e}", path.display())))?;
    writeln!(file, "{line}").map_err(|e| CliError::failed(format!("cannot write {}: {e}", path.display())))?;
    Ok(PathBuf::from(RESULTS_DIR).join(RESULTS_FILE))
}

fn print(out: &Outcome, json: bool) -> Result<(), CliError> {
    if json {
        let line = serde_json::to_string(&out.record)
            .map_err(|e| CliError::failed(format!("cannot encode the record: {e}")))?;
        println!("{line}");
        // The record stays the only stdout; the jobs line is for the reader.
        if let Some(jobs) = &out.jobs {
            eprintln!("jobs      {jobs}");
        }
    } else {
        print!("{}", text(out));
    }
    Ok(())
}

pub fn outcome_text(o: &OutcomeRecord, replay: Option<&Replay>) -> String {
    let seed = replay.map(|r| r.seed.as_str()).unwrap_or("none");
    match o {
        OutcomeRecord::NoCounterexample { budget } => {
            format!("no counterexample in {budget} cases; replay with seed {seed}")
        }
        OutcomeRecord::Counterexample { witness } => format!("counterexample: {witness}; replay with seed {seed}"),
        OutcomeRecord::Scored { killed, survived, timeout, unviable, unsampled } => {
            scored_text(*killed, *survived, *timeout, *unviable, unsampled.unwrap_or(0))
        }
        OutcomeRecord::NoMutants => "no mutants: nothing in the scope could be mutated".to_string(),
        OutcomeRecord::Errored { reason } => format!("errored: {reason}"),
        OutcomeRecord::Verified { rlimit: Some(rlimit), .. } => format!("verified (rlimit {rlimit})"),
        OutcomeRecord::Verified { checks: Some(checks), .. } => {
            format!("verified within the harness's bounds ({checks} checks)")
        }
        OutcomeRecord::Verified { .. } => "verified".to_string(),
        OutcomeRecord::ProofFailed { diagnostics } => format!("proof failed: {diagnostics}"),
        OutcomeRecord::NoViolation => "no violation".to_string(),
        OutcomeRecord::Violation { trace } => format!("violation: {trace}"),
    }
}

/// A score's line; a sampled one says it is a sample and of how many.
fn scored_text(killed: u32, survived: u32, timeout: u32, unviable: u32, unsampled: u32) -> String {
    let sampled = killed + survived + timeout + unviable;
    match unsampled {
        0 => format!(
            "scored: {killed} killed, {survived} survived, {timeout} timed out, {unviable} unviable (only the bound check ran)"
        ),
        n => format!(
            "scored: killed {killed} of {sampled} sampled ({} in scope); {survived} survived, {timeout} timed out, {unviable} unviable (only the bound check ran; {n} not run past max_mutants, so the counts and the kill floor are the sample's)",
            sampled.saturating_add(n)
        ),
    }
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

pub fn text(out: &Outcome) -> String {
    let r = &out.record;
    let tools: Vec<String> = r.tool_versions.iter().map(|(k, v)| format!("{k} {v}")).collect();
    let recorded = out.recorded.as_ref().map_or(String::new(), |p| format!("recorded  {}\n", p.display()));
    let jobs = out.jobs.as_ref().map_or(String::new(), |j| format!("jobs      {j}\n"));
    format!(
        "proob run {}: local run, unverified (not evidence; only an Action run on the default branch produces evidence)\n\
         property  {} ({})\n\
         check     {} (token at {})\n\
         outcome   {}\n\
         {jobs}\
         tools     {}\n\
         hashes    content {}… binding {}… scope {}… over {} file{}\n\
         {recorded}",
        r.kind.name(),
        out.property_path,
        out.title,
        out.check_ref,
        out.token,
        outcome_text(&r.outcome, r.replay.as_ref()),
        if tools.is_empty() { "none recorded".to_string() } else { tools.join(", ") },
        short(&r.content_hash),
        short(&r.binding_hash),
        short(&r.scope_hash),
        out.scope_files,
        if out.scope_files == 1 { "" } else { "s" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sampled_score_says_it_is_a_sample() {
        let sampled = OutcomeRecord::Scored { killed: 9, survived: 18, timeout: 2, unviable: 1, unsampled: Some(382) };
        assert!(
            outcome_text(&sampled, None).starts_with("scored: killed 9 of 30 sampled (412 in scope); 18 survived"),
            "{}",
            outcome_text(&sampled, None)
        );
        let all = OutcomeRecord::Scored { killed: 3, survived: 1, timeout: 0, unviable: 0, unsampled: None };
        assert_eq!(
            outcome_text(&all, None),
            "scored: 3 killed, 1 survived, 0 timed out, 0 unviable (only the bound check ran)"
        );
    }
}
