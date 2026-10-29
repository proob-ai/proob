//! Which GitHub and which repository a run talks to.
//!
//! Three backends serve the same two traits:
//!
//! - **Live**: a token (`GITHUB_TOKEN`, `GH_TOKEN`, or `gh auth token`) and a
//!   repository (`GITHUB_REPOSITORY` or the `origin` remote) give the REST
//!   client, behind the fact cache. The view is `Verified`.
//! - **Offline**: no token or no remote. Every call answers `NoToken`, behind
//!   the same cache, so facts fetched earlier still reach the verified core
//!   and the view says how old they are. The view is `Unverified`.
//! - **Fixture** (test hook): `PROOB_FIXTURE_DIR` names a directory holding
//!   `github.json`, a `proob_effects::fixture::Scenario` served as GitHub,
//!   and optionally `bindings.json`, binding sets (`{"<ULID>": {"files":
//!   [..]}}`) that stand in for the adapters' discovery when present. The
//!   repository is always the real working tree. Honoured only when set;
//!   nothing else reads it.
//!
//! Binding sets otherwise come from the adapter registry (`cli::adapters`),
//! run by the command before the backend is built and handed in here.
//!
//! The cache lives under `proob/.cache/facts` and is disabled in GitHub
//! Actions, so a judge job starts from nothing on disk.
use crate::cli::error::CliError;
use crate::cli::workspace::Workspace;
use proob_core::types::{PropertyId, Secs};
use proob_effects::cache::{Cache, CachedApi};
use proob_effects::facts::GovernedSet;
use proob_effects::fixture::{FixtureApi, Scenario};
use proob_effects::gather::BindingSet;
use proob_effects::git::{Git, Repo};
use proob_effects::github::{CallBudget, GitHubApi, GitHubClient, Offline, RepoId, Token};
use proob_facts::ulid;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const FIXTURE_DIR_ENV: &str = "PROOB_FIXTURE_DIR";
const GH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineWhy {
    NoToken,
    NoRemote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Live,
    Offline { why: OfflineWhy, cache_age: Option<Secs> },
    Fixture,
}

/// Binding sets by property id, as an adapter's `discover` would report them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bindings {
    pub by_id: BTreeMap<PropertyId, BindingSet>,
    pub governed: GovernedSet,
    /// Properties whose token an adapter saw where no configured adapter
    /// checks (a `verus!` item in an unverified crate): `Not checked (no adapter)`.
    pub no_adapter: BTreeSet<PropertyId>,
}

pub struct Backend<'a> {
    pub api: &'a dyn GitHubApi,
    pub repo: &'a dyn Repo,
    pub source: Source,
    pub budget: &'a CallBudget,
    /// `Some` when an adapter (or the fixture) covers this repository.
    pub bindings: Option<&'a Bindings>,
}

/// Builds the backend for this run and hands it to `f`. The budget and the
/// client live on this frame, which keeps the trait objects free of lifetimes.
/// `discovered` is what the adapters found; a fixture's `bindings.json`
/// replaces it when present.
pub fn with_backend<R>(
    ws: &Workspace,
    calls: u32,
    ttl_secs: Secs,
    discovered: Option<&Bindings>,
    f: impl FnOnce(&Backend<'_>) -> R,
) -> Result<R, CliError> {
    let budget = CallBudget::new(calls);
    let repo = Git::open(&ws.root);
    if let Some(dir) = fixture_dir() {
        let scenario = Scenario::load(&dir.join("github.json")).map_err(CliError::Failed)?;
        let fixture_bindings = load_bindings(&dir)?;
        let api = FixtureApi::new(&scenario, &budget);
        let bindings = fixture_bindings.as_ref().or(discovered);
        let backend = Backend { api: &api, repo: &repo, source: Source::Fixture, budget: &budget, bindings };
        return Ok(f(&backend));
    }
    let cache = if in_actions() { Cache::disabled() } else { Cache::in_repo(&ws.root) };
    let cache_age = cache.age(ws.now);
    match live_client(&repo, &budget)? {
        Ok(client) => {
            let api = CachedApi::new(&client, cache, ws.now, ttl_secs);
            Ok(f(&Backend { api: &api, repo: &repo, source: Source::Live, budget: &budget, bindings: discovered }))
        }
        Err(why) => {
            let offline = Offline;
            let api = CachedApi::new(&offline, cache, ws.now, ttl_secs);
            let source = Source::Offline { why, cache_age };
            Ok(f(&Backend { api: &api, repo: &repo, source, budget: &budget, bindings: discovered }))
        }
    }
}

/// A backend that never looks for a token and never reads the cache: git and
/// the working tree only, every GitHub call answering `NoToken`. For the
/// commands that need no fact from GitHub (`run`, the Action's execute job),
/// so a job that holds no token never goes looking for one.
pub fn with_offline_backend<R>(ws: &Workspace, bindings: Option<&Bindings>, f: impl FnOnce(&Backend<'_>) -> R) -> R {
    let budget = CallBudget::new(0);
    let repo = Git::open(&ws.root);
    let offline = Offline;
    let source = Source::Offline { why: OfflineWhy::NoToken, cache_age: None };
    f(&Backend { api: &offline, repo: &repo, source, budget: &budget, bindings })
}

fn fixture_dir() -> Option<PathBuf> {
    std::env::var_os(FIXTURE_DIR_ENV).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// A fixture supplies binding sets, so discovery is not run.
pub fn fixture_has_bindings() -> bool {
    fixture_dir().is_some_and(|d| d.join("bindings.json").is_file())
}

fn in_actions() -> bool {
    std::env::var("GITHUB_ACTIONS").is_ok_and(|v| v == "true")
}

/// The REST client, or why there is none. A client that cannot be built at
/// all (a malformed token) is an error, not an offline run.
fn live_client<'b>(repo: &dyn Repo, budget: &'b CallBudget) -> Result<Result<GitHubClient<'b>, OfflineWhy>, CliError> {
    let Some(id) = repo_id(repo)? else { return Ok(Err(OfflineWhy::NoRemote)) };
    let Ok(token) = Token::discover(GH_TIMEOUT) else { return Ok(Err(OfflineWhy::NoToken)) };
    GitHubClient::new(id, token, budget).map(Ok).map_err(CliError::failed)
}

/// `GITHUB_REPOSITORY`, then the `origin` remote.
fn repo_id(repo: &dyn Repo) -> Result<Option<RepoId>, CliError> {
    if let Some(id) = std::env::var("GITHUB_REPOSITORY").ok().and_then(|v| RepoId::parse(&v)) {
        return Ok(Some(id));
    }
    Ok(repo.remote_url("origin").map_err(CliError::failed)?.and_then(|url| RepoId::from_remote_url(&url)))
}

#[derive(Deserialize)]
struct BindingFile {
    files: Vec<String>,
}

fn load_bindings(dir: &Path) -> Result<Option<Bindings>, CliError> {
    let path = dir.join("bindings.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CliError::failed(format!("{}: {e}", path.display()))),
    };
    let raw: BTreeMap<String, BindingFile> =
        serde_json::from_str(&text).map_err(|e| CliError::invalid(path.display().to_string(), e))?;
    let by_id = raw
        .into_iter()
        .map(|(id, b)| ulid::decode(&id).map(|n| (n, BindingSet { files: b.files })))
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|e| CliError::invalid(path.display().to_string(), e))?;
    let governed = GovernedSet::new(by_id.values().flat_map(|b| b.files.iter().cloned()));
    Ok(Some(Bindings { by_id, governed, no_adapter: BTreeSet::new() }))
}
