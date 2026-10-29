//! The fact cache: fetched GitHub payloads under `<repo>/proob/.cache/facts/`.
//!
//! What it may hold, and nothing else (plan: "The cache holds facts, never
//! verdicts"):
//!
//! - immutable payloads keyed by what fixes them: the PRs associated with a
//!   commit sha, a merged PR by number, its reviews and commits by number, a
//!   commit (with files) by sha, a file's blob id at a commit by sha and path,
//!   and an evidence artifact by run id and artifact id;
//! - permission and account facts by login, with a time to live equal to the
//!   freshness window, because they change.
//!
//! Every entry is one of the facts crate's payload types inside `Entry`, so
//! there is no file shape that decodes to an `Approval`, `Status`, or
//! `Verdict`; a hand-written file is either a payload (which the verified
//! `eligibility` then judges exactly as it would judge GitHub's answer) or an
//! unreadable entry, which is a miss. Reads never fail: a missing, stale,
//! truncated, or foreign file is a miss and is overwritten on the next fetch.
//! `last_write` is the cache's own age, for the `Unverified` view.
//!
//! The cache is local and gitignored (`proob/.cache/`); the Action's judge
//! job starts from an empty one, so nothing on disk can reach a `Verified`
//! view in CI.
use crate::error::{ApiError, CacheError};
use crate::github::{GitHubApi, RunFilter};
use proob_core::types::Secs;
use proob_facts::github::{
    Artifact, Branch, BranchRule, Commit, Issue, Permission, PullRequest, Review, User, WorkflowRun,
};
use proob_facts::hex;
use proob_facts::paths::CACHE_DIR;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A cached payload and when it was fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry<T> {
    pub fetched_at: Secs,
    pub payload: T,
}

const STAMP: &str = "last-write";

/// The cache directory, or a disabled cache that misses on every read.
#[derive(Debug, Clone)]
pub struct Cache {
    root: Option<PathBuf>,
}

impl Cache {
    /// `<repo>/proob/.cache/facts`.
    pub fn in_repo(repo: &Path) -> Cache {
        Cache::at(&repo.join(CACHE_DIR).join("facts"))
    }

    pub fn at(dir: &Path) -> Cache {
        Cache { root: Some(dir.to_path_buf()) }
    }

    /// Misses on every read and writes nothing.
    pub fn disabled() -> Cache {
        Cache { root: None }
    }

    pub fn is_enabled(&self) -> bool {
        self.root.is_some()
    }

    fn file(&self, bucket: &str, key: &str, extension: &str) -> Option<PathBuf> {
        let root = self.root.as_ref()?;
        let safe = |s: &str| s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-');
        if !(safe(bucket) && safe(key) && !key.starts_with('.') && !key.is_empty()) {
            return None;
        }
        Some(root.join(bucket).join(format!("{key}.{extension}")))
    }

    /// A payload, or a miss. Any failure to read or decode is a miss.
    pub fn read<T: DeserializeOwned>(&self, bucket: &str, key: &str) -> Option<Entry<T>> {
        let path = self.file(bucket, key, "json")?;
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn write<T: Serialize>(
        &self,
        bucket: &str,
        key: &str,
        fetched_at: Secs,
        payload: &T,
    ) -> Result<(), CacheError> {
        let Some(path) = self.file(bucket, key, "json") else { return Ok(()) };
        let body = serde_json::to_vec_pretty(&Entry { fetched_at, payload })
            .map_err(|e| CacheError { path: path.clone(), message: e.to_string() })?;
        self.store(&path, &body)?;
        self.stamp(fetched_at)
    }

    /// Raw bytes (an artifact), or a miss.
    pub fn read_bytes(&self, bucket: &str, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.file(bucket, key, "bin")?).ok()
    }

    pub fn write_bytes(&self, bucket: &str, key: &str, fetched_at: Secs, bytes: &[u8]) -> Result<(), CacheError> {
        let Some(path) = self.file(bucket, key, "bin") else { return Ok(()) };
        self.store(&path, bytes)?;
        self.stamp(fetched_at)
    }

    /// When the cache was last written, by its own record.
    pub fn last_write(&self) -> Option<Secs> {
        let text = std::fs::read_to_string(self.root.as_ref()?.join(STAMP)).ok()?;
        text.trim().parse().ok()
    }

    /// Seconds since the last write, `None` for an empty or disabled cache.
    pub fn age(&self, now: Secs) -> Option<Secs> {
        self.last_write().map(|t| now.saturating_sub(t))
    }

    fn stamp(&self, now: Secs) -> Result<(), CacheError> {
        let Some(root) = &self.root else { return Ok(()) };
        self.store(&root.join(STAMP), now.to_string().as_bytes())
    }

    /// Writes through a temporary file so a reader never sees a partial entry.
    fn store(&self, path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
        let err = |e: std::io::Error| CacheError { path: path.to_path_buf(), message: e.to_string() };
        let dir = path.parent().ok_or_else(|| CacheError { path: path.to_path_buf(), message: "no parent".into() })?;
        std::fs::create_dir_all(dir).map_err(err)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes).map_err(err)?;
        std::fs::rename(&tmp, path).map_err(err)
    }
}

/// `blob_at`'s key: the ref and a short hash of the path, both filename-safe.
fn blob_key(path: &str, git_ref: &str) -> String {
    let digest = proob_core::content_hash(path.as_bytes());
    format!("{git_ref}-{}", &hex::encode(&digest)[..16])
}

/// A read-through cache in front of any `GitHubApi`. Immutable answers are
/// kept for good; permission and account answers for `ttl_secs`; everything
/// that changes (runs, branches, rules, issues, artifact listings) passes
/// through. Reviews and commits of a PR are cached only once that PR has been
/// seen merged in this run, so an open PR's preview is always live.
pub struct CachedApi<'a, A: GitHubApi> {
    inner: &'a A,
    cache: Cache,
    now: Secs,
    ttl_secs: Secs,
    merged: RefCell<BTreeSet<u64>>,
}

impl<'a, A: GitHubApi> CachedApi<'a, A> {
    pub fn new(inner: &'a A, cache: Cache, now: Secs, ttl_secs: Secs) -> CachedApi<'a, A> {
        CachedApi { inner, cache, now, ttl_secs, merged: RefCell::new(BTreeSet::new()) }
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    fn note_merged(&self, pr: &PullRequest) {
        if pr.is_merged() {
            self.merged.borrow_mut().insert(pr.number);
        }
    }

    fn is_merged(&self, number: u64) -> bool {
        self.merged.borrow().contains(&number)
    }

    /// Cached for good.
    fn forever<T: Serialize + DeserializeOwned>(
        &self,
        bucket: &str,
        key: &str,
        fetch: impl FnOnce() -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        if let Some(entry) = self.cache.read::<T>(bucket, key) {
            return Ok(entry.payload);
        }
        let value = fetch()?;
        let _ = self.cache.write(bucket, key, self.now, &value);
        Ok(value)
    }

    /// Cached while younger than the time to live.
    fn fresh<T: Serialize + DeserializeOwned>(
        &self,
        bucket: &str,
        key: &str,
        fetch: impl FnOnce() -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        if let Some(entry) = self.cache.read::<T>(bucket, key)
            && entry.fetched_at <= self.now
            && self.now - entry.fetched_at <= self.ttl_secs
        {
            return Ok(entry.payload);
        }
        let value = fetch()?;
        let _ = self.cache.write(bucket, key, self.now, &value);
        Ok(value)
    }
}

impl<A: GitHubApi> GitHubApi for CachedApi<'_, A> {
    fn pulls_for_commit(&self, sha: &str) -> Result<Vec<PullRequest>, ApiError> {
        let pulls = self.forever("pulls-for-commit", sha, || self.inner.pulls_for_commit(sha))?;
        pulls.iter().for_each(|p| self.note_merged(p));
        Ok(pulls)
    }

    fn pull(&self, number: u64) -> Result<PullRequest, ApiError> {
        let key = number.to_string();
        let pull = match self.cache.read::<PullRequest>("pull", &key) {
            Some(entry) => entry.payload,
            None => {
                let pull = self.inner.pull(number)?;
                if pull.is_merged() {
                    let _ = self.cache.write("pull", &key, self.now, &pull);
                }
                pull
            }
        };
        self.note_merged(&pull);
        Ok(pull)
    }

    fn reviews(&self, number: u64) -> Result<Vec<Review>, ApiError> {
        if self.is_merged(number) {
            self.forever("reviews", &number.to_string(), || self.inner.reviews(number))
        } else {
            self.inner.reviews(number)
        }
    }

    fn user(&self, login: &str) -> Result<User, ApiError> {
        self.fresh("user", login, || self.inner.user(login))
    }

    fn permission(&self, login: &str) -> Result<Permission, ApiError> {
        self.fresh("permission", login, || self.inner.permission(login))
    }

    fn pull_commits(&self, number: u64) -> Result<Vec<Commit>, ApiError> {
        if self.is_merged(number) {
            self.forever("pull-commits", &number.to_string(), || self.inner.pull_commits(number))
        } else {
            self.inner.pull_commits(number)
        }
    }

    fn commit(&self, sha: &str) -> Result<Commit, ApiError> {
        self.forever("commit", sha, || self.inner.commit(sha))
    }

    fn blob_at(&self, path: &str, git_ref: &str) -> Result<Option<String>, ApiError> {
        let key = blob_key(path, git_ref);
        if let Some(entry) = self.cache.read::<String>("blob", &key) {
            return Ok(Some(entry.payload));
        }
        let blob = self.inner.blob_at(path, git_ref)?;
        // Only a found blob is immutable; "not found" is asked again next time.
        if let Some(sha) = &blob {
            let _ = self.cache.write("blob", &key, self.now, sha);
        }
        Ok(blob)
    }

    fn workflow_runs(&self, workflow_file: &str, filter: &RunFilter<'_>) -> Result<Vec<WorkflowRun>, ApiError> {
        self.inner.workflow_runs(workflow_file, filter)
    }

    fn artifacts(&self, run_id: u64) -> Result<Vec<Artifact>, ApiError> {
        self.inner.artifacts(run_id)
    }

    fn download_artifact(&self, run_id: u64, artifact_id: u64) -> Result<Vec<u8>, ApiError> {
        let key = format!("run-{run_id}-artifact-{artifact_id}");
        if let Some(bytes) = self.cache.read_bytes("artifact", &key) {
            return Ok(bytes);
        }
        let bytes = self.inner.download_artifact(run_id, artifact_id)?;
        let _ = self.cache.write_bytes("artifact", &key, self.now, &bytes);
        Ok(bytes)
    }

    fn branch(&self, name: &str) -> Result<Branch, ApiError> {
        self.inner.branch(name)
    }

    fn branch_rules(&self, name: &str) -> Result<Vec<BranchRule>, ApiError> {
        self.inner.branch_rules(name)
    }

    fn issue(&self, number: u64) -> Result<Issue, ApiError> {
        self.inner.issue(number)
    }
}
