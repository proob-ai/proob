//! Assembling an `Introduction` for a governed file, or a binding set, from
//! GitHub's records. Everything here is a fact lookup; the judgement is the
//! verified `eligibility`.
//!
//! For a file whose current content arrived in commit `C` on the default
//! branch (the last first-parent commit that touched it, found by git):
//!
//! 1. The PRs associated with `C` are listed. The introducing PR is one that
//!    is merged into the default branch, preferring the one whose merge
//!    commit is `C`. All three merge strategies arrive this way: a merge
//!    commit is `C` itself, a squash commit is `C`, and a rebased commit is
//!    associated with its PR by GitHub (U14 confirms this live). No such PR
//!    is `Introduction::DirectPush`.
//! 2. Each reviewer's latest review that approves, requests changes, or was
//!    dismissed is kept; comments and pending reviews change nothing, as on
//!    GitHub. A reviewer is `Human` when `type: User` and the login does not
//!    end in `[bot]`; write access is read now, for approving humans only.
//! 3. The blob of the file at each approving review's commit is read through
//!    the contents API; a commit GitHub no longer has gives `None`.
//! 4. Every non-merge commit of the PR is read with its files; the authors
//!    and committers of those that added or modified a governed file are the
//!    governed-change authors (deletions exempt; co-author trailers are not
//!    read); an author GitHub cannot resolve sets `unresolved_governed_author`.
//!
//! A binding set is several files approved as one: its "blob" is `set_oid`
//! over the members' blobs, so the same `PrFacts` shape and the same
//! verified rule apply, and a reviewed commit counts only when every member
//! had its merged content there.
use crate::error::FactsError;
use crate::github::GitHubApi;
use proob_core::scope_hash::{PathDigest, ScopeHash, scope_hash};
use proob_core::types::{Introduction, Oid, PrFacts, Review, ReviewState, Reviewer, UserKind};
use proob_facts::convert;
use proob_facts::github::{self, PullRequest};
use proob_facts::hex;
use proob_facts::paths::is_fixed_governed;
use std::collections::{BTreeMap, BTreeSet};

/// What the eligibility rule treats as governed: the fixed paths plus the
/// members of every bound check's binding set (supplied by the adapters).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GovernedSet {
    binding_files: BTreeSet<String>,
}

impl GovernedSet {
    pub fn new(binding_files: impl IntoIterator<Item = String>) -> GovernedSet {
        GovernedSet { binding_files: binding_files.into_iter().collect() }
    }

    pub fn is_governed(&self, path: &str) -> bool {
        is_fixed_governed(path) || self.binding_files.contains(path)
    }
}

/// Where a governed file's (or set's) current content came from on the
/// default branch: the commit git found, and the content's blob there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntroducingCommit {
    pub sha: Oid,
    pub blob_at_merge: Oid,
}

/// The identity of a set of blobs: the blob itself for one file, the scope
/// hash of (path, blob) pairs for several. `None` when two paths are equal.
pub fn set_oid(pairs: &[(String, Oid)]) -> Option<Oid> {
    match pairs {
        [(_, only)] => Some(*only),
        many => {
            let list = many.iter().map(|(p, d)| PathDigest { path: p.as_bytes().to_vec(), digest: *d }).collect();
            match scope_hash(list) {
                ScopeHash::Ok(h) => Some(h),
                ScopeHash::DuplicatePath => None,
            }
        }
    }
}

/// The `Introduction` of `paths` whose current content arrived in `commit`.
pub fn introduction(
    api: &dyn GitHubApi,
    governed: &GovernedSet,
    default_branch: &str,
    paths: &[&str],
    commit: &IntroducingCommit,
) -> Result<Introduction, FactsError> {
    let sha = hex::encode_oid(&commit.sha);
    let pulls = api.pulls_for_commit(&sha)?;
    match introducing_pr(&pulls, default_branch, &sha) {
        None => Ok(Introduction::DirectPush),
        Some(pr) => Ok(Introduction::Merged(pr_facts(api, governed, pr, paths, commit.sha, commit.blob_at_merge)?)),
    }
}

/// The `Introduction` `paths` would have if `pr` merged now: its head is the
/// merge. Reviews, permissions, and blobs are read live.
pub fn introduction_if_merged(
    api: &dyn GitHubApi,
    governed: &GovernedSet,
    default_branch: &str,
    paths: &[&str],
    pr: &PullRequest,
) -> Result<Introduction, FactsError> {
    if pr.base.name != default_branch {
        return Err(FactsError::NotIntoDefaultBranch { number: pr.number, base: pr.base.name.clone() });
    }
    let head = convert::oid("head.sha", &pr.head.sha)?;
    let Some(blob_at_head) = blobs_at(api, paths, &pr.head.sha)? else {
        // The head has no such file: nothing governed would merge, so no
        // review can be about it. Read as a merge introducing nothing.
        return Ok(Introduction::DirectPush);
    };
    Ok(Introduction::Merged(pr_facts(api, governed, pr, paths, head, blob_at_head)?))
}

/// A merged PR into the default branch, preferring the one whose merge commit is `sha`.
fn introducing_pr<'a>(pulls: &'a [PullRequest], default_branch: &str, sha: &str) -> Option<&'a PullRequest> {
    let candidates = || pulls.iter().filter(|p| p.is_merged() && p.base.name == default_branch);
    candidates().find(|p| p.merge_commit_sha.as_deref() == Some(sha)).or_else(|| candidates().next())
}

fn pr_facts(
    api: &dyn GitHubApi,
    governed: &GovernedSet,
    pr: &PullRequest,
    paths: &[&str],
    merge_sha: Oid,
    blob_at_merge: Oid,
) -> Result<PrFacts, FactsError> {
    let (author, author_unresolved) = match &pr.user {
        Some(u) => (u.id, false),
        None => (0, true),
    };
    let reviews = reviews_of(api, pr.number, paths)?;
    let changes = governed_changes(api, governed, pr.number)?;
    Ok(PrFacts {
        number: pr.number,
        merge_sha,
        author,
        governed_change_authors: changes.authors.into_iter().collect(),
        unresolved_governed_author: author_unresolved || changes.unresolved,
        blob_at_merge,
        reviews,
    })
}

// ---------------------------------------------------------------------------
// Reviews
// ---------------------------------------------------------------------------

/// Each reviewer's latest review that approves, requests changes, or was
/// dismissed, in review-id order, as core facts.
fn reviews_of(api: &dyn GitHubApi, number: u64, paths: &[&str]) -> Result<Vec<Review>, FactsError> {
    let mut all = api.reviews(number)?;
    all.sort_by_key(|r| r.id);
    latest_per_reviewer(&all).into_iter().map(|r| review_facts(api, r, paths)).collect()
}

fn latest_per_reviewer(reviews: &[github::Review]) -> Vec<&github::Review> {
    let decisive = |r: &&github::Review| {
        matches!(
            r.state,
            github::ReviewState::Approved | github::ReviewState::ChangesRequested | github::ReviewState::Dismissed
        )
    };
    // Keyed by account id; a review from a deleted account is its own key.
    let mut latest: BTreeMap<(u8, u64), &github::Review> = BTreeMap::new();
    for r in reviews.iter().filter(decisive) {
        let key = match &r.user {
            Some(u) => (0, u.id),
            None => (1, r.id),
        };
        latest.insert(key, r);
    }
    latest.into_values().collect()
}

fn review_facts(api: &dyn GitHubApi, r: &github::Review, paths: &[&str]) -> Result<Review, FactsError> {
    let kind = convert::user_kind(r.user.as_ref());
    let state = convert::review_state(r.state).unwrap_or(ReviewState::Pending);
    let approving = state == ReviewState::Approved;
    // Permission and blob are read for approving humans only: no other review
    // can count, and each lookup spends budget.
    let has_write_now = match (&r.user, approving, kind) {
        (Some(u), true, UserKind::Human) => has_write(api, &u.login)?,
        _ => false,
    };
    let blob_at_commit = match (&r.commit_id, approving) {
        (Some(commit), true) => blobs_at(api, paths, commit)?,
        _ => None,
    };
    Ok(Review {
        user: Reviewer { id: r.user.as_ref().map_or(0, |u| u.id), kind, has_write_now },
        state,
        blob_at_commit,
    })
}

/// Write access now; a login GitHub does not list as a collaborator has none.
fn has_write(api: &dyn GitHubApi, login: &str) -> Result<bool, FactsError> {
    match api.permission(login) {
        Ok(p) => Ok(convert::has_write(&p)),
        Err(crate::error::ApiError::NotFound { .. }) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// The set identity of `paths` at `git_ref`, `None` when any member is absent there.
fn blobs_at(api: &dyn GitHubApi, paths: &[&str], git_ref: &str) -> Result<Option<Oid>, FactsError> {
    let mut pairs = Vec::with_capacity(paths.len());
    for path in paths {
        match api.blob_at(path, git_ref)? {
            Some(sha) => pairs.push((path.to_string(), convert::oid("sha", &sha)?)),
            None => return Ok(None),
        }
    }
    match set_oid(&pairs) {
        Some(oid) => Ok(Some(oid)),
        None => Err(FactsError::DuplicatePath { path: duplicate(paths) }),
    }
}

fn duplicate(paths: &[&str]) -> String {
    let mut seen = BTreeSet::new();
    paths.iter().find(|p| !seen.insert(**p)).map_or_else(String::new, |p| p.to_string())
}

// ---------------------------------------------------------------------------
// Governed changes
// ---------------------------------------------------------------------------

struct GovernedChanges {
    authors: BTreeSet<u64>,
    unresolved: bool,
}

/// Authors and committers of the PR's non-merge commits that added or
/// modified a governed file.
fn governed_changes(api: &dyn GitHubApi, governed: &GovernedSet, number: u64) -> Result<GovernedChanges, FactsError> {
    let mut changes = GovernedChanges { authors: BTreeSet::new(), unresolved: false };
    for summary in api.pull_commits(number)?.iter().filter(|c| !c.is_merge()) {
        let commit = api.commit(&summary.sha)?;
        if commit.is_merge() || !touches_governed(&commit, governed) {
            continue;
        }
        for account in [&commit.author, &commit.committer] {
            match account {
                Some(u) => {
                    changes.authors.insert(u.id);
                }
                None => changes.unresolved = true,
            }
        }
    }
    Ok(changes)
}

fn touches_governed(commit: &github::Commit, governed: &GovernedSet) -> bool {
    commit.files.iter().any(|f| f.status.adds_or_modifies() && governed.is_governed(&f.filename))
}
