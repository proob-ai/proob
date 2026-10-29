//! `proob action --mode <check|evidence> --job <execute|judge>`: the two jobs
//! of each proob workflow, as the composite action runs them.
//!
//! Each mode is two jobs with one rule between them: `execute` runs
//! repository code through the adapters and holds no token (it refuses to
//! start if one is in its environment); `judge` holds the read-only token and
//! runs no repository code (its only inputs are the tree, git history,
//! GitHub's records, and the raw results the execute job uploaded, which it
//! accepts only when their manifest names this very run). Nothing here
//! decides a verdict: the judge calls the same `check` and `status` pipelines
//! the CLI does, so a job summary and a session line are the same function
//! of the same facts.
//!
//! Fork pull requests get the same two jobs unchanged: nothing needs a write
//! permission, and the read-only token GitHub hands a fork's run is all the
//! judge ever uses.
pub mod env;
pub mod execute;
pub mod judge;

use crate::cli::error::CliError;
use crate::cli::{Common, Exit};
use env::{ActionEnv, Event, PullRequestEvent};
use proob_effects::git::{Git, Repo};
use proob_facts::hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    /// The pull-request gate.
    Check,
    /// The default-branch evidence run.
    Evidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Job {
    /// Runs the adapters; holds no token.
    Execute,
    /// Derives verdicts with the read-only token; runs no repository code.
    Judge,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Check => "check",
            Mode::Evidence => "evidence",
        }
    }
}

pub fn run(common: &Common, mode: Mode, job: Job) -> Result<Exit, CliError> {
    if job == Job::Execute {
        refuse_token()?;
    }
    let env = env::read()?;
    check_event(mode, &env.event)?;
    let common = with_default_branch(common, &env);
    match job {
        Job::Execute => execute::run(&common, &env, mode),
        Job::Judge => judge::run(&common, &env, mode),
    }
}

/// An execute job runs repository code; a token in its environment would
/// contradict the design, so the job does not start.
fn refuse_token() -> Result<(), CliError> {
    match env::token_present() {
        Some(name) => Err(CliError::Usage(format!(
            "proob action: the execute job runs repository code and must hold no token, but {name} is set; remove `env: {name}` from the execute job (only the judge job takes the token)"
        ))),
        None => Ok(()),
    }
}

/// Check mode is for pull requests; evidence mode for the default branch's own events.
fn check_event(mode: Mode, event: &Event) -> Result<(), CliError> {
    match (mode, event) {
        (Mode::Check, Event::PullRequest(_)) => Ok(()),
        (Mode::Evidence, Event::Push | Event::Schedule | Event::WorkflowDispatch) => Ok(()),
        (Mode::Check, other) => Err(CliError::Usage(format!(
            "mode check judges pull requests, but this run's event is {}; the default-branch workflow uses mode evidence",
            other.name()
        ))),
        (Mode::Evidence, other) => Err(CliError::Usage(format!(
            "mode evidence runs on push, schedule, or workflow_dispatch, but this run's event is {}; the pull-request workflow uses mode check",
            other.name()
        ))),
    }
}

/// The flag wins; else the default branch the event payload names.
fn with_default_branch(common: &Common, env: &ActionEnv) -> Common {
    Common { default_branch: common.default_branch.clone().or_else(|| env.default_branch.clone()), ..common.clone() }
}

/// The pull request of a check-mode run; `check_event` has already ensured it.
pub fn pull_request(env: &ActionEnv) -> Result<&PullRequestEvent, CliError> {
    match &env.event {
        Event::PullRequest(pr) => Ok(pr),
        other => Err(CliError::Usage(format!("not a pull_request run: {}", other.name()))),
    }
}

/// The checkout must be at the run's commit: both jobs reason about `GITHUB_SHA`.
pub fn require_at(repo: &dyn Repo, sha: &str) -> Result<(), CliError> {
    let at = repo.head("HEAD").map_err(CliError::failed)?;
    let wanted = repo.head(sha).map_err(|e| {
        CliError::Usage(format!("GITHUB_SHA {sha} is not in this checkout: {e}; check out with fetch-depth: 0"))
    })?;
    if at == wanted {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "the checkout is at {} but this run is about GITHUB_SHA {}; let actions/checkout check out the run's own commit",
            &hex::encode_oid(&at)[..12],
            &sha[..12]
        )))
    }
}

/// The base revision of a pull request: the event's base sha when the
/// checkout has it, else the base branch after fetching it (a shallow
/// checkout), else an error that names the fix.
pub fn resolve_base(git: &Git, pr: &PullRequestEvent) -> Result<String, CliError> {
    if git.head(&pr.base_sha).is_ok() {
        return Ok(pr.base_sha.clone());
    }
    let fetched = git.fetch("origin", &pr.base_ref);
    if git.head(&pr.base_sha).is_ok() {
        return Ok(pr.base_sha.clone());
    }
    let remote = format!("origin/{}", pr.base_ref);
    if git.head(&remote).is_ok() {
        return Ok(remote);
    }
    Err(CliError::Usage(format!(
        "cannot resolve the base of this pull request ({} at {}): {}; check out with fetch-depth: 0",
        pr.base_ref,
        &pr.base_sha[..12],
        fetched.err().map_or_else(|| "the fetch brought nothing".to_string(), |e| e.to_string())
    )))
}

/// Whether the checkout declares anything: `proob/config.toml` or
/// `proob/properties/`. Not `proob/` itself, which a downloaded artifact
/// under `proob/.cache/` creates in a repository that has never run `init`.
pub fn has_declaration(ws: &crate::cli::workspace::Workspace) -> bool {
    ws.root.join(proob_facts::paths::CONFIG).is_file() || ws.root.join(proob_facts::paths::PROPERTIES_DIR).is_dir()
}

/// The one-line headline when the checkout declares nothing: nothing is
/// governed, nothing runs, nothing blocks.
pub fn nothing_headline(mode: Mode, sha: &str) -> String {
    format!("proob {}: no proob/ directory at {}; nothing is governed yet (run `proob init`)", mode.name(), &sha[..12])
}
