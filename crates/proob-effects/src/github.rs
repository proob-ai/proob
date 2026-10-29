//! The GitHub client: a read-only trait with exactly the calls the eligibility
//! rule, governance, and evidence retrieval need, a `reqwest` implementation
//! behind it, a per-run call budget, and a token that is never printed.
//!
//! Every call reads; nothing here can write to GitHub. The trait is the seam
//! for tests (`fixture::FixtureApi`) and for the cache (`cache::CachedApi`),
//! both of which implement it. Payloads are the facts crate's DTOs; no call
//! returns an `Approval`, a `Status`, or a `Verdict`.
//!
//! The budget bounds API cost per run (plan: "API cost is bounded per run"):
//! each request, each page of a paginated request, spends one call, and a
//! spent budget answers `ApiError::RateLimited` without touching the network,
//! so the caller's ordering (affected files first) decides what is derived.
use crate::error::ApiError;
use proob_facts::github::{
    Artifact, Artifacts, Branch, BranchRule, Commit, Contents, Issue, Permission, PullRequest, Review, User,
    WorkflowRun, WorkflowRuns,
};
use proob_facts::paths::CODEOWNERS;
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderValue, USER_AGENT};
use serde::de::DeserializeOwned;
use std::cell::Cell;
use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::time::Duration;

/// How to list workflow runs. `None` leaves GitHub's default; `max_runs` caps
/// the pages fetched (100 runs per page, one call each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunFilter<'a> {
    pub branch: Option<&'a str>,
    pub event: Option<&'a str>,
    pub status: Option<&'a str>,
    pub max_runs: usize,
}

/// The read-only calls proob makes. Each is one REST endpoint; paginated ones
/// return every page up to a bound.
pub trait GitHubApi {
    /// `GET /repos/{o}/{r}/commits/{sha}/pulls`: the PRs associated with a
    /// commit; empty for a commit GitHub does not know or that no PR carries.
    fn pulls_for_commit(&self, sha: &str) -> Result<Vec<PullRequest>, ApiError>;
    /// `GET /repos/{o}/{r}/pulls/{number}`.
    fn pull(&self, number: u64) -> Result<PullRequest, ApiError>;
    /// `GET /repos/{o}/{r}/pulls/{number}/reviews`, all pages, in submission order.
    fn reviews(&self, number: u64) -> Result<Vec<Review>, ApiError>;
    /// `GET /users/{login}`.
    fn user(&self, login: &str) -> Result<User, ApiError>;
    /// `GET /repos/{o}/{r}/collaborators/{login}/permission`.
    fn permission(&self, login: &str) -> Result<Permission, ApiError>;
    /// `GET /repos/{o}/{r}/pulls/{number}/commits`, all pages (no files).
    fn pull_commits(&self, number: u64) -> Result<Vec<Commit>, ApiError>;
    /// `GET /repos/{o}/{r}/commits/{sha}`, with the files it touched.
    fn commit(&self, sha: &str) -> Result<Commit, ApiError>;
    /// `GET /repos/{o}/{r}/contents/{path}?ref=`: the blob id of a file at a
    /// ref, `None` when the ref or the path does not exist or is not a file.
    fn blob_at(&self, path: &str, git_ref: &str) -> Result<Option<String>, ApiError>;
    /// `GET /repos/{o}/{r}/actions/workflows/{file}/runs`, newest first.
    fn workflow_runs(&self, workflow_file: &str, filter: &RunFilter<'_>) -> Result<Vec<WorkflowRun>, ApiError>;
    /// `GET /repos/{o}/{r}/actions/runs/{run_id}/artifacts`.
    fn artifacts(&self, run_id: u64) -> Result<Vec<Artifact>, ApiError>;
    /// `GET /repos/{o}/{r}/actions/artifacts/{artifact_id}/zip`: the archive bytes.
    fn download_artifact(&self, run_id: u64, artifact_id: u64) -> Result<Vec<u8>, ApiError>;
    /// `GET /repos/{o}/{r}/branches/{name}`: the `protected` flag.
    fn branch(&self, name: &str) -> Result<Branch, ApiError>;
    /// `GET /repos/{o}/{r}/rules/branches/{name}`: the rules in force.
    fn branch_rules(&self, name: &str) -> Result<Vec<BranchRule>, ApiError>;
    /// `GET /repos/{o}/{r}/issues/{number}`: open or closed.
    fn issue(&self, number: u64) -> Result<Issue, ApiError>;
    /// Whether any of GitHub's three CODEOWNERS locations exists at a ref:
    /// up to three `blob_at` calls.
    fn codeowners_present(&self, git_ref: &str) -> Result<bool, ApiError> {
        for path in CODEOWNERS {
            if self.blob_at(path, git_ref)?.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// No token
// ---------------------------------------------------------------------------

/// The API of a run without a token: every call answers `NoToken`. Behind a
/// `CachedApi` it makes a cache-only run, whose view is `Unverified`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Offline;

impl GitHubApi for Offline {
    fn pulls_for_commit(&self, _sha: &str) -> Result<Vec<PullRequest>, ApiError> {
        Err(ApiError::NoToken)
    }
    fn pull(&self, _number: u64) -> Result<PullRequest, ApiError> {
        Err(ApiError::NoToken)
    }
    fn reviews(&self, _number: u64) -> Result<Vec<Review>, ApiError> {
        Err(ApiError::NoToken)
    }
    fn user(&self, _login: &str) -> Result<User, ApiError> {
        Err(ApiError::NoToken)
    }
    fn permission(&self, _login: &str) -> Result<Permission, ApiError> {
        Err(ApiError::NoToken)
    }
    fn pull_commits(&self, _number: u64) -> Result<Vec<Commit>, ApiError> {
        Err(ApiError::NoToken)
    }
    fn commit(&self, _sha: &str) -> Result<Commit, ApiError> {
        Err(ApiError::NoToken)
    }
    fn blob_at(&self, _path: &str, _git_ref: &str) -> Result<Option<String>, ApiError> {
        Err(ApiError::NoToken)
    }
    fn workflow_runs(&self, _workflow_file: &str, _filter: &RunFilter<'_>) -> Result<Vec<WorkflowRun>, ApiError> {
        Err(ApiError::NoToken)
    }
    fn artifacts(&self, _run_id: u64) -> Result<Vec<Artifact>, ApiError> {
        Err(ApiError::NoToken)
    }
    fn download_artifact(&self, _run_id: u64, _artifact_id: u64) -> Result<Vec<u8>, ApiError> {
        Err(ApiError::NoToken)
    }
    fn branch(&self, _name: &str) -> Result<Branch, ApiError> {
        Err(ApiError::NoToken)
    }
    fn branch_rules(&self, _name: &str) -> Result<Vec<BranchRule>, ApiError> {
        Err(ApiError::NoToken)
    }
    fn issue(&self, _number: u64) -> Result<Issue, ApiError> {
        Err(ApiError::NoToken)
    }
}

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

/// Calls left in this run. Shared by reference between the client and the
/// caller, which reads `spent` for the footer.
#[derive(Debug)]
pub struct CallBudget {
    remaining: Cell<u32>,
    spent: Cell<u32>,
}

impl CallBudget {
    pub fn new(calls: u32) -> CallBudget {
        CallBudget { remaining: Cell::new(calls), spent: Cell::new(0) }
    }

    /// Takes one call, or refuses with `RateLimited`.
    pub fn spend(&self) -> Result<(), ApiError> {
        match self.remaining.get() {
            0 => Err(ApiError::RateLimited),
            n => {
                self.remaining.set(n - 1);
                self.spent.set(self.spent.get() + 1);
                Ok(())
            }
        }
    }

    pub fn remaining(&self) -> u32 {
        self.remaining.get()
    }

    pub fn spent(&self) -> u32 {
        self.spent.get()
    }
}

// ---------------------------------------------------------------------------
// Token and repository
// ---------------------------------------------------------------------------

/// A GitHub token. `Debug` and `Display` redact it; it leaves this module only
/// inside an `Authorization` header.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// A non-empty token after trimming; `None` otherwise.
    pub fn new(text: &str) -> Option<Token> {
        let trimmed = text.trim();
        if trimmed.is_empty() { None } else { Some(Token(trimmed.to_string())) }
    }

    /// `GITHUB_TOKEN`, then `GH_TOKEN`.
    pub fn from_env() -> Option<Token> {
        ["GITHUB_TOKEN", "GH_TOKEN"].iter().find_map(|name| std::env::var(name).ok().and_then(|t| Token::new(&t)))
    }

    /// `gh auth token`, through proob-spawn; `None` when `gh` is absent or not signed in.
    pub fn from_gh(timeout: Duration) -> Option<Token> {
        let args = [OsString::from("auth"), OsString::from("token")];
        let out = proob_spawn::run(Path::new("gh"), &args, None, &[], timeout).ok()?;
        if out.success() { Token::new(&String::from_utf8_lossy(&out.stdout)) } else { None }
    }

    /// The environment, then `gh`.
    pub fn discover(gh_timeout: Duration) -> Result<Token, ApiError> {
        Token::from_env().or_else(|| Token::from_gh(gh_timeout)).ok_or(ApiError::NoToken)
    }

    fn header(&self) -> Result<HeaderValue, ApiError> {
        let mut value = HeaderValue::from_str(&format!("Bearer {}", self.0))
            .map_err(|_| ApiError::Transport { path: String::new(), message: "token is not a valid header".into() })?;
        value.set_sensitive(true);
        Ok(value)
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token([redacted])")
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

/// `owner/name` on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoId {
    pub owner: String,
    pub name: String,
}

impl RepoId {
    /// `owner/name`, as `GITHUB_REPOSITORY` gives it.
    pub fn parse(text: &str) -> Option<RepoId> {
        let (owner, name) = text.trim().split_once('/')?;
        let name = name.strip_suffix(".git").unwrap_or(name);
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        Some(RepoId { owner: owner.to_string(), name: name.to_string() })
    }

    /// From a remote URL: `https://github.com/o/r.git`, `git@github.com:o/r.git`,
    /// `ssh://git@github.com/o/r`. The host is not checked here.
    pub fn from_remote_url(url: &str) -> Option<RepoId> {
        let url = url.trim().trim_end_matches('/');
        let path = match url.split_once("://") {
            Some((_, rest)) => rest.split_once('/')?.1,
            None => url.split_once(':')?.1,
        };
        RepoId::parse(path)
    }
}

// ---------------------------------------------------------------------------
// The reqwest client
// ---------------------------------------------------------------------------

const DEFAULT_API: &str = "https://api.github.com";
const PER_PAGE: usize = 100;
const MAX_PAGES: usize = 10;
const API_VERSION: &str = "2022-11-28";

/// The REST client. Every request carries the token in an `Authorization`
/// header and nothing else; redirects to another host (artifact downloads)
/// drop it, which `reqwest` does on its own.
pub struct GitHubClient<'b> {
    http: Client,
    api_base: String,
    repo: RepoId,
    token: Token,
    budget: &'b CallBudget,
}

impl<'b> GitHubClient<'b> {
    /// Against `GITHUB_API_URL` when set (GitHub Enterprise, and Actions sets
    /// it), else api.github.com.
    pub fn new(repo: RepoId, token: Token, budget: &'b CallBudget) -> Result<GitHubClient<'b>, ApiError> {
        let base = std::env::var("GITHUB_API_URL").ok().filter(|b| !b.trim().is_empty()).unwrap_or(DEFAULT_API.into());
        GitHubClient::with_base(&base, repo, token, budget)
    }

    pub fn with_base(
        api_base: &str,
        repo: RepoId,
        token: Token,
        budget: &'b CallBudget,
    ) -> Result<GitHubClient<'b>, ApiError> {
        // The process-wide TLS provider: ring, installed once; a second
        // installation is refused and that is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| ApiError::Transport { path: String::new(), message: e.to_string() })?;
        Ok(GitHubClient { http, api_base: api_base.trim_end_matches('/').to_string(), repo, token, budget })
    }

    fn repo_path(&self, tail: &str) -> String {
        format!("/repos/{}/{}/{tail}", self.repo.owner, self.repo.name)
    }

    /// One request; spends one call.
    fn send(&self, path: &str, query: &[(&str, String)]) -> Result<Response, ApiError> {
        self.budget.spend()?;
        let url = format!("{}{path}", self.api_base);
        self.http
            .get(url)
            .header(AUTHORIZATION, self.token.header()?)
            .header(ACCEPT, "application/vnd.github+json")
            .header(USER_AGENT, concat!("proob/", env!("CARGO_PKG_VERSION")))
            .header("X-GitHub-Api-Version", API_VERSION)
            .query(query)
            .send()
            .map_err(|e| ApiError::Transport { path: path.to_string(), message: e.to_string() })
    }

    /// Body bytes of a 2xx answer; everything else is an error by status.
    fn body(&self, path: &str, query: &[(&str, String)]) -> Result<Vec<u8>, ApiError> {
        let response = self.send(path, query)?;
        let status = response.status();
        if status.is_success() {
            return response
                .bytes()
                .map(|b| b.to_vec())
                .map_err(|e| ApiError::Transport { path: path.to_string(), message: e.to_string() });
        }
        let limited = response.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()) == Some("0");
        Err(match status.as_u16() {
            429 => ApiError::RateLimited,
            403 if limited => ApiError::RateLimited,
            401 | 403 => ApiError::Unauthorized { path: path.to_string() },
            404 => ApiError::NotFound { path: path.to_string() },
            code => ApiError::Http { status: code, path: path.to_string() },
        })
    }

    fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T, ApiError> {
        let bytes = self.body(path, query)?;
        serde_json::from_slice(&bytes).map_err(|e| ApiError::Decode { path: path.to_string(), message: e.to_string() })
    }

    /// Every page until a short page, or `max_items`, or `MAX_PAGES`.
    fn pages<T: DeserializeOwned, U>(
        &self,
        path: &str,
        query: &[(&str, String)],
        max_items: usize,
        items: fn(T) -> Vec<U>,
    ) -> Result<Vec<U>, ApiError> {
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let mut q: Vec<(&str, String)> = query.to_vec();
            q.push(("per_page", PER_PAGE.to_string()));
            q.push(("page", page.to_string()));
            let got = items(self.get::<T>(path, &q)?);
            let short = got.len() < PER_PAGE;
            all.extend(got);
            if short || all.len() >= max_items {
                break;
            }
        }
        all.truncate(max_items);
        Ok(all)
    }
}

fn identity<T>(v: Vec<T>) -> Vec<T> {
    v
}

impl GitHubApi for GitHubClient<'_> {
    fn pulls_for_commit(&self, sha: &str) -> Result<Vec<PullRequest>, ApiError> {
        match self.pages(&self.repo_path(&format!("commits/{sha}/pulls")), &[], usize::MAX, identity::<PullRequest>) {
            // 422 "No commit found" and 404 both mean GitHub has no such commit.
            Err(ApiError::NotFound { .. } | ApiError::Http { status: 422, .. }) => Ok(Vec::new()),
            other => other,
        }
    }

    fn pull(&self, number: u64) -> Result<PullRequest, ApiError> {
        self.get(&self.repo_path(&format!("pulls/{number}")), &[])
    }

    fn reviews(&self, number: u64) -> Result<Vec<Review>, ApiError> {
        self.pages(&self.repo_path(&format!("pulls/{number}/reviews")), &[], usize::MAX, identity::<Review>)
    }

    fn user(&self, login: &str) -> Result<User, ApiError> {
        self.get(&format!("/users/{login}"), &[])
    }

    fn permission(&self, login: &str) -> Result<Permission, ApiError> {
        self.get(&self.repo_path(&format!("collaborators/{login}/permission")), &[])
    }

    fn pull_commits(&self, number: u64) -> Result<Vec<Commit>, ApiError> {
        self.pages(&self.repo_path(&format!("pulls/{number}/commits")), &[], usize::MAX, identity::<Commit>)
    }

    fn commit(&self, sha: &str) -> Result<Commit, ApiError> {
        self.get(&self.repo_path(&format!("commits/{sha}")), &[])
    }

    fn blob_at(&self, path: &str, git_ref: &str) -> Result<Option<String>, ApiError> {
        let request = self.repo_path(&format!("contents/{path}"));
        match self.body(&request, &[("ref", git_ref.to_string())]) {
            Ok(bytes) => {
                Ok(serde_json::from_slice::<Contents>(&bytes).ok().filter(|c| c.kind == "file").map(|c| c.sha))
            }
            Err(ApiError::NotFound { .. } | ApiError::Http { status: 422, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn workflow_runs(&self, workflow_file: &str, filter: &RunFilter<'_>) -> Result<Vec<WorkflowRun>, ApiError> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(b) = filter.branch {
            query.push(("branch", b.to_string()));
        }
        if let Some(e) = filter.event {
            query.push(("event", e.to_string()));
        }
        if let Some(s) = filter.status {
            query.push(("status", s.to_string()));
        }
        self.pages(
            &self.repo_path(&format!("actions/workflows/{workflow_file}/runs")),
            &query,
            filter.max_runs,
            |r: WorkflowRuns| r.workflow_runs,
        )
    }

    fn artifacts(&self, run_id: u64) -> Result<Vec<Artifact>, ApiError> {
        self.pages(&self.repo_path(&format!("actions/runs/{run_id}/artifacts")), &[], usize::MAX, |a: Artifacts| {
            a.artifacts
        })
    }

    fn download_artifact(&self, _run_id: u64, artifact_id: u64) -> Result<Vec<u8>, ApiError> {
        self.body(&self.repo_path(&format!("actions/artifacts/{artifact_id}/zip")), &[])
    }

    fn branch(&self, name: &str) -> Result<Branch, ApiError> {
        self.get(&self.repo_path(&format!("branches/{name}")), &[])
    }

    fn branch_rules(&self, name: &str) -> Result<Vec<BranchRule>, ApiError> {
        self.pages(&self.repo_path(&format!("rules/branches/{name}")), &[], usize::MAX, identity::<BranchRule>)
    }

    fn issue(&self, number: u64) -> Result<Issue, ApiError> {
        self.get(&self.repo_path(&format!("issues/{number}")), &[])
    }
}
