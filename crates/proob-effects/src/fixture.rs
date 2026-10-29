//! Fixture-backed fakes: a `GitHubApi` and a `Repo` served from one JSON
//! scenario file, so every rule has a scenario in `tests/fixtures/` and no
//! test touches the network or a real repository.
//!
//! A scenario holds GitHub payloads in the facts crate's own shapes (what the
//! REST API would answer) and repository facts (heads, first-parent history
//! per path, commit times, blobs by `<rev>:<path>`, working-tree files). It
//! holds no approval and no verdict: the expected outcome of a scenario lives
//! in the test that reads it. Every API call spends one call of the budget,
//! like the real client.
use crate::error::{ApiError, GitError};
use crate::git::Repo;
use crate::github::{CallBudget, GitHubApi, RunFilter};
use proob_core::types::{Oid, Secs};
use proob_facts::evidence::RECORDS_FILE;
use proob_facts::github::{
    Artifact, Branch, BranchRule, Commit, Issue, Permission, PullRequest, Review, User, WorkflowRun,
};
use proob_facts::hex;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

fn main_branch() -> String {
    "main".to_string()
}

/// One scenario file. Every section is optional; a missing lookup is
/// `NotFound`, except the PRs of a commit, which are empty (as GitHub
/// answers for a commit no PR carries).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Scenario {
    #[serde(default)]
    pub description: String,
    #[serde(default = "main_branch")]
    pub default_branch: String,
    /// The governed file(s) the scenario is about.
    #[serde(default)]
    pub governed_paths: Vec<String>,
    /// The commit git names as introducing them, for scenarios that bypass `history`.
    #[serde(default)]
    pub introducing_commit: Option<String>,
    // GitHub, keyed by sha, number, or login (numbers as strings: JSON keys).
    #[serde(default)]
    pub pulls_for_commit: BTreeMap<String, Vec<PullRequest>>,
    #[serde(default)]
    pub pulls: BTreeMap<String, PullRequest>,
    #[serde(default)]
    pub reviews: BTreeMap<String, Vec<Review>>,
    #[serde(default)]
    pub users: BTreeMap<String, User>,
    #[serde(default)]
    pub permissions: BTreeMap<String, Permission>,
    #[serde(default)]
    pub pull_commits: BTreeMap<String, Vec<Commit>>,
    #[serde(default)]
    pub commits: BTreeMap<String, Commit>,
    /// `<rev>:<path>` to a blob sha, or `null` for GitHub's 404. Shared with
    /// the repository side.
    #[serde(default)]
    pub blobs: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub issues: BTreeMap<String, Issue>,
    #[serde(default)]
    pub workflow_runs: Vec<WorkflowRun>,
    /// Run id to its artifacts.
    #[serde(default)]
    pub artifacts: BTreeMap<String, Vec<Artifact>>,
    /// Artifact id to the JSON values written as the lines of its records file.
    #[serde(default)]
    pub artifact_lines: BTreeMap<String, Vec<serde_json::Value>>,
    #[serde(default)]
    pub branch: Option<Branch>,
    #[serde(default)]
    pub branch_rules: Vec<BranchRule>,
    // The repository.
    /// Revision to commit sha.
    #[serde(default)]
    pub heads: BTreeMap<String, String>,
    /// Path to the first-parent commits that touched it, newest first.
    #[serde(default)]
    pub history: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub commit_times: BTreeMap<String, Secs>,
    /// Working tree: path to text content.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// Committed content by `<rev>:<path>`, for `Repo::file_at`; a path
    /// absent here is read from `files`.
    #[serde(default)]
    pub files_at: BTreeMap<String, String>,
    #[serde(default)]
    pub changed: Vec<String>,
    /// The fetch URL of `origin`, for `Repo::remote_url`.
    #[serde(default)]
    pub remote_url: Option<String>,
}

impl Scenario {
    pub fn load(path: &Path) -> Result<Scenario, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn not_found(what: &str) -> ApiError {
    ApiError::NotFound { path: what.to_string() }
}

fn oid(text: &str) -> Option<Oid> {
    hex::decode_oid(text).ok()
}

// ---------------------------------------------------------------------------
// GitHub
// ---------------------------------------------------------------------------

/// The scenario as GitHub, under a budget.
pub struct FixtureApi<'a> {
    scenario: &'a Scenario,
    budget: &'a CallBudget,
}

impl<'a> FixtureApi<'a> {
    pub fn new(scenario: &'a Scenario, budget: &'a CallBudget) -> FixtureApi<'a> {
        FixtureApi { scenario, budget }
    }

    fn lookup<T: Clone>(&self, map: &BTreeMap<String, T>, key: &str, what: &str) -> Result<T, ApiError> {
        self.budget.spend()?;
        map.get(key).cloned().ok_or_else(|| not_found(&format!("{what} {key}")))
    }
}

/// A zip holding `evidence.jsonl` with one JSON value per line, as the
/// evidence job uploads it.
pub fn records_zip(lines: &[serde_json::Value]) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        // Writing to an in-memory cursor cannot fail for lack of space; an
        // error here would be a bug in the fixture, and an empty archive then
        // fails the test that reads it.
        if zip.start_file(RECORDS_FILE, options).is_ok() {
            for line in lines {
                let _ = writeln!(zip, "{line}");
            }
        }
        let _ = zip.finish();
    }
    cursor.into_inner()
}

impl GitHubApi for FixtureApi<'_> {
    fn pulls_for_commit(&self, sha: &str) -> Result<Vec<PullRequest>, ApiError> {
        self.budget.spend()?;
        Ok(self.scenario.pulls_for_commit.get(sha).cloned().unwrap_or_default())
    }

    fn pull(&self, number: u64) -> Result<PullRequest, ApiError> {
        self.lookup(&self.scenario.pulls, &number.to_string(), "pull")
    }

    fn reviews(&self, number: u64) -> Result<Vec<Review>, ApiError> {
        self.lookup(&self.scenario.reviews, &number.to_string(), "reviews of")
    }

    fn user(&self, login: &str) -> Result<User, ApiError> {
        self.lookup(&self.scenario.users, login, "user")
    }

    fn permission(&self, login: &str) -> Result<Permission, ApiError> {
        self.lookup(&self.scenario.permissions, login, "permission of")
    }

    fn pull_commits(&self, number: u64) -> Result<Vec<Commit>, ApiError> {
        self.lookup(&self.scenario.pull_commits, &number.to_string(), "commits of pull")
    }

    fn commit(&self, sha: &str) -> Result<Commit, ApiError> {
        self.lookup(&self.scenario.commits, sha, "commit")
    }

    fn blob_at(&self, path: &str, git_ref: &str) -> Result<Option<String>, ApiError> {
        self.lookup(&self.scenario.blobs, &format!("{git_ref}:{path}"), "blob")
    }

    fn workflow_runs(&self, workflow_file: &str, filter: &RunFilter<'_>) -> Result<Vec<WorkflowRun>, ApiError> {
        self.budget.spend()?;
        let runs = self
            .scenario
            .workflow_runs
            .iter()
            .filter(|r| r.path.rsplit('/').next() == Some(workflow_file))
            .filter(|r| filter.branch.is_none_or(|b| r.head_branch.as_deref() == Some(b)))
            .filter(|r| {
                filter
                    .status
                    .is_none_or(|s| s != "success" || r.conclusion == Some(proob_facts::github::RunConclusion::Success))
            })
            .take(filter.max_runs)
            .cloned()
            .collect();
        Ok(runs)
    }

    fn artifacts(&self, run_id: u64) -> Result<Vec<Artifact>, ApiError> {
        self.budget.spend()?;
        Ok(self.scenario.artifacts.get(&run_id.to_string()).cloned().unwrap_or_default())
    }

    fn download_artifact(&self, _run_id: u64, artifact_id: u64) -> Result<Vec<u8>, ApiError> {
        let lines = self.lookup(&self.scenario.artifact_lines, &artifact_id.to_string(), "artifact")?;
        Ok(records_zip(&lines))
    }

    fn branch(&self, name: &str) -> Result<Branch, ApiError> {
        self.budget.spend()?;
        self.scenario.branch.clone().filter(|b| b.name == name).ok_or_else(|| not_found(&format!("branch {name}")))
    }

    fn branch_rules(&self, _name: &str) -> Result<Vec<BranchRule>, ApiError> {
        self.budget.spend()?;
        Ok(self.scenario.branch_rules.clone())
    }

    fn issue(&self, number: u64) -> Result<Issue, ApiError> {
        self.lookup(&self.scenario.issues, &number.to_string(), "issue")
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

/// The scenario as a repository.
pub struct FixtureRepo<'a> {
    scenario: &'a Scenario,
}

impl<'a> FixtureRepo<'a> {
    pub fn new(scenario: &'a Scenario) -> FixtureRepo<'a> {
        FixtureRepo { scenario }
    }

    fn time_of(&self, sha: &str) -> Secs {
        self.scenario.commit_times.get(sha).copied().unwrap_or(0)
    }

    /// Commits touching any of `paths`, newest first by commit time.
    fn touching(&self, paths: &[&str]) -> Vec<String> {
        let mut shas: Vec<String> = paths
            .iter()
            .flat_map(|p| self.scenario.history.get(*p).cloned().unwrap_or_default())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        shas.sort_by_key(|s| std::cmp::Reverse(self.time_of(s)));
        shas
    }
}

fn git_oid(sha: &str) -> Result<Oid, GitError> {
    oid(sha).ok_or_else(|| GitError::Unparsable { command: "fixture".into(), output: sha.to_string() })
}

impl Repo for FixtureRepo<'_> {
    fn head(&self, rev: &str) -> Result<Oid, GitError> {
        let sha = self.scenario.heads.get(rev).cloned().or_else(|| oid(rev).map(|_| rev.to_string()));
        git_oid(&sha.ok_or_else(|| GitError::Failed {
            command: format!("rev-parse {rev}"),
            code: Some(128),
            stderr: "unknown revision".into(),
        })?)
    }

    fn introducing_commit(&self, rev: &str, paths: &[&str]) -> Result<Option<Oid>, GitError> {
        Ok(self.touching_commits(rev, paths, 1)?.into_iter().next())
    }

    fn touching_commits(&self, _rev: &str, paths: &[&str], limit: usize) -> Result<Vec<Oid>, GitError> {
        self.touching(paths).iter().take(limit).map(|s| git_oid(s)).collect()
    }

    fn commits_since(&self, since: &Oid, _rev: &str, paths: &[&str]) -> Result<Vec<(Oid, Secs)>, GitError> {
        let after = self.time_of(&hex::encode_oid(since));
        self.touching(paths)
            .iter()
            .filter(|s| self.time_of(s) > after)
            .map(|s| git_oid(s).map(|o| (o, self.time_of(s))))
            .collect()
    }

    fn commit_time(&self, oid: &Oid) -> Result<Secs, GitError> {
        Ok(self.time_of(&hex::encode_oid(oid)))
    }

    fn blob_at(&self, rev: &str, path: &str) -> Result<Option<Oid>, GitError> {
        let rev = self.scenario.heads.get(rev).map_or(rev, String::as_str);
        match self.scenario.blobs.get(&format!("{rev}:{path}")) {
            Some(Some(sha)) => git_oid(sha).map(Some),
            _ => Ok(None),
        }
    }

    fn ls_files(&self) -> Result<Vec<String>, GitError> {
        Ok(self.scenario.files.keys().cloned().collect())
    }

    fn changed_paths(&self, _base: &str, _head: &str) -> Result<Vec<String>, GitError> {
        Ok(self.scenario.changed.clone())
    }

    fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        Ok(self.scenario.files.get(path).map(|t| t.as_bytes().to_vec()))
    }

    /// `files_at` by `<rev>:<path>`, else the working-tree file: a scenario
    /// that says nothing about committed content has committed what its tree holds.
    fn file_at(&self, rev: &str, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        let rev = self.scenario.heads.get(rev).map_or(rev, String::as_str);
        let text = self.scenario.files_at.get(&format!("{rev}:{path}")).or_else(|| self.scenario.files.get(path));
        Ok(text.map(|t| t.as_bytes().to_vec()))
    }

    fn default_branch(&self) -> Result<Option<String>, GitError> {
        Ok(Some(self.scenario.default_branch.clone()))
    }

    fn remote_url(&self, _remote: &str) -> Result<Option<String>, GitError> {
        Ok(self.scenario.remote_url.clone())
    }
}
