//! The execute job: runs the adapters on the checkout and leaves raw results
//! for the judge under `proob/.cache/results/`.
//!
//! It holds no token (`action::run` refuses to start it with one), reads
//! nothing from GitHub, and exits 0 whatever the results say: a
//! counterexample here is a fact for the judge, not a failure of this job.
//! What runs is a pure function of the mode and the event: in check mode,
//! `check` for every bound property the pull request affects, computed with
//! the same verified `affected` the judge will use; in evidence mode on a
//! push, `check` for every bound property; on a schedule or a dispatch,
//! `check` with the large budget and `score` for every bound property, of
//! every kind the registry knows. Each run is `proob run <what> <id>
//! --record` in process. `manifest.json` names the run, the attempt, the
//! commit, and what was attempted, so the judge can tell this run's results
//! from any other's.
use super::env::{ActionEnv, Event, Level, annotation, append, set_outputs};
use super::{Mode, has_declaration, nothing_headline, pull_request, require_at, resolve_base};
use crate::cli::adapters::{Discovered, discover};
use crate::cli::error::CliError;
use crate::cli::pipeline::affected_paths;
use crate::cli::run::{Options, Outcome, What, outcome_text, perform};
use crate::cli::workspace::{LoadedProperty, Workspace};
use crate::cli::{Common, Exit, base_config};
use proob_effects::git::{Git, Repo};
use proob_facts::config::Config;
use proob_facts::paths::RESULTS_DIR;
use proob_facts::results::{
    Attempt, Attempted, MANIFEST_FILE, Manifest, ManifestEvent, ManifestMode, RESULTS_FILE, Skipped,
};
use serde::Serialize;
use std::fmt::Write as _;
use std::path::Path;

/// One run to make.
struct Task<'a> {
    property: &'a LoadedProperty,
    what: What,
    budget: Option<u32>,
}

/// What the job will run, and the facts that explain why it is not more.
struct Plan<'a> {
    tasks: Vec<Task<'a>>,
    notes: Vec<String>,
    pull_request: Option<u64>,
    base_sha: Option<String>,
}

impl Plan<'_> {
    fn nothing(note: String) -> Plan<'static> {
        Plan { tasks: Vec::new(), notes: vec![note], pull_request: None, base_sha: None }
    }
}

pub fn run(common: &Common, env: &ActionEnv, mode: Mode) -> Result<Exit, CliError> {
    let ws = Workspace::open(common.repo.as_deref())?;
    let git = Git::open(&ws.root);
    require_at(&git, &env.sha)?;
    // Decided before the results directory exists: creating it makes `proob/`.
    let declared = has_declaration(&ws);
    fresh_results_dir(&ws.root)?;
    let manifest = match declared {
        false => manifest_of(env, mode, &ws, &Plan::nothing(nothing_headline(mode, &env.sha)), Vec::new(), Vec::new()),
        true => execute(env, mode, &ws, &git)?,
    };
    write_manifest(&ws.root, &manifest)?;
    report(common, env, mode, &manifest)?;
    Ok(Exit::Pass)
}

fn execute(env: &ActionEnv, mode: Mode, ws: &Workspace, git: &Git) -> Result<Manifest, CliError> {
    let properties = ws.load_properties()?;
    let head_config = ws.load_config()?.candidate();
    let discovered = discover(ws, &head_config)?;
    let plan = match mode {
        Mode::Check => plan_check(env, ws, git, &properties, discovered.as_ref())?,
        Mode::Evidence => plan_evidence(env, &head_config, &properties, discovered.as_ref())?,
    };
    let runs: Vec<Result<Attempt, Skipped>> = plan
        .tasks
        .iter()
        .map(|t| discovered.as_ref().map_or_else(|| Err(no_adapter(t)), |d| attempt(ws, &head_config, d, t)))
        .collect();
    let attempted = runs.iter().filter_map(|r| r.as_ref().ok().cloned()).collect();
    let skipped = runs.iter().filter_map(|r| r.as_ref().err().cloned()).collect();
    Ok(manifest_of(env, mode, ws, &plan, attempted, skipped))
}

// ---------------------------------------------------------------------------
// Plans
// ---------------------------------------------------------------------------

fn is_bound(d: Option<&Discovered>, p: &LoadedProperty) -> bool {
    d.is_some_and(|d| d.by_id.contains_key(&p.id))
}

const NO_ADAPTER: &str = "proob/config.toml lists no adapter this proob has; nothing can run (every property renders Not checked (no adapter))";

/// Check mode: `check` for the bound properties this pull request affects,
/// decided exactly as the judge decides it (changed paths against resolved
/// scopes and binding sets, under the base ref's config). A base without a
/// config is a bootstrap pull request: the gate is not in force and nothing runs.
fn plan_check<'a>(
    env: &ActionEnv,
    ws: &Workspace,
    git: &Git,
    properties: &'a [LoadedProperty],
    d: Option<&Discovered>,
) -> Result<Plan<'a>, CliError> {
    let pr = pull_request(env)?;
    let base = resolve_base(git, pr)?;
    let base_sha = git.head(&base).map(|oid| proob_facts::hex::encode_oid(&oid)).map_err(CliError::failed)?;
    let (pull_request, base_sha) = (Some(pr.number), Some(base_sha));
    let Some(governing) = base_config(git, &base)? else {
        let note = format!(
            "bootstrap: {} has no proob/config.toml, so the gate is not in force yet and nothing runs",
            pr.base_ref
        );
        return Ok(Plan { tasks: Vec::new(), notes: vec![note], pull_request, base_sha });
    };
    let Some(d) = d else {
        return Ok(Plan { tasks: Vec::new(), notes: vec![NO_ADAPTER.into()], pull_request, base_sha });
    };
    let changed = git.changed_paths(&base, &env.sha).map_err(CliError::failed)?;
    let bindings = d.bindings();
    let affected = affected_paths(git, Some(&bindings), properties, &changed, ws.now, governing.waiver_max_days)?;
    let affected_props: Vec<&LoadedProperty> =
        properties.iter().filter(|p| affected.get(&p.path) == Some(&true)).collect();
    let tasks: Vec<Task<'a>> = affected_props
        .iter()
        .filter(|p| is_bound(Some(d), p))
        .map(|p| Task { property: p, what: What::Check, budget: None })
        .collect();
    let notes = vec![format!(
        "{} changed path{} {} {} of {} propert{}; {} of those bind a check",
        changed.len(),
        if changed.len() == 1 { "" } else { "s" },
        if changed.len() == 1 { "affects" } else { "affect" },
        affected_props.len(),
        properties.len(),
        if properties.len() == 1 { "y" } else { "ies" },
        tasks.len()
    )];
    Ok(Plan { tasks, notes, pull_request, base_sha })
}

/// Evidence mode: on a push, `check` for every bound property; on a schedule
/// or a dispatch, `check` with the large budget and `score` for every bound
/// property, whatever its kind (the registry decides which adapter runs it).
fn plan_evidence<'a>(
    env: &ActionEnv,
    config: &Config,
    properties: &'a [LoadedProperty],
    d: Option<&Discovered>,
) -> Result<Plan<'a>, CliError> {
    if d.is_none() {
        return Ok(Plan::nothing(NO_ADAPTER.into()));
    }
    let bound: Vec<&'a LoadedProperty> = properties.iter().filter(|p| is_bound(d, p)).collect();
    let tasks: Vec<Task<'a>> = match env.event {
        Event::Push => bound.iter().map(|p| Task { property: p, what: What::Check, budget: None }).collect(),
        Event::Schedule | Event::WorkflowDispatch => bound
            .iter()
            .flat_map(|p| {
                [
                    Task { property: p, what: What::Check, budget: Some(config.scheduled_cases) },
                    Task { property: p, what: What::Score, budget: None },
                ]
            })
            .collect(),
        Event::PullRequest(_) => {
            return Err(CliError::Usage("evidence mode does not run on pull_request events".into()));
        }
    };
    let what = match env.event {
        Event::Push => "every bound check at the pushed commit".to_string(),
        _ => format!("every bound check with {} cases and every score", config.scheduled_cases),
    };
    let notes = vec![format!(
        "{}: {} of {} properties bind a check; running {what}",
        env.event.name(),
        bound.len(),
        properties.len()
    )];
    Ok(Plan { tasks, notes, pull_request: None, base_sha: None })
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

fn attempted(what: What) -> Attempted {
    match what {
        What::Check => Attempted::Check,
        What::Score => Attempted::Score,
    }
}

fn no_adapter(t: &Task<'_>) -> Skipped {
    Skipped { id: t.property.file.id.clone(), what: attempted(t.what), reason: "no adapter".into() }
}

/// One task through `run::perform`, recorded; a run the CLI could not even
/// start is a skip with its reason, never a failure of the job.
fn attempt(ws: &Workspace, config: &Config, d: &Discovered, t: &Task<'_>) -> Result<Attempt, Skipped> {
    let opts = Options { budget: t.budget, seed: None, record: true, json: false };
    let id = t.property.file.id.clone();
    match perform(ws, config, d, t.property, t.what, opts) {
        Ok(out) => {
            println!(
                "proob execute: {} {id}: {}",
                what_name(t.what),
                outcome_text(&out.record.outcome, out.record.replay.as_ref())
            );
            Ok(Attempt { id, what: attempted(t.what), outcome: outcome_name(&out) })
        }
        Err(e) => {
            println!("proob execute: {} {id}: skipped: {e}", what_name(t.what));
            Err(Skipped { id, what: attempted(t.what), reason: e.to_string() })
        }
    }
}

fn what_name(w: What) -> &'static str {
    match w {
        What::Check => "check",
        What::Score => "score",
    }
}

fn outcome_name(out: &Outcome) -> String {
    out.record.outcome.name().to_string()
}

// ---------------------------------------------------------------------------
// The manifest
// ---------------------------------------------------------------------------

fn manifest_event(e: &Event) -> ManifestEvent {
    match e {
        Event::PullRequest(_) => ManifestEvent::PullRequest,
        Event::Push => ManifestEvent::Push,
        Event::Schedule => ManifestEvent::Schedule,
        Event::WorkflowDispatch => ManifestEvent::WorkflowDispatch,
    }
}

fn manifest_of(
    env: &ActionEnv,
    mode: Mode,
    ws: &Workspace,
    plan: &Plan<'_>,
    attempted: Vec<Attempt>,
    skipped: Vec<Skipped>,
) -> Manifest {
    Manifest {
        schema_version: 1,
        mode: match mode {
            Mode::Check => ManifestMode::Check,
            Mode::Evidence => ManifestMode::Evidence,
        },
        event: manifest_event(&env.event),
        repository: env.repository.clone(),
        run_id: env.run_id,
        run_attempt: env.run_attempt,
        head_sha: env.sha.clone(),
        pull_request: plan.pull_request,
        base_sha: plan.base_sha.clone(),
        created_at: ws.now,
        attempted,
        skipped,
        notes: plan.notes.clone(),
    }
}

/// An empty results directory: this run's results and nothing else.
fn fresh_results_dir(root: &Path) -> Result<(), CliError> {
    let dir = root.join(RESULTS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| CliError::failed(format!("cannot create {}: {e}", dir.display())))?;
    for file in [RESULTS_FILE, MANIFEST_FILE] {
        let path = dir.join(file);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(CliError::failed(format!("cannot clear {}: {e}", path.display()))),
        }
    }
    Ok(())
}

fn write_manifest(root: &Path, manifest: &Manifest) -> Result<(), CliError> {
    let path = root.join(RESULTS_DIR).join(MANIFEST_FILE);
    let text = serde_json::to_string_pretty(manifest)
        .map_err(|e| CliError::failed(format!("cannot encode the manifest: {e}")))?;
    std::fs::write(&path, text + "\n").map_err(|e| CliError::failed(format!("cannot write {}: {e}", path.display())))
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

fn headline(mode: Mode, m: &Manifest) -> String {
    format!("proob execute ({}): {} run, {} skipped", mode.name(), m.attempted.len(), m.skipped.len())
}

fn summary_markdown(mode: Mode, m: &Manifest) -> String {
    let mut out = format!("## {}\n\n", headline(mode, m));
    if !m.attempted.is_empty() {
        out.push_str("| Property | Ran | Outcome |\n|---|---|---|\n");
        for a in &m.attempted {
            let _ = writeln!(out, "| `{}` | {} | {} |", a.id, attempted_name(a.what), a.outcome);
        }
        out.push('\n');
    }
    for s in &m.skipped {
        let _ = writeln!(out, "- skipped: `{}` {}: {}", s.id, attempted_name(s.what), s.reason);
    }
    for note in &m.notes {
        let _ = writeln!(out, "- note: {note}");
    }
    out.push_str("\nRaw results only: the judge job turns them into verdicts.\n");
    out
}

fn attempted_name(a: Attempted) -> &'static str {
    match a {
        Attempted::Check => "check",
        Attempted::Score => "score",
    }
}

#[derive(Serialize)]
struct ExecuteJ<'a> {
    schema_version: u32,
    job: &'static str,
    headline: String,
    manifest: &'a Manifest,
    exit: u8,
}

fn report(common: &Common, env: &ActionEnv, mode: Mode, m: &Manifest) -> Result<(), CliError> {
    let line = headline(mode, m);
    append(&env.step_summary, &summary_markdown(mode, m))?;
    set_outputs(
        &env.output,
        &[
            ("job", "execute".to_string()),
            ("mode", mode.name().to_string()),
            ("attempted", m.attempted.len().to_string()),
            ("skipped", m.skipped.len().to_string()),
            ("headline", line.clone()),
        ],
    )?;
    if common.json {
        let body = ExecuteJ { schema_version: 1, job: "execute", headline: line, manifest: m, exit: 0 };
        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
    } else {
        println!("{line}");
        for note in &m.notes {
            println!("note: {note}");
        }
        println!("{}", annotation(Level::Notice, &[("title", "proob")], &line));
    }
    Ok(())
}
