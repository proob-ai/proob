//! The judge job: the read-only token, no repository code.
//!
//! Its inputs are the checkout, git history, GitHub's records (through the
//! same pipelines `check` and `status` use), and the raw results the execute
//! job uploaded. Those results count only when their `manifest.json` names
//! this run (run id and commit); anything else is ignored with a note, so an
//! artifact from another run, or a hand-made one, can never reach `derive`.
//! No adapter is called here: discovery is a source scan, and nothing in
//! this module starts a process other than `git`.
//!
//! Check mode renders the gate: Markdown to the step summary, a `::notice::`
//! with the headline, an `::error::` per blocking row naming the property
//! file, outputs for later steps, and exit 1 exactly when an affected
//! property is Broken (0 for a bootstrap pull request). Evidence mode
//! repackages this run's records as evidence records carrying the run's
//! provenance, writes them under `proob/.cache/evidence/` for the workflow to
//! upload, renders the status of the default branch, and exits 0, noting any
//! negative outcome.
use super::env::{ActionEnv, Level, annotation, append, set_outputs};
use super::{Mode, has_declaration, nothing_headline, pull_request, require_at, resolve_base};
use crate::cli::error::CliError;
use crate::cli::json::report_json;
use crate::cli::render::{markdown, summary_line};
use crate::cli::report::{Mode as ReportMode, Report, Row};
use crate::cli::vocab;
use crate::cli::workspace::Workspace;
use crate::cli::{Common, Exit, check_report, load, status_report};
use proob_core::types::PropertyId;
use proob_effects::git::{Git, Repo};
use proob_facts::evidence::{EvidenceRecord, OutcomeRecord, RECORDS_FILE};
use proob_facts::hex;
use proob_facts::paths::{EVIDENCE_DIR, EVIDENCE_WORKFLOW, RESULTS_DIR};
use proob_facts::results::{MANIFEST_FILE, Manifest, RESULTS_FILE, RunResultRecord, parse_manifest, parse_results};
use proob_facts::ulid;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

pub fn run(common: &Common, env: &ActionEnv, mode: Mode) -> Result<Exit, CliError> {
    let ws = Workspace::open(common.repo.as_deref())?;
    let git = Git::open(&ws.root);
    require_at(&git, &env.sha)?;
    let results = load_results(&ws.root, env)?;
    match mode {
        Mode::Check => judge_check(common, env, &ws, &git, results),
        Mode::Evidence => judge_evidence(common, env, &ws, &git, results),
    }
}

// ---------------------------------------------------------------------------
// The raw results artifact
// ---------------------------------------------------------------------------

/// This run's own results, in file order, with notes about lines left out.
struct Own {
    manifest: Manifest,
    records: Vec<RunResultRecord>,
    notes: Vec<String>,
}

/// What the results directory held, judged against this run.
enum Results {
    Accepted(Box<Own>),
    /// Nothing was downloaded.
    Missing,
    /// Results that are not this run's: ignored, with the reason.
    Rejected {
        reason: String,
    },
}

fn read_records(path: &Path) -> Result<Vec<RunResultRecord>, CliError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(CliError::failed(format!("cannot read {}: {e}", path.display()))),
    };
    let label = path.display().to_string();
    parse_results(&text)
        .map(|(line, r)| r.map_err(|e| CliError::invalid(&label, format!("line {line}: {e}"))))
        .collect()
}

fn load_results(root: &Path, env: &ActionEnv) -> Result<Results, CliError> {
    let dir = root.join(RESULTS_DIR);
    let (manifest_path, results_path) = (dir.join(MANIFEST_FILE), dir.join(RESULTS_FILE));
    let text = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(if results_path.is_file() {
                Results::Rejected { reason: "raw results without a manifest.json were ignored; only the execute job of this run can supply results".into() }
            } else {
                Results::Missing
            });
        }
        Err(e) => return Err(CliError::failed(format!("cannot read {}: {e}", manifest_path.display()))),
    };
    let manifest = match parse_manifest(&text) {
        Ok(m) => m,
        Err(e) => {
            return Ok(Results::Rejected {
                reason: format!("manifest.json is not readable ({e}); raw results were ignored"),
            });
        }
    };
    if manifest.run_id != env.run_id || manifest.head_sha != env.sha {
        return Ok(Results::Rejected {
            reason: format!(
                "raw results from another run were ignored (the manifest names run {} at {}; this is run {} at {})",
                manifest.run_id,
                short(&manifest.head_sha),
                env.run_id,
                short(&env.sha)
            ),
        });
    }
    let (own, foreign): (Vec<RunResultRecord>, Vec<RunResultRecord>) =
        read_records(&results_path)?.into_iter().partition(|r| r.head_sha.as_deref().is_none_or(|h| h == env.sha));
    let notes = foreign.iter().map(|r| format!("result for {} names another commit and was ignored", r.id)).collect();
    Ok(Results::Accepted(Box::new(Own { manifest, records: own, notes })))
}

fn short(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

/// Records by property, the last line per id winning, as `check --results` reads them.
fn by_id(records: Vec<RunResultRecord>) -> Result<BTreeMap<PropertyId, RunResultRecord>, CliError> {
    records
        .into_iter()
        .map(|r| ulid::decode(&r.id).map(|id| (id, r)).map_err(|e| CliError::invalid(RESULTS_FILE, format!("id: {e}"))))
        .collect()
}

// ---------------------------------------------------------------------------
// Check mode
// ---------------------------------------------------------------------------

fn judge_check(
    common: &Common,
    env: &ActionEnv,
    ws: &Workspace,
    git: &Git,
    results: Results,
) -> Result<Exit, CliError> {
    let pr = pull_request(env)?;
    if !has_declaration(ws) {
        return nothing(common, env, Mode::Check);
    }
    let loaded = load(common)?;
    let base = resolve_base(git, pr)?;
    let (records, notes) = match results {
        Results::Accepted(own) => (by_id(own.records)?, own.notes),
        Results::Missing => (
            BTreeMap::new(),
            vec!["no raw results under proob/.cache/results (the execute job uploads them as proob-results); this PR's own checks show as not run".into()],
        ),
        Results::Rejected { reason } => (BTreeMap::new(), vec![reason]),
    };
    let mut report = check_report(common, &loaded, &base, &env.sha, &records, Some(pr.number))?;
    report.notes.extend(notes);
    let exit = if report.blocks() { Exit::Block } else { Exit::Pass };
    emit(common, env, &report, exit, &[])?;
    Ok(exit)
}

// ---------------------------------------------------------------------------
// Evidence mode
// ---------------------------------------------------------------------------

/// What every record of this run carries: the run, the commit, the workflow
/// blob that executed, and when.
struct Provenance {
    run_id: u64,
    head_sha: String,
    workflow_blob: String,
    at: u64,
}

/// A record repackaged with this run's provenance; a record whose hashes or
/// id would not decode is left out, so one bad line cannot make the whole
/// artifact unreadable for every later verdict.
fn repackage(r: &RunResultRecord, p: &Provenance) -> Result<EvidenceRecord, String> {
    ulid::decode(&r.id).map_err(|e| format!("{}: id: {e}", r.id))?;
    for (name, hash) in
        [("content_hash", &r.content_hash), ("binding_hash", &r.binding_hash), ("scope_hash", &r.scope_hash)]
    {
        hex::decode_digest(hash).map_err(|e| format!("{}: {name}: {e}", r.id))?;
    }
    Ok(EvidenceRecord {
        schema_version: 1,
        id: r.id.clone(),
        kind: r.kind,
        algo: r.algo,
        content_hash: r.content_hash.clone(),
        binding_hash: r.binding_hash.clone(),
        scope_hash: r.scope_hash.clone(),
        spec_hash: None,
        outcome: r.outcome.clone(),
        replay: r.replay.clone(),
        tool_versions: r.tool_versions.clone(),
        run_id: p.run_id,
        head_sha: p.head_sha.clone(),
        workflow_blob: p.workflow_blob.clone(),
        at: p.at,
    })
}

fn is_negative(o: &OutcomeRecord) -> bool {
    matches!(
        o,
        OutcomeRecord::Counterexample { .. }
            | OutcomeRecord::ProofFailed { .. }
            | OutcomeRecord::Violation { .. }
            | OutcomeRecord::Errored { .. }
    )
}

fn write_evidence(root: &Path, records: &[EvidenceRecord]) -> Result<(), CliError> {
    let dir = root.join(EVIDENCE_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| CliError::failed(format!("cannot create {}: {e}", dir.display())))?;
    let lines = records
        .iter()
        .map(|r| serde_json::to_string(r).map(|l| l + "\n"))
        .collect::<Result<String, _>>()
        .map_err(|e| CliError::failed(format!("cannot encode an evidence record: {e}")))?;
    let path = dir.join(RECORDS_FILE);
    std::fs::write(&path, lines).map_err(|e| CliError::failed(format!("cannot write {}: {e}", path.display())))
}

struct Packaged {
    records: Vec<EvidenceRecord>,
    negative: Vec<String>,
    notes: Vec<String>,
}

fn package(
    env: &ActionEnv,
    ws: &Workspace,
    git: &Git,
    manifest: &Manifest,
    records: &[RunResultRecord],
    mut notes: Vec<String>,
) -> Result<Packaged, CliError> {
    let blob = git.blob_at(&env.sha, &env.workflow.path).map_err(CliError::failed)?.ok_or_else(|| {
        CliError::Usage(format!(
            "the workflow file {} is not in the tree at {}; evidence needs the workflow that produced it",
            env.workflow.path,
            short(&env.sha)
        ))
    })?;
    let provenance =
        Provenance { run_id: env.run_id, head_sha: env.sha.clone(), workflow_blob: hex::encode_oid(&blob), at: ws.now };
    let (ok, bad): (Vec<_>, Vec<_>) = records.iter().map(|r| repackage(r, &provenance)).partition(Result::is_ok);
    let records: Vec<EvidenceRecord> = ok.into_iter().filter_map(Result::ok).collect();
    notes.extend(bad.into_iter().filter_map(Result::err).map(|e| format!("record left out: {e}")));
    notes.extend(manifest.skipped.iter().map(|s| format!("the execute job skipped `{}`: {}", s.id, s.reason)));
    notes.extend(manifest.notes.iter().cloned());
    if env.workflow.path != EVIDENCE_WORKFLOW {
        notes.push(format!(
            "this workflow runs from {}, but verdicts read evidence only from {EVIDENCE_WORKFLOW}; move the file there for these records to count",
            env.workflow.path
        ));
    }
    let negative = records
        .iter()
        .filter(|r| is_negative(&r.outcome))
        .map(|r| format!("negative outcome on the default branch: `{}` {} {}", r.id, r.kind.name(), r.outcome.name()))
        .collect();
    Ok(Packaged { records, negative, notes })
}

fn packaging_headline(env: &ActionEnv, p: &Packaged) -> String {
    format!(
        "proob evidence: {} record{} packaged for run {} at {} ({}){}",
        p.records.len(),
        vocab::plural(p.records.len() as u64),
        env.run_id,
        short(&env.sha),
        env.event.name(),
        if p.negative.is_empty() { String::new() } else { format!("; {} negative", p.negative.len()) }
    )
}

fn packaging_markdown(env: &ActionEnv, p: &Packaged) -> String {
    let mut out = format!("## {}\n\n", packaging_headline(env, p));
    if !p.records.is_empty() {
        out.push_str("| Property | Kind | Outcome |\n|---|---|---|\n");
        for r in &p.records {
            let _ = writeln!(out, "| `{}` | {} | {} |", r.id, r.kind.name(), r.outcome.name());
        }
        out.push('\n');
    }
    for line in p.negative.iter().chain(&p.notes) {
        let _ = writeln!(out, "- note: {line}");
    }
    out.push_str(&format!("\nUploaded as `proob-evidence`; a verdict reads it only from a successful run of the approved {EVIDENCE_WORKFLOW} on the default branch.\n\n"));
    out
}

fn judge_evidence(
    common: &Common,
    env: &ActionEnv,
    ws: &Workspace,
    git: &Git,
    results: Results,
) -> Result<Exit, CliError> {
    let own = match results {
        Results::Accepted(own) => *own,
        Results::Missing => {
            return Err(CliError::Failed(
                "no raw results under proob/.cache/results: the execute job's upload of proob-results, or this job's download, did not happen; no evidence is produced".into(),
            ));
        }
        Results::Rejected { reason } => return Err(CliError::Failed(format!("{reason}; no evidence is produced"))),
    };
    if !has_declaration(ws) {
        // Nothing declared, nothing to package: an empty records file, so the
        // workflow's upload step has something to upload.
        write_evidence(&ws.root, &[])?;
        return nothing_with(
            common,
            env,
            Mode::Evidence,
            &[("records", "0".to_string()), ("negative", "0".to_string())],
        );
    }
    let packaged = package(env, ws, git, &own.manifest, &own.records, own.notes)?;
    write_evidence(&ws.root, &packaged.records)?;
    let packaging = packaging_markdown(env, &packaged);
    append(&env.step_summary, &packaging)?;
    let extra = [("records", packaged.records.len().to_string()), ("negative", packaged.negative.len().to_string())];
    if !common.json {
        print!("{packaging}");
        for line in &packaged.negative {
            println!("{}", annotation(Level::Warning, &[("title", "proob evidence")], line));
        }
    }
    let loaded = load(common)?;
    let report = status_report(&loaded, common, None)?;
    emit(common, env, &report, Exit::Pass, &extra)?;
    Ok(Exit::Pass)
}

// ---------------------------------------------------------------------------
// Rendering a report to the job
// ---------------------------------------------------------------------------

fn gate_word(r: &Report) -> &'static str {
    if r.blocks() {
        "block"
    } else if r.warns() {
        "warn"
    } else {
        "pass"
    }
}

fn mode_word(r: &Report) -> &'static str {
    match r.mode {
        ReportMode::Status => "status",
        ReportMode::Check { .. } => "check",
        ReportMode::Bootstrap { .. } => "bootstrap",
    }
}

/// The `::error::` for one blocking row: the property file and the words of the table.
fn error_annotation(row: &Row) -> String {
    let message = format!(
        "{}: {} ({}); this PR: {}",
        row.title,
        vocab::row_word(&row.verdict),
        vocab::row_reason(&row.verdict, Some(row.kind)),
        vocab::this_pr(row.this_pr.as_ref())
    );
    annotation(Level::Error, &[("file", &row.path), ("title", "proob gate")], &message)
}

/// The step outputs: the gate word, the counts, the headline, and whatever
/// the mode adds (`extra`).
fn outputs_of<'a>(r: &Report, headline: &str, exit: Exit, extra: &'a [(&'a str, String)]) -> Vec<(&'a str, String)> {
    let c = r.counts();
    let base: [(&'a str, String); 14] = [
        ("job", "judge".to_string()),
        ("mode", mode_word(r).to_string()),
        ("verdict", gate_word(r).to_string()),
        ("blocked", (exit == Exit::Block).to_string()),
        ("affected", r.affected_rows().count().to_string()),
        ("holds", c.holds.to_string()),
        ("checked", c.checked.to_string()),
        ("not_checked", c.not_checked.to_string()),
        ("re_approve", c.re_approve.to_string()),
        ("broken", c.broken.to_string()),
        ("proposed", c.proposed.to_string()),
        ("waived", c.waived.to_string()),
        ("unverified", c.unverified.to_string()),
        ("headline", headline.to_string()),
    ];
    base.into_iter().chain(extra.iter().map(|(k, v)| (*k, v.clone()))).collect()
}

/// Markdown to the summary, the report (JSON or Markdown with annotations)
/// to stdout, outputs for later steps.
fn emit(
    common: &Common,
    env: &ActionEnv,
    report: &Report,
    exit: Exit,
    extra: &[(&str, String)],
) -> Result<(), CliError> {
    let md = markdown(report);
    let headline = summary_line(report);
    append(&env.step_summary, &md)?;
    let pairs = outputs_of(report, &headline, exit, extra);
    set_outputs(&env.output, &pairs)?;
    if common.json {
        let body = report_json(report, headline, exit.code());
        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
        return Ok(());
    }
    print!("{md}");
    println!("{}", annotation(Level::Notice, &[("title", "proob")], &headline));
    for row in report.rows.iter().filter(|r| r.blocks()) {
        println!("{}", error_annotation(row));
    }
    Ok(())
}

#[derive(Serialize)]
struct NothingJ {
    schema_version: u32,
    mode: &'static str,
    headline: String,
    exit: u8,
}

/// No `proob/` directory: nothing is governed, the job passes and says so.
fn nothing(common: &Common, env: &ActionEnv, mode: Mode) -> Result<Exit, CliError> {
    nothing_with(common, env, mode, &[])
}

fn nothing_with(common: &Common, env: &ActionEnv, mode: Mode, extra: &[(&str, String)]) -> Result<Exit, CliError> {
    let headline = nothing_headline(mode, &env.sha);
    append(&env.step_summary, &format!("## {headline}\n\n"))?;
    let base: [(&str, String); 5] = [
        ("job", "judge".to_string()),
        ("mode", "none".to_string()),
        ("verdict", "pass".to_string()),
        ("blocked", "false".to_string()),
        ("headline", headline.clone()),
    ];
    let pairs: Vec<(&str, String)> = base.into_iter().chain(extra.iter().map(|(k, v)| (*k, v.clone()))).collect();
    set_outputs(&env.output, &pairs)?;
    if common.json {
        let body = NothingJ { schema_version: 1, mode: "none", headline, exit: 0 };
        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
    } else {
        println!("{headline}");
        println!("{}", annotation(Level::Notice, &[("title", "proob")], &headline));
    }
    Ok(Exit::Pass)
}
