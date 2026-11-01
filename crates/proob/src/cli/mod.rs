//! The command line: subcommands, exit codes (0 pass, 1 block, 2 usage or
//! internal error), and the glue from arguments to the pipeline and the
//! renderers. Every subcommand takes `--json`.
pub mod action;
pub mod adapters;
pub mod backend;
pub mod doctor;
pub mod error;
pub mod hash;
pub mod init;
pub mod json;
pub mod pipeline;
pub mod render;
pub mod report;
pub mod run;
pub mod shadow;
pub mod uncovered;
pub mod validate;
pub mod vocab;
pub mod workspace;

use crate::cli::backend::{Backend, with_backend};
use crate::cli::error::CliError;
use crate::cli::pipeline::{CheckInputs, Inputs};
use crate::cli::report::Report;
use crate::cli::workspace::{ConfigSource, LoadedProperty, Workspace, parse_config_text};
use clap::{Args, Parser, Subcommand};
use proob_core::types::PropertyId;
use proob_effects::git::Repo;
use proob_facts::config::Config;
use proob_facts::date::SECS_PER_DAY;
use proob_facts::paths::CONFIG;
use proob_facts::results::{RunResultRecord, parse_results};
use proob_facts::ulid;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

/// Team-approved, machine-checked, continuously guarded properties.
#[derive(Parser)]
#[command(name = "proob", version, about, long_about = None)]
pub struct Cli {
    #[command(flatten)]
    common: Common,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Clone, Debug)]
pub struct Common {
    /// The repository root (default: the current directory).
    #[arg(long, global = true)]
    pub repo: Option<PathBuf>,
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    /// The default branch on GitHub (default: origin/HEAD, else main or master).
    #[arg(long, global = true)]
    pub default_branch: Option<String>,
    /// Run against this repository's shadow (`proob shadow open`) instead of the repository.
    #[arg(long, global = true)]
    pub shadow: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Check proob's files: schemas, binding tokens, an untracked cache, no secrets.
    Validate,
    /// Print a property's content, binding, and scope hashes for the current tree.
    Hash {
        /// The property's ULID.
        id: String,
    },
    /// Derive the status of every property from GitHub's records and the tree.
    Status {
        /// The default-branch revision to derive against (default: origin/<branch>).
        #[arg(long)]
        base: Option<String>,
        /// Print exactly one line (the session hook's message).
        #[arg(long, conflicts_with = "json")]
        summary: bool,
    },
    /// Status plus the gate policy over the properties this change affects.
    Check {
        /// The base revision (the default branch as the PR will merge into it).
        #[arg(long)]
        base: String,
        /// The head revision; the working tree must be checked out at it.
        #[arg(long)]
        head: String,
        /// `results.jsonl` from the execute job: this PR's own run.
        #[arg(long)]
        results: Option<PathBuf>,
        /// The pull request number, for the "if merged now" rows.
        #[arg(long)]
        pr: Option<u64>,
    },
    /// Run a property's bound check or its strength score through its adapter (local, unverified).
    Run {
        #[arg(value_enum)]
        what: run::What,
        /// The property's ULID.
        id: String,
        /// Cases for the property-based test (check only; default: the adapter's, 256).
        #[arg(long)]
        budget: Option<u32>,
        /// RNG seed to replay (check only; default: fresh).
        #[arg(long)]
        seed: Option<u64>,
        /// Append the record to proob/.cache/results/results.jsonl.
        #[arg(long)]
        record: bool,
    },
    /// Write proob/config.toml, the two workflows, and the plugin recommendation.
    Init {
        /// Also set up Kani: its adapter, a pinned and cached install in both workflows, and the build probe.
        #[arg(long)]
        kani: bool,
        /// Also set up the TLA+ adapter (TLC); TLC itself is not installed.
        #[arg(long)]
        tla: bool,
    },
    /// Whether this machine has the tools each configured adapter runs, and the room to run them; installs nothing.
    Doctor,
    /// Try proob in a private copy of this repository; nothing in the repository changes until you adopt.
    Shadow {
        #[command(subcommand)]
        cmd: shadow::ShadowCmd,
    },
    /// Claude Code hooks (JSON on stdin, JSON on stdout, never non-zero).
    Hook {
        #[command(subcommand)]
        event: HookEvent,
    },
    /// The GitHub Action's jobs: reads the Actions environment, nothing else.
    Action {
        /// `check` for the pull-request gate, `evidence` for the default-branch run.
        #[arg(long, value_enum)]
        mode: action::Mode,
        /// `execute` runs the adapters and holds no token; `judge` derives verdicts and runs no repository code.
        #[arg(long, value_enum)]
        job: action::Job,
    },
}

#[derive(Subcommand)]
enum HookEvent {
    /// The one-line status at session start.
    SessionStart,
    /// Deny a schema-invalid Write or Edit under proob/ (fail-fast feedback, never the boundary).
    PreToolUse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Pass,
    Block,
    Usage,
}

impl Exit {
    pub fn code(self) -> u8 {
        match self {
            Exit::Pass => 0,
            Exit::Block => 1,
            Exit::Usage => 2,
        }
    }
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Hook { event: HookEvent::SessionStart } => {
            return crate::hook::session_start::run(&mut std::io::stdin().lock(), cli.common.repo.as_deref());
        }
        Command::Hook { event: HookEvent::PreToolUse } => {
            return crate::hook::pre_tool_use::run(&mut std::io::stdin().lock());
        }
        _ => {}
    }
    match dispatch(&cli.common, cli.command) {
        Ok(exit) => ExitCode::from(exit.code()),
        Err(e) => {
            report_error(&e, cli.common.json);
            ExitCode::from(Exit::Usage.code())
        }
    }
}

#[derive(Serialize)]
struct ErrorJ<'a> {
    error: String,
    exit: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    problems: Option<&'a [validate::Problem]>,
}

fn report_error(e: &CliError, json: bool) {
    if json {
        let body = ErrorJ { error: e.to_string(), exit: Exit::Usage.code(), problems: None };
        println!("{}", serde_json::to_string(&body).unwrap_or_default());
    }
    match e {
        CliError::Shadow(_) => eprintln!("{e}"),
        _ => eprintln!("proob: {e}"),
    }
}

fn dispatch(common: &Common, command: Command) -> Result<Exit, CliError> {
    let common = &shadow::redirect(common)?;
    match command {
        Command::Validate => cmd_validate(common),
        Command::Hash { id } => cmd_hash(common, &id),
        Command::Status { base, summary } => cmd_status(common, base.as_deref(), summary),
        Command::Check { base, head, results, pr } => cmd_check(common, &base, &head, results.as_deref(), pr),
        Command::Run { what, id, budget, seed, record } => {
            cmd_run(common, what, &id, run::Options { budget, seed, record, json: common.json })
        }
        Command::Init { kani, tla } => {
            cmd_init(common, init::Options { kani: kani.then_some(init::KaniSetup::default()), tla })
        }
        Command::Doctor => cmd_doctor(common),
        Command::Shadow { cmd } => shadow::run(common, cmd),
        Command::Hook { .. } => Ok(Exit::Pass),
        Command::Action { mode, job } => action::run(common, mode, job),
    }
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// The default branch's name: the flag, `origin/HEAD`, else `main` or
/// `master` when one exists.
fn default_branch(repo: &dyn Repo, flag: Option<&str>) -> Result<String, CliError> {
    if let Some(name) = flag {
        return Ok(name.to_string());
    }
    if let Some(name) = repo.default_branch().map_err(CliError::failed)? {
        return Ok(name);
    }
    ["main", "master"]
        .into_iter()
        .find(|b| repo.head(&format!("origin/{b}")).is_ok() || repo.head(b).is_ok())
        .map(str::to_string)
        .ok_or_else(|| {
            CliError::Usage(
                "cannot determine the default branch (no origin/HEAD, main, or master); pass --default-branch".into(),
            )
        })
}

/// The revision the default branch is read at: `origin/<branch>` when the
/// remote-tracking ref exists, else the local branch.
fn default_ref(repo: &dyn Repo, branch: &str, base: Option<&str>) -> String {
    match base {
        Some(b) => b.to_string(),
        None => {
            let remote = format!("origin/{branch}");
            if repo.head(&remote).is_ok() { remote } else { branch.to_string() }
        }
    }
}

struct Loaded {
    ws: Workspace,
    properties: Vec<LoadedProperty>,
    config: ConfigSource,
}

fn load(common: &Common) -> Result<Loaded, CliError> {
    let ws = Workspace::open(common.repo.as_deref())?;
    ws.require_proob_dir()?;
    let properties = ws.load_properties()?;
    let config = ws.load_config()?;
    Ok(Loaded { ws, properties, config })
}

fn ttl_secs(c: &Config) -> u64 {
    u64::from(c.freshness_days) * SECS_PER_DAY
}

fn print_report(report: &Report, json: bool, summary: bool) -> Exit {
    let exit = if report.blocks() { Exit::Block } else { Exit::Pass };
    if json {
        let body = json::report_json(report, render::summary_line(report), exit.code());
        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
    } else if summary {
        println!("{}", render::summary_line(report));
    } else {
        print!("{}", render::markdown(report));
    }
    exit
}

/// The status report of a workspace, as the hook and `status` compute it,
/// with the working tree's uncovered paths against the default branch.
fn status_report(loaded: &Loaded, common: &Common, base: Option<&str>) -> Result<Report, CliError> {
    let candidate = loaded.config.candidate();
    let bindings = adapters::bindings(&loaded.ws, &candidate)?;
    with_backend(&loaded.ws, candidate.call_budget, ttl_secs(&candidate), bindings.as_ref(), |backend| {
        let branch = default_branch(backend.repo, common.default_branch.as_deref())?;
        let default_ref = default_ref(backend.repo, &branch, base);
        let inputs = Inputs {
            ws: &loaded.ws,
            backend,
            properties: &loaded.properties,
            default_branch: &branch,
            default_ref: &default_ref,
            candidate: &candidate,
            candidate_missing: matches!(loaded.config, ConfigSource::Missing),
        };
        let report = pipeline::run(&inputs, None)?;
        // Informational only: a shallow clone or an unrelated history leaves it out, never fails status.
        let uncovered = worktree_uncovered(loaded, backend.bindings, &candidate, &default_ref).ok();
        Ok(Report { uncovered, ..report })
    })?
}

/// The working tree's uncovered paths against its merge base with the default branch.
fn worktree_uncovered(
    loaded: &Loaded,
    bindings: Option<&backend::Bindings>,
    config: &Config,
    default_ref: &str,
) -> Result<Vec<String>, CliError> {
    let changed =
        proob_effects::git::Git::open(&loaded.ws.root).worktree_changed_paths(default_ref).map_err(CliError::failed)?;
    uncovered::for_tree(&loaded.ws.root, changed, &loaded.properties, bindings, config)
}

/// The one-line summary for a workspace (the session hook).
pub fn summary_for(ws: &Workspace, common: &Common) -> Result<String, CliError> {
    let loaded = Loaded { properties: ws.load_properties()?, config: ws.load_config()?, ws: ws.clone() };
    let report = status_report(&loaded, common, None)?;
    // A shadow's line stays as it is: its proposals are never committed anywhere.
    Ok(if shadow::is_shadow(&ws.root) { render::summary_line(&report) } else { render::hook_line(&report) })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ValidateJ<'a> {
    ok: bool,
    problems: &'a [validate::Problem],
}

fn cmd_validate(common: &Common) -> Result<Exit, CliError> {
    let ws = Workspace::open(common.repo.as_deref())?;
    ws.require_proob_dir()?;
    let repo = proob_effects::git::Git::open(&ws.root);
    let problems = validate::run(&ws, &repo)?;
    let ok = problems.is_empty();
    if common.json {
        println!("{}", serde_json::to_string_pretty(&ValidateJ { ok, problems: &problems }).unwrap_or_default());
    } else if ok {
        println!(
            "proob validate: ok ({} property file{})",
            ws.property_paths()?.len(),
            vocab::plural(ws.property_paths()?.len() as u64)
        );
    } else {
        for p in &problems {
            println!("{p}");
        }
        println!("proob validate: {} problem{}", problems.len(), vocab::plural(problems.len() as u64));
    }
    Ok(if ok { Exit::Pass } else { Exit::Block })
}

fn cmd_doctor(common: &Common) -> Result<Exit, CliError> {
    let ws = Workspace::open(common.repo.as_deref())?;
    let report = doctor::run(&ws.root, &ws.load_config()?.candidate())?;
    if common.json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    } else {
        print!("{}", doctor::text(&report));
    }
    Ok(if report.ok { Exit::Pass } else { Exit::Block })
}

fn cmd_hash(common: &Common, id: &str) -> Result<Exit, CliError> {
    let loaded = load(common)?;
    let candidate = loaded.config.candidate();
    let property = hash::find(&loaded.properties, id)?;
    let bindings = adapters::bindings(&loaded.ws, &candidate)?;
    let hashes = with_backend(&loaded.ws, 0, 0, bindings.as_ref(), |backend: &Backend<'_>| {
        hash::hashes(&loaded.ws, backend, property, candidate.waiver_max_days)
    })??;
    if common.json {
        println!("{}", serde_json::to_string_pretty(&hashes).unwrap_or_default());
    } else {
        print!("{}", hash::text(&hashes));
    }
    Ok(Exit::Pass)
}

fn cmd_status(common: &Common, base: Option<&str>, summary: bool) -> Result<Exit, CliError> {
    let loaded = load(common)?;
    let report = status_report(&loaded, common, base)?;
    print_report(&report, common.json, summary);
    Ok(Exit::Pass)
}

/// `results.jsonl` by property id; a malformed line names its number.
fn load_results(path: Option<&std::path::Path>) -> Result<BTreeMap<PropertyId, RunResultRecord>, CliError> {
    let Some(path) = path else { return Ok(BTreeMap::new()) };
    let label = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|e| CliError::failed(format!("cannot read {label}: {e}")))?;
    let mut by_id = BTreeMap::new();
    for (line, parsed) in parse_results(&text) {
        let record = parsed.map_err(|e| CliError::invalid(&label, format!("line {line}: {e}")))?;
        let id = ulid::decode(&record.id).map_err(|e| CliError::invalid(&label, format!("line {line}: id: {e}")))?;
        by_id.insert(id, record);
    }
    Ok(by_id)
}

/// The base ref's `proob/config.toml`, the candidate that governs a check;
/// `None` is bootstrap.
fn base_config(repo: &dyn Repo, base: &str) -> Result<Option<Config>, CliError> {
    repo.file_at(base, CONFIG)
        .map_err(CliError::failed)?
        .map(|bytes| parse_config_text(&String::from_utf8_lossy(&bytes), &format!("{base}:{CONFIG}")))
        .transpose()
}

fn cmd_check(
    common: &Common,
    base: &str,
    head: &str,
    results: Option<&std::path::Path>,
    pr: Option<u64>,
) -> Result<Exit, CliError> {
    let loaded = load(common)?;
    let results = load_results(results)?;
    let report = check_report(common, &loaded, base, head, &results, pr)?;
    Ok(print_report(&report, common.json, false))
}

/// The `check` report: the base ref's config governs, the head's adapters
/// find the bindings, and bootstrap applies when the base has no config. The
/// Action's judge job computes exactly this.
fn check_report(
    common: &Common,
    loaded: &Loaded,
    base: &str,
    head: &str,
    results: &BTreeMap<PropertyId, RunResultRecord>,
    pr: Option<u64>,
) -> Result<Report, CliError> {
    let head_config = loaded.config.candidate();
    let git = proob_effects::git::Git::open(&loaded.ws.root);
    require_checked_out(&git, head)?;
    let base_config = base_config(&git, base)?;
    // The base config governs everything about the check, its call budget included.
    let governing = base_config.clone().unwrap_or_else(Config::defaults);
    // Bindings are a fact about the tree, which is at the head: the head's adapters find them.
    let bindings = adapters::bindings(&loaded.ws, &head_config)?;
    with_backend(&loaded.ws, governing.call_budget, ttl_secs(&governing), bindings.as_ref(), |backend| {
        let branch = default_branch(backend.repo, common.default_branch.as_deref())?;
        let default_ref = default_ref(backend.repo, &branch, None);
        let inputs = Inputs {
            ws: &loaded.ws,
            backend,
            properties: &loaded.properties,
            default_branch: &branch,
            default_ref: &default_ref,
            candidate: &governing,
            candidate_missing: false,
        };
        let report = match base_config {
            None => bootstrap(loaded, &inputs, base, head)?,
            Some(_) => {
                let chk =
                    CheckInputs { base, head, results, pr, head_allow_self_signed: head_config.allow_self_signed };
                pipeline::run(&inputs, Some(&chk))?
            }
        };
        // The head's config names the languages and the ignore list: like the
        // bindings, they are facts about the tree, and the list decides nothing.
        let changed = git.changed_paths(base, head).map_err(CliError::failed)?;
        let uncovered =
            uncovered::for_tree(&loaded.ws.root, changed, &loaded.properties, backend.bindings, &head_config)?;
        Ok(Report { uncovered: Some(uncovered), ..report })
    })?
}

/// The working tree is what `check` judges, so it must be at `--head`.
fn require_checked_out(repo: &dyn Repo, head: &str) -> Result<(), CliError> {
    let at = repo.head("HEAD").map_err(CliError::failed)?;
    let wanted = repo.head(head).map_err(|e| CliError::Usage(format!("cannot resolve --head {head}: {e}")))?;
    if at == wanted {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "the working tree is at {} but --head {head} is {}; check out {head} first (check judges the tree)",
            &proob_facts::hex::encode_oid(&at)[..12],
            &proob_facts::hex::encode_oid(&wanted)[..12]
        )))
    }
}

/// The base ref has no config: validate the head and render every property
/// as proposed. Invalid files are an error (exit 2): a bootstrap PR must be
/// well-formed before the gate it introduces can mean anything.
fn bootstrap(loaded: &Loaded, inputs: &Inputs<'_>, base: &str, head: &str) -> Result<Report, CliError> {
    let problems = validate::run(&loaded.ws, inputs.backend.repo)?;
    if let Some(first) = problems.first() {
        let rest = problems.len() - 1;
        return Err(CliError::Usage(format!(
            "bootstrap: the head's proob files are not valid: {first}{}",
            if rest > 0 { format!(" (and {rest} more; run `proob validate`)") } else { String::new() }
        )));
    }
    Ok(pipeline::bootstrap(inputs, base, head))
}

fn cmd_run(common: &Common, what: run::What, id: &str, opts: run::Options) -> Result<Exit, CliError> {
    let loaded = load(common)?;
    // In a shadow every run is recorded: `proob shadow report` reads them.
    let opts = run::Options { record: opts.record || shadow::is_shadow(&loaded.ws.root), ..opts };
    run::run(&loaded.ws, &loaded.config.candidate(), &loaded.properties, what, id, opts)
}

fn cmd_init(common: &Common, opts: init::Options) -> Result<Exit, CliError> {
    let ws = Workspace::open(common.repo.as_deref())?;
    let repo = proob_effects::git::Git::open(&ws.root);
    let branch = default_branch(&repo, common.default_branch.as_deref()).unwrap_or_else(|_| "main".to_string());
    let outcome = init::run(&ws, &branch, opts)?;
    if common.json {
        println!("{}", serde_json::to_string_pretty(&outcome).unwrap_or_default());
    } else {
        print!("{}", init::text(&outcome));
    }
    Ok(Exit::Pass)
}
