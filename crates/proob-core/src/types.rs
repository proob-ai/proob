//! The closed data types of the verified core.
//!
//! Every value that crosses into `derive`, `verdict`, `policy`, and
//! `eligibility` has one of these shapes. The shapes are chosen so that an
//! incoherent state has no constructor: a declaration axis and a binding axis
//! live inside one `Scoped` status that carries the single scope hash both
//! compare against; `ScopeEmpty` has no evidence fields at all; evidence from
//! the default branch (`Evidence`) and the result of this PR's own run
//! (`RunResult`) are different types and nothing converts one to the other.
//!
//! Data is plain: ids are `u128`, digests `[u8; 32]`, paths byte vectors, times
//! `u64` seconds since the Unix epoch, counts `u32` or `u64`. Nothing here is a
//! string, so nothing here is parsed; the facts crate parses and converts.
//!
//! Each type's comment states the invariant it carries, not its mechanics. The
//! predicates that make those invariants checkable are in `specs.rs`.
use crate::scope_hash::PathDigest;
use vstd::prelude::*;

verus! {

// ---------------------------------------------------------------------------
// Scalars
// ---------------------------------------------------------------------------

/// A SHA-256 digest: content, scope, binding, and spec hashes are all this wide.
pub type Digest = [u8; 32];

/// A git object id, left-aligned: a SHA-1 id fills the first 20 bytes and the
/// rest are zero; a SHA-256 id fills all 32.
pub type Oid = [u8; 32];

/// Whole seconds since the Unix epoch. Never read from a clock inside the
/// verified core; always handed in as data.
pub type Secs = u64;

/// A property id: the 128-bit value of its ULID. Every `u128` is a valid ULID
/// value, so no range predicate is needed; the textual form is parsed in the
/// facts crate.
pub type PropertyId = u128;

// ---------------------------------------------------------------------------
// Approval: what GitHub's review records say about one governed file
// ---------------------------------------------------------------------------

/// Who vouches for a governed file's current content.
///
/// `Team`: an eligible reviewer approved it. `SelfSigned`: nobody eligible did,
/// and the repository allows a merge without one to count as a weaker
/// signature. `SelfSigned` never renders `Holds` and never `Proposed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Sig {
    Team,
    SelfSigned,
}

/// Why no review on the introducing PR was eligible.
///
/// Each variant is the first rule that disqualified the best candidate, in the
/// order the eligibility rule checks them, so a reviewer can act on it;
/// `eligibility.rs` fixes both orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Ineligible {
    /// No approving review at all.
    NoReview,
    /// Every approver authored or committed a non-merge commit that added or
    /// modified a governed file in the PR.
    ApproverEditedGoverned,
    /// Every approver reviewed a commit at which the file's blob differs from
    /// its blob at merge, or one whose blob can no longer be fetched.
    ApprovalPredatesChange,
    /// Every approver is a bot account.
    BotOnly,
    /// Every approver lacks write access at derivation time.
    NoWriteAccess,
    /// A reviewer's identity, or a governed commit's author, is unresolvable;
    /// this fails closed.
    UnknownIdentity,
}

/// Why a governed file is not approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum UnreviewedReason {
    /// The introducing commit on the default branch has no associated PR.
    DirectPush,
    /// A PR introduced it but no review on that PR was eligible.
    NoEligibleReview(Ineligible),
    /// It was approved, and the approver has since lost write access.
    ReviewerAccessRevoked,
}

/// The approval state of one governed file: a property file, a bound check's
/// binding set, `proob/config.toml`, or a proob workflow file.
///
/// Derived at read time from GitHub's review records by the verified
/// `eligibility` function. Never stored, never cached, never read from any
/// file in the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Approval {
    Approved { sig: Sig, pr: u64, merge_sha: Oid },
    Unreviewed(UnreviewedReason),
}

// ---------------------------------------------------------------------------
// Governance: repository-level facts the verdict reads
// ---------------------------------------------------------------------------

/// A fact GitHub may decline to tell us. `Unknown` is a standing warning, never
/// a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Known {
    Yes,
    No,
    Unknown,
}

/// Repository governance as the verdict sees it.
///
/// The three approvals are for the config file and the two proob workflow
/// files on the default branch. `floor` is the kill floor in force: at least
/// one by schema, which `GateConfig`'s well-formedness predicate states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct Governance {
    pub config: Approval,
    pub evidence_workflow: Approval,
    pub gate_workflow: Approval,
    pub branch_requires_review: Known,
    pub gate_required: Known,
    pub codeowners: Known,
    pub floor: u32,
}

// ---------------------------------------------------------------------------
// Status: the declaration axis and the binding axis
// ---------------------------------------------------------------------------

/// What kind of primary evidence a property's bound check produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Kind {
    /// A property-based test: positive evidence is `NoCounterexample`.
    Test,
    /// A machine-checked proof: positive evidence is `Verified`.
    Proof,
    /// A model checked against a spec: positive evidence is `NoViolation`.
    Model,
}

/// Why a declaration is not approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum ProposedWhy {
    /// No content of this property file was ever approved.
    NeverReviewed,
    /// An earlier content was approved; the current content is not.
    Amended,
    /// The current content was approved, and the approver has since lost
    /// write access (`UnreviewedReason::ReviewerAccessRevoked`). Renders
    /// `Re-approve (reviewer access revoked)`, never `Proposed`.
    ReviewerAccessRevoked,
}

/// Commits that touched the property's scope since its approving merge, and
/// the days spanned. Informational for test- and proof-backed properties;
/// for model-backed properties a non-zero count demotes the verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct Drift {
    pub commits: u64,
    pub days: u64,
}

/// A waiver's window at the instant of derivation.
///
/// `Active` means the instant is before `until`; `issue_open` is what the
/// issues API said about the waiver's issue. `Expired` carries no issue state
/// because an expired waiver is `Broken` whatever the issue says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Window {
    Active { until: Secs, issue_open: bool },
    Expired { until: Secs },
}

/// The declaration axis: what the team has said about the statement.
///
/// `since` is the merge commit of the approving PR. A waiver is content, so it
/// carries the same signature as an approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Decl {
    Proposed(ProposedWhy),
    Approved { sig: Sig, since: Oid, drift: Drift },
    Waived { sig: Sig, since: Oid, window: Window },
}

/// The outcome of one run of a property's primary check.
///
/// Flat across kinds; `specs::positive` says which outcome counts as positive
/// for which kind, and a kind mismatch is never positive. Carries no witness,
/// diagnostics, or versions: those stay in the facts crate's records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum PrimaryOutcome {
    NoCounterexample,
    Counterexample,
    Verified,
    ProofFailed,
    NoViolation,
    Violation,
    Errored,
}

/// The outcome of one mutation-scoring run with only the bound check.
///
/// `NoMutants` never satisfies the kill floor; an errored run never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum ScoreOutcome {
    Scored { killed: u32, survived: u32, timeout: u32, unviable: u32 },
    NoMutants,
    Errored,
}

/// Default-branch primary evidence relative to the current hashes.
///
/// `Fresh` is produced by `derive` only when the record's scope hash and
/// binding hash both equal the current ones (U3's `ensures`). `Pending` means a
/// newer merge touched the scope and the push run has not reported yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Primary {
    NoEvidence,
    Pending,
    Stale,
    Fresh(PrimaryOutcome),
}

/// This PR's own run, check mode only. A negative result here is `Broken` for
/// this PR; a positive one changes nothing and can never contribute to `Holds`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Provisional {
    None,
    Provisional(PrimaryOutcome),
}

/// Default-branch score evidence relative to the current binding hash and the
/// freshness window. `Fresh` is produced by `derive` only when the binding
/// hash matches and the record's age is within the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Score {
    NoEvidence,
    Stale,
    Fresh(ScoreOutcome),
}

/// Why no check is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum UnboundWhy {
    /// The repository's language has no adapter.
    NoAdapter,
    /// An adapter exists and found no check carrying this property's token.
    NoCheck,
}

/// The binding axis: the check bound to the property, if any, with its own
/// approval and the evidence about it.
///
/// `binding_hash` is the hash of the binding set (the check and everything
/// that can change its meaning); `approval` is the approval of that set;
/// `previously_approved` says an earlier content of the set was approved, so
/// an `Unreviewed` approval with it set renders `Re-approve (check amended)`
/// and without it `Not checked (check unreviewed)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Binding {
    Unbound(UnboundWhy),
    Bound {
        binding_hash: Digest,
        approval: Approval,
        previously_approved: bool,
        primary: Primary,
        provisional: Provisional,
        score: Score,
    },
}

/// The derived status of one property that exists on the default branch.
///
/// `ScopeEmpty`: the scope globs match nothing in the tree, so there is no
/// scope hash and no evidence can be about anything. `Scoped`: the one scope
/// hash both axes compare against, the kind the bound check (or the file)
/// declares, and the two axes. A deleted property file has no status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Status {
    ScopeEmpty { decl: Decl },
    Scoped { kind: Kind, scope_hash: Digest, decl: Decl, binding: Binding },
}

// ---------------------------------------------------------------------------
// Verdict, outcome, policy
// ---------------------------------------------------------------------------

/// Why an approved property is not checked.
///
/// `GitHubUnreachable` and `RateLimited` are never produced by `verdict`: they
/// are set by the CLI (U4) from an `Unverified` view, where no facts exist to
/// derive from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum NotCheckedWhy {
    NoAdapter,
    NoCheck,
    /// A check is bound and no content of its binding set was ever approved.
    CheckUnreviewed,
    EvidencePending,
    EvidenceStale,
    AdapterErrored,
    GitHubUnreachable,
    RateLimited,
}

/// What changed since the last approval, or why an approval no longer counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum ReApproveWhy {
    StatementAmended,
    CheckAmended,
    ConfigAmended,
    WorkflowAmended,
    ReviewerAccessRevoked,
    /// Model-backed only: commits to the modelled scope since the spec's
    /// approval.
    SpecDrifted(u64),
}

/// Where a negative result came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Source {
    ThisPr,
    DefaultBranch,
}

/// Why a property is broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum BrokenWhy {
    /// A counterexample, failed proof, or violation.
    Negative(Source),
    /// The runner errored on this PR's head.
    RunnerErrored,
    /// The scope matches nothing.
    ScopeEmpty,
    /// The waiver window has passed, or its issue is closed.
    WaiverExpired,
}

/// The closed set of words a status can render as.
///
/// `Holds` is reachable only through `specs::holds_requirements`, which a
/// test-backed property never meets. `Checked` carries at least one modifier
/// (`specs::verdict_wf`): `tested` (the evidence is a test, not a defect),
/// `strength_pending`, or `self_signed`; with none it would be `Holds`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Verdict {
    Holds,
    Checked { tested: bool, strength_pending: bool, self_signed: bool },
    NotChecked { reason: NotCheckedWhy, self_signed: bool },
    Proposed,
    ReApprove(ReApproveWhy),
    Broken(BrokenWhy),
    Waived { until: Secs },
}

/// What the gate does with one verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Outcome {
    Pass,
    Warn,
    Block,
}

/// Whether an approved-but-unchecked property is still inside the grace window
/// that follows its approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Grace {
    Within,
    After,
}

/// The gate policy: a total table over `Verdict`, split where the default
/// table distinguishes cases (grace for `NotChecked`, affectedness for
/// `Broken`). Totality is by construction: every row is a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct Policy {
    pub holds: Outcome,
    /// `Checked` carrying a defect modifier: `strength_pending` or `self_signed`.
    pub checked: Outcome,
    /// `Checked` whose only modifier is `tested`.
    pub tested: Outcome,
    pub proposed: Outcome,
    pub waived: Outcome,
    pub not_checked_within_grace: Outcome,
    pub not_checked_after_grace: Outcome,
    pub re_approve: Outcome,
    pub broken_affected: Outcome,
    pub broken_unaffected: Outcome,
}

/// The slice of `proob/config.toml` the verified core reads. Applied only when
/// the config file is approved; otherwise the built-in defaults replace it.
///
/// Invariant (`specs::gate_config_wf`): `floor >= 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct GateConfig {
    pub policy: Policy,
    pub grace_secs: Secs,
    pub freshness_secs: Secs,
    pub floor: u32,
    pub allow_self_signed: bool,
}

// ---------------------------------------------------------------------------
// Property: the governed content of one property file, as the core sees it
// ---------------------------------------------------------------------------

/// How much a violation matters. Rendering only; never changes a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// A waiver as content: the window's end. The issue reference stays in the
/// facts crate; whether it is open arrives as a fact in `Window::Active`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct Waiver {
    pub until: Secs,
}

/// The parts of a property file the core reads. Title, statement, rationale,
/// and the waiver's issue and reason are prose and stay in the facts crate.
///
/// Invariant (`specs::property_wf`): `scope` is non-empty and every glob
/// satisfies `grammar::glob_ok`. `kind` absent means the bound check's adapter
/// decides.
#[derive(Debug, PartialEq, Eq)]
pub struct Property {
    pub id: PropertyId,
    pub kind: Option<Kind>,
    pub scope: Vec<Vec<u8>>,
    pub severity: Severity,
    pub waiver: Option<Waiver>,
    pub reviewed: Option<Secs>,
}

// ---------------------------------------------------------------------------
// Evidence and run results: same measurements, different provenance
// ---------------------------------------------------------------------------

/// What one run measured: a kind's primary check, or the strength score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Measured {
    Primary(Kind, PrimaryOutcome),
    Score(ScoreOutcome),
}

/// A record from an artifact of an accepted run on the default branch.
///
/// Provenance is in the type: the facts crate builds one only from an artifact
/// whose run passed U9's acceptance predicate (evidence workflow path, default
/// branch, push/schedule/dispatch event, successful conclusion, approved
/// workflow blob). `run_id`, `head_sha`, and `workflow_blob` tie it to that run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct Evidence {
    pub property: PropertyId,
    pub content_hash: Digest,
    pub binding_hash: Digest,
    pub scope_hash: Digest,
    pub spec_hash: Option<Digest>,
    pub measured: Measured,
    pub run_id: u64,
    pub head_sha: Oid,
    pub workflow_blob: Oid,
    pub at: Secs,
}

/// A result from this PR's run or a local run. It enters `derive` only as the
/// `Provisional` field of a binding and renders as provisional or local. No
/// function in any crate converts a `RunResult` into an `Evidence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct RunResult {
    pub property: PropertyId,
    pub content_hash: Digest,
    pub binding_hash: Digest,
    pub scope_hash: Digest,
    pub measured: Measured,
}

// ---------------------------------------------------------------------------
// Facts for derive (read by U3's `derive`; populated by U9 and U4)
// ---------------------------------------------------------------------------

/// The bound check as the facts layer found it: the hash of its binding set,
/// that set's approval, and whether an earlier content of the set was approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct BindingFacts {
    pub binding_hash: Digest,
    pub approval: Approval,
    pub previously_approved: bool,
}

/// Everything `derive` reads about one property. Facts only: no field is an
/// `Approval` of anything but a governed file's content, and nothing here is a
/// `Status` or `Verdict`.
///
/// `scope` is the resolved file list (glob resolution is the effects layer's
/// job): empty means `ScopeEmpty`, and its paths must be pairwise distinct
/// (`scope_hash::paths_distinct`, decidable by `paths_distinct_exec`), which
/// resolved files are by construction. `kind` is the file's kind or, when the
/// file names none, the adapter's. `statement` is the approval of the property
/// file's current content and `previously_approved` says an earlier content was
/// approved. `drift` counts commits (and days) touching the scope since the
/// approving merge, zero when unapproved. `records` is every default-branch
/// evidence record for this id, any kind; `evidence_run_head` is the head of the
/// latest accepted evidence run, `None` when there is none; `provisional` is
/// this PR's own run, check mode only.
#[derive(Debug, PartialEq, Eq)]
pub struct DeriveInput {
    pub property: Property,
    pub kind: Kind,
    pub scope: Vec<PathDigest>,
    pub statement: Approval,
    pub previously_approved: bool,
    pub drift: Drift,
    pub waiver_issue_open: bool,
    pub binding: Option<BindingFacts>,
    pub adapter_available: bool,
    pub records: Vec<Evidence>,
    pub evidence_run_head: Option<Oid>,
    pub default_head: Oid,
    pub provisional: Option<RunResult>,
}

// ---------------------------------------------------------------------------
// Facts for eligibility (read by U9's `eligibility`)
// ---------------------------------------------------------------------------

/// What GitHub says an account is. `Human` is `type: User` with a login that
/// does not end in `[bot]`; anything unresolvable is `Unknown`, which never
/// counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum UserKind {
    Human,
    Bot,
    Unknown,
}

/// GitHub's review states, closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Pending,
}

/// The reviewer behind a review, with the permission fact read now, not at
/// review time (GitHub keeps no record of the latter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct Reviewer {
    pub id: u64,
    pub kind: UserKind,
    pub has_write_now: bool,
}

/// One reviewer's latest review on the introducing PR.
///
/// `blob_at_commit` is the governed file's blob id at the reviewed commit, or
/// `None` when that commit's blob can no longer be fetched (such a review does
/// not count).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct Review {
    pub user: Reviewer,
    pub state: ReviewState,
    pub blob_at_commit: Option<Oid>,
}

/// The facts about the PR that introduced one governed file's current content.
///
/// `governed_change_authors` lists every account that authored or committed a
/// non-merge commit in the PR that added or modified a governed file
/// (deletions exempt, co-author trailers ignored). `unresolved_governed_author`
/// is set when any such author could not be resolved; it fails closed.
#[derive(Debug, PartialEq, Eq)]
pub struct PrFacts {
    pub number: u64,
    pub merge_sha: Oid,
    pub author: u64,
    pub governed_change_authors: Vec<u64>,
    pub unresolved_governed_author: bool,
    pub blob_at_merge: Oid,
    pub reviews: Vec<Review>,
}

/// How a governed file's current content reached the default branch.
#[derive(Debug, PartialEq, Eq)]
pub enum Introduction {
    DirectPush,
    Merged(PrFacts),
}

// ---------------------------------------------------------------------------
// Facts for evidence acceptance (read by U9's `accept_run`)
// ---------------------------------------------------------------------------

/// The GitHub event that started a workflow run. Only `Push`, `Schedule`, and
/// `WorkflowDispatch` can produce evidence: a pull-request event runs the PR's
/// own code, and anything GitHub adds is `Other` and never counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum RunEvent {
    Push,
    Schedule,
    WorkflowDispatch,
    PullRequest,
    Other,
}

/// Whether a run concluded successfully. Failure, cancellation, time-out, a
/// run still in progress, and anything GitHub adds are all `NotSuccess`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum RunConclusion {
    Success,
    NotSuccess,
}

/// What GitHub and git say about one workflow run.
///
/// `workflow_blob` is the blob id of the workflow file at `head_sha`, read
/// from the repository's history, or `None` when that commit's tree cannot be
/// read; such a run is never accepted. `head_branch` is `None` when GitHub
/// reports none (a run on a detached commit).
#[derive(Debug, PartialEq, Eq)]
pub struct RunFacts {
    pub run_id: u64,
    pub workflow_path: Vec<u8>,
    pub head_branch: Option<Vec<u8>>,
    pub head_sha: Oid,
    pub event: RunEvent,
    pub conclusion: RunConclusion,
    pub workflow_blob: Option<Oid>,
}

/// What an accepted evidence run must match: the evidence workflow's path,
/// the default branch, and the one workflow blob whose approval is
/// `workflow_approval` (the blob on the default branch at derivation time). A
/// run that executed any other blob, even one approved earlier, is not
/// accepted: evidence is only as trusted as the workflow that is approved now.
#[derive(Debug, PartialEq, Eq)]
pub struct RunExpectation {
    pub workflow_path: Vec<u8>,
    pub default_branch: Vec<u8>,
    pub workflow_blob: Oid,
    pub workflow_approval: Approval,
}

} // verus!
