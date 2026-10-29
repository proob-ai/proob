//! The GitHub REST payloads the facts layer reads, reduced to the fields it
//! uses. GitHub adds fields freely, so these never deny unknown fields;
//! closed enums keep an `Unknown` arm for values GitHub may add, and the
//! conversion treats `Unknown` as a reason to fail closed. They serialize too,
//! because the fact cache stores them as fetched; there is no payload here,
//! and none anywhere, for an `Approval`, `Status`, or `Verdict`.
use serde::{Deserialize, Serialize};

/// An account as every payload embeds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub login: String,
    pub id: u64,
    #[serde(rename = "type")]
    pub kind: AccountType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountType {
    User,
    Bot,
    Organization,
    #[serde(other)]
    Unknown,
}

/// `GET /repos/{owner}/{repo}/pulls/{number}`, and each element of
/// `GET /repos/{owner}/{repo}/commits/{sha}/pulls`. The list form carries
/// `merged_at` but not `merged`; `is_merged` reads both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    /// `None` for a deleted account.
    pub user: Option<User>,
    #[serde(default)]
    pub merged: bool,
    #[serde(default)]
    pub merged_at: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub base: GitRef,
    pub head: GitRef,
}

impl PullRequest {
    pub fn is_merged(&self) -> bool {
        self.merged || self.merged_at.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRef {
    #[serde(rename = "ref")]
    pub name: String,
    pub sha: String,
}

/// Each element of `GET /repos/{owner}/{repo}/pulls/{number}/reviews`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub id: u64,
    pub user: Option<User>,
    pub state: ReviewState,
    /// The commit the review was submitted against.
    pub commit_id: Option<String>,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Pending,
    #[serde(other)]
    Unknown,
}

/// `GET /repos/{owner}/{repo}/collaborators/{username}/permission`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permission {
    pub permission: PermissionLevel,
    /// The finer role: `maintain` and `triage` show here, with `permission`
    /// reporting `write` and `read` respectively.
    #[serde(default)]
    pub role_name: Option<String>,
    pub user: Option<User>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    Admin,
    Write,
    Read,
    None,
    #[serde(other)]
    Unknown,
}

/// `GET /repos/{owner}/{repo}/commits/{sha}` (with `files`), and each element
/// of `GET /repos/{owner}/{repo}/pulls/{number}/commits` (without). `author`
/// and `committer` are the GitHub accounts GitHub could resolve from the git
/// identities; `None` means unresolvable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub sha: String,
    pub author: Option<User>,
    pub committer: Option<User>,
    #[serde(default)]
    pub parents: Vec<Parent>,
    #[serde(default)]
    pub files: Vec<CommitFile>,
}

impl Commit {
    /// More than one parent.
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parent {
    pub sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitFile {
    pub filename: String,
    pub status: FileStatus,
    #[serde(default)]
    pub previous_filename: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Added,
    Modified,
    Removed,
    Renamed,
    Copied,
    Changed,
    Unchanged,
    #[serde(other)]
    Unknown,
}

impl FileStatus {
    /// The file's content at this commit is new or different: everything but
    /// a removal or no change. `Unknown` fails closed as a change.
    pub fn adds_or_modifies(self) -> bool {
        !matches!(self, FileStatus::Removed | FileStatus::Unchanged)
    }
}

/// `GET /repos/{owner}/{repo}/contents/{path}?ref=...` for a file: only the
/// blob id is read. A directory answers an array and a symlink or submodule a
/// different `type`; the client maps both to "no blob".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contents {
    pub sha: String,
    #[serde(rename = "type")]
    pub kind: String,
}

/// `GET /repos/{owner}/{repo}/actions/workflows/{file}/runs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRuns {
    pub total_count: u64,
    pub workflow_runs: Vec<WorkflowRun>,
}

/// Each element of the runs list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    /// The workflow file, such as `.github/workflows/proob-evidence.yml`.
    pub path: String,
    pub head_branch: Option<String>,
    pub head_sha: String,
    pub event: RunEvent,
    pub conclusion: Option<RunConclusion>,
    /// RFC 3339, as GitHub writes it.
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEvent {
    Push,
    Schedule,
    WorkflowDispatch,
    PullRequest,
    PullRequestTarget,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunConclusion {
    Success,
    Failure,
    Cancelled,
    Skipped,
    TimedOut,
    ActionRequired,
    Neutral,
    Stale,
    StartupFailure,
    #[serde(other)]
    Unknown,
}

/// `GET /repos/{owner}/{repo}/actions/runs/{run_id}/artifacts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifacts {
    pub total_count: u64,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub size_in_bytes: u64,
    #[serde(default)]
    pub expired: bool,
}

/// `GET /repos/{owner}/{repo}/branches/{branch}`: the `protected` flag is
/// readable with the default token; the protection's details are not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    #[serde(default)]
    pub protected: bool,
}

/// Each element of `GET /repos/{owner}/{repo}/rules/branches/{branch}`: the
/// rules in force from rulesets, readable with the default token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchRule {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub parameters: Option<RuleParameters>,
}

/// The parameters this crate reads; every other rule's parameters are kept
/// as unknown fields and ignored.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RuleParameters {
    #[serde(default)]
    pub required_status_checks: Vec<RequiredCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredCheck {
    pub context: String,
}

/// `GET /repos/{owner}/{repo}/issues/{number}`: a waiver's issue must be open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub state: IssueState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueState {
    Open,
    Closed,
    #[serde(other)]
    Unknown,
}
