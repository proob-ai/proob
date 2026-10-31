//! The JSON view of a report: the full `Status`, `Verdict`, reason, this
//! PR's result, and governance, in shapes U10's judge job emits identically.
//!
//! Every type here derives `Serialize` only. There is deliberately no
//! `Deserialize` for a status, a verdict, or an approval anywhere in proob
//! (ASSERTIONS.md, assertion 6): this file is an output, never an input.
use crate::cli::report::*;
use crate::cli::vocab;
use proob_core::preview::Remedy;
use proob_core::types::*;
use proob_facts::evidence::{OutcomeRecord, Replay};
use proob_facts::hex;
use serde::Serialize;

pub const SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Core mirrors
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SigJ {
    Team,
    SelfSigned,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum IneligibleJ {
    NoReview,
    ApproverEditedGoverned,
    ApprovalPredatesChange,
    BotOnly,
    NoWriteAccess,
    UnknownIdentity,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum UnreviewedJ {
    DirectPush,
    NoEligibleReview { why: IneligibleJ },
    ReviewerAccessRevoked,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum ApprovalJ {
    Approved { sig: SigJ, pr: u64, merge_sha: String },
    Unreviewed { reason: UnreviewedJ },
}

impl From<Approval> for ApprovalJ {
    fn from(a: Approval) -> Self {
        match a {
            Approval::Approved { sig, pr, merge_sha } => ApprovalJ::Approved {
                sig: match sig {
                    Sig::Team => SigJ::Team,
                    Sig::SelfSigned => SigJ::SelfSigned,
                },
                pr,
                merge_sha: hex::encode_oid(&merge_sha),
            },
            Approval::Unreviewed(r) => ApprovalJ::Unreviewed {
                reason: match r {
                    UnreviewedReason::DirectPush => UnreviewedJ::DirectPush,
                    UnreviewedReason::ReviewerAccessRevoked => UnreviewedJ::ReviewerAccessRevoked,
                    UnreviewedReason::NoEligibleReview(why) => UnreviewedJ::NoEligibleReview {
                        why: match why {
                            Ineligible::NoReview => IneligibleJ::NoReview,
                            Ineligible::ApproverEditedGoverned => IneligibleJ::ApproverEditedGoverned,
                            Ineligible::ApprovalPredatesChange => IneligibleJ::ApprovalPredatesChange,
                            Ineligible::BotOnly => IneligibleJ::BotOnly,
                            Ineligible::NoWriteAccess => IneligibleJ::NoWriteAccess,
                            Ineligible::UnknownIdentity => IneligibleJ::UnknownIdentity,
                        },
                    },
                },
            },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum KnownJ {
    Yes,
    No,
    Unknown,
}

impl From<Known> for KnownJ {
    fn from(k: Known) -> Self {
        match k {
            Known::Yes => KnownJ::Yes,
            Known::No => KnownJ::No,
            Known::Unknown => KnownJ::Unknown,
        }
    }
}

#[derive(Serialize)]
struct GovernanceJ {
    config: ApprovalJ,
    evidence_workflow: ApprovalJ,
    gate_workflow: ApprovalJ,
    branch_requires_review: KnownJ,
    gate_required: KnownJ,
    codeowners: KnownJ,
    floor: u32,
}

impl From<&Governance> for GovernanceJ {
    fn from(g: &Governance) -> Self {
        GovernanceJ {
            config: g.config.into(),
            evidence_workflow: g.evidence_workflow.into(),
            gate_workflow: g.gate_workflow.into(),
            branch_requires_review: g.branch_requires_review.into(),
            gate_required: g.gate_required.into(),
            codeowners: g.codeowners.into(),
            floor: g.floor,
        }
    }
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum KindJ {
    Test,
    Proof,
    Model,
}

impl From<Kind> for KindJ {
    fn from(k: Kind) -> Self {
        match k {
            Kind::Test => KindJ::Test,
            Kind::Proof => KindJ::Proof,
            Kind::Model => KindJ::Model,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SeverityJ {
    Low,
    Medium,
    High,
    Critical,
}

impl From<Severity> for SeverityJ {
    fn from(s: Severity) -> Self {
        match s {
            Severity::Low => SeverityJ::Low,
            Severity::Medium => SeverityJ::Medium,
            Severity::High => SeverityJ::High,
            Severity::Critical => SeverityJ::Critical,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ProposedWhyJ {
    NeverReviewed,
    Amended,
    ReviewerAccessRevoked,
}

#[derive(Serialize)]
struct DriftJ {
    commits: u64,
    days: u64,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum WindowJ {
    Active { until: Secs, issue_open: bool },
    Expired { until: Secs },
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum DeclJ {
    Proposed { why: ProposedWhyJ },
    Approved { sig: SigJ, since: String, drift: DriftJ },
    Waived { sig: SigJ, since: String, window: WindowJ },
}

fn sig(s: Sig) -> SigJ {
    match s {
        Sig::Team => SigJ::Team,
        Sig::SelfSigned => SigJ::SelfSigned,
    }
}

impl From<Decl> for DeclJ {
    fn from(d: Decl) -> Self {
        match d {
            Decl::Proposed(why) => DeclJ::Proposed {
                why: match why {
                    ProposedWhy::NeverReviewed => ProposedWhyJ::NeverReviewed,
                    ProposedWhy::Amended => ProposedWhyJ::Amended,
                    ProposedWhy::ReviewerAccessRevoked => ProposedWhyJ::ReviewerAccessRevoked,
                },
            },
            Decl::Approved { sig: s, since, drift } => DeclJ::Approved {
                sig: sig(s),
                since: hex::encode_oid(&since),
                drift: DriftJ { commits: drift.commits, days: drift.days },
            },
            Decl::Waived { sig: s, since, window } => DeclJ::Waived {
                sig: sig(s),
                since: hex::encode_oid(&since),
                window: match window {
                    Window::Active { until, issue_open } => WindowJ::Active { until, issue_open },
                    Window::Expired { until } => WindowJ::Expired { until },
                },
            },
        }
    }
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum PrimaryOutcomeJ {
    NoCounterexample,
    Counterexample,
    Verified,
    ProofFailed,
    NoViolation,
    Violation,
    Errored,
}

impl From<PrimaryOutcome> for PrimaryOutcomeJ {
    fn from(o: PrimaryOutcome) -> Self {
        match o {
            PrimaryOutcome::NoCounterexample => PrimaryOutcomeJ::NoCounterexample,
            PrimaryOutcome::Counterexample => PrimaryOutcomeJ::Counterexample,
            PrimaryOutcome::Verified => PrimaryOutcomeJ::Verified,
            PrimaryOutcome::ProofFailed => PrimaryOutcomeJ::ProofFailed,
            PrimaryOutcome::NoViolation => PrimaryOutcomeJ::NoViolation,
            PrimaryOutcome::Violation => PrimaryOutcomeJ::Violation,
            PrimaryOutcome::Errored => PrimaryOutcomeJ::Errored,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
enum ScoreOutcomeJ {
    Scored { killed: u32, survived: u32, timeout: u32, unviable: u32 },
    NoMutants,
    Errored,
}

impl From<ScoreOutcome> for ScoreOutcomeJ {
    fn from(o: ScoreOutcome) -> Self {
        match o {
            ScoreOutcome::Scored { killed, survived, timeout, unviable } => {
                ScoreOutcomeJ::Scored { killed, survived, timeout, unviable }
            }
            ScoreOutcome::NoMutants => ScoreOutcomeJ::NoMutants,
            ScoreOutcome::Errored => ScoreOutcomeJ::Errored,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum PrimaryJ {
    NoEvidence,
    Pending,
    Stale,
    Fresh { outcome: PrimaryOutcomeJ },
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum ProvisionalJ {
    None,
    Provisional { outcome: PrimaryOutcomeJ },
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum ScoreJ {
    NoEvidence,
    Stale,
    Fresh { outcome: ScoreOutcomeJ },
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum UnboundWhyJ {
    NoAdapter,
    NoCheck,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum BindingJ {
    Unbound {
        why: UnboundWhyJ,
    },
    Bound {
        binding_hash: String,
        approval: ApprovalJ,
        previously_approved: bool,
        primary: PrimaryJ,
        provisional: ProvisionalJ,
        score: ScoreJ,
    },
}

impl From<Binding> for BindingJ {
    fn from(b: Binding) -> Self {
        match b {
            Binding::Unbound(UnboundWhy::NoAdapter) => BindingJ::Unbound { why: UnboundWhyJ::NoAdapter },
            Binding::Unbound(UnboundWhy::NoCheck) => BindingJ::Unbound { why: UnboundWhyJ::NoCheck },
            Binding::Bound { binding_hash, approval, previously_approved, primary, provisional, score } => {
                BindingJ::Bound {
                    binding_hash: hex::encode(&binding_hash),
                    approval: approval.into(),
                    previously_approved,
                    primary: match primary {
                        Primary::NoEvidence => PrimaryJ::NoEvidence,
                        Primary::Pending => PrimaryJ::Pending,
                        Primary::Stale => PrimaryJ::Stale,
                        Primary::Fresh(o) => PrimaryJ::Fresh { outcome: o.into() },
                    },
                    provisional: match provisional {
                        Provisional::None => ProvisionalJ::None,
                        Provisional::Provisional(o) => ProvisionalJ::Provisional { outcome: o.into() },
                    },
                    score: match score {
                        Score::NoEvidence => ScoreJ::NoEvidence,
                        Score::Stale => ScoreJ::Stale,
                        Score::Fresh(o) => ScoreJ::Fresh { outcome: o.into() },
                    },
                }
            }
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
enum StatusJ {
    ScopeEmpty { decl: DeclJ },
    Scoped { kind: KindJ, scope_hash: String, decl: DeclJ, binding: BindingJ },
}

impl From<&Status> for StatusJ {
    fn from(s: &Status) -> Self {
        match *s {
            Status::ScopeEmpty { decl } => StatusJ::ScopeEmpty { decl: decl.into() },
            Status::Scoped { kind, scope_hash, decl, binding } => StatusJ::Scoped {
                kind: kind.into(),
                scope_hash: hex::encode(&scope_hash),
                decl: decl.into(),
                binding: binding.into(),
            },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum NotCheckedWhyJ {
    NoAdapter,
    NoCheck,
    CheckUnreviewed,
    EvidencePending,
    EvidenceStale,
    AdapterErrored,
    GithubUnreachable,
    RateLimited,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ReApproveWhyJ {
    StatementAmended,
    CheckAmended,
    ConfigAmended,
    WorkflowAmended,
    ReviewerAccessRevoked,
    SpecDrifted { commits: u64 },
}

impl From<ReApproveWhy> for ReApproveWhyJ {
    fn from(w: ReApproveWhy) -> Self {
        match w {
            ReApproveWhy::StatementAmended => ReApproveWhyJ::StatementAmended,
            ReApproveWhy::CheckAmended => ReApproveWhyJ::CheckAmended,
            ReApproveWhy::ConfigAmended => ReApproveWhyJ::ConfigAmended,
            ReApproveWhy::WorkflowAmended => ReApproveWhyJ::WorkflowAmended,
            ReApproveWhy::ReviewerAccessRevoked => ReApproveWhyJ::ReviewerAccessRevoked,
            ReApproveWhy::SpecDrifted(n) => ReApproveWhyJ::SpecDrifted { commits: n },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SourceJ {
    ThisPr,
    DefaultBranch,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BrokenWhyJ {
    Negative { source: SourceJ },
    RunnerErrored,
    ScopeEmpty,
    WaiverExpired,
}

#[derive(Serialize)]
#[serde(tag = "word", rename_all = "snake_case")]
enum VerdictJ {
    Holds,
    Checked { tested: bool, strength_pending: bool, self_signed: bool },
    NotChecked { reason: NotCheckedWhyJ, self_signed: bool },
    Proposed,
    ReApprove { why: ReApproveWhyJ },
    Broken { why: BrokenWhyJ },
    Waived { until: Secs },
}

impl From<Verdict> for VerdictJ {
    fn from(v: Verdict) -> Self {
        match v {
            Verdict::Holds => VerdictJ::Holds,
            Verdict::Checked { tested, strength_pending, self_signed } => {
                VerdictJ::Checked { tested, strength_pending, self_signed }
            }
            Verdict::NotChecked { reason, self_signed } => VerdictJ::NotChecked {
                reason: match reason {
                    NotCheckedWhy::NoAdapter => NotCheckedWhyJ::NoAdapter,
                    NotCheckedWhy::NoCheck => NotCheckedWhyJ::NoCheck,
                    NotCheckedWhy::CheckUnreviewed => NotCheckedWhyJ::CheckUnreviewed,
                    NotCheckedWhy::EvidencePending => NotCheckedWhyJ::EvidencePending,
                    NotCheckedWhy::EvidenceStale => NotCheckedWhyJ::EvidenceStale,
                    NotCheckedWhy::AdapterErrored => NotCheckedWhyJ::AdapterErrored,
                    NotCheckedWhy::GitHubUnreachable => NotCheckedWhyJ::GithubUnreachable,
                    NotCheckedWhy::RateLimited => NotCheckedWhyJ::RateLimited,
                },
                self_signed,
            },
            Verdict::Proposed => VerdictJ::Proposed,
            Verdict::ReApprove(why) => VerdictJ::ReApprove { why: why.into() },
            Verdict::Broken(why) => VerdictJ::Broken {
                why: match why {
                    BrokenWhy::Negative(Source::ThisPr) => BrokenWhyJ::Negative { source: SourceJ::ThisPr },
                    BrokenWhy::Negative(Source::DefaultBranch) => {
                        BrokenWhyJ::Negative { source: SourceJ::DefaultBranch }
                    }
                    BrokenWhy::RunnerErrored => BrokenWhyJ::RunnerErrored,
                    BrokenWhy::ScopeEmpty => BrokenWhyJ::ScopeEmpty,
                    BrokenWhy::WaiverExpired => BrokenWhyJ::WaiverExpired,
                },
            },
            Verdict::Waived { until } => VerdictJ::Waived { until },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum OutcomeJ {
    Pass,
    Warn,
    Block,
}

impl From<Outcome> for OutcomeJ {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Pass => OutcomeJ::Pass,
            Outcome::Warn => OutcomeJ::Warn,
            Outcome::Block => OutcomeJ::Block,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum GraceJ {
    Within,
    After,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum RemedyJ {
    None,
    OptionalSecondReviewer,
    AddReviewerWithWriteAccess,
    AskAuthorToCommitSuggestion,
    ReApproveLatestCommit,
    ResolveIdentity,
    OpenPullRequest,
}

impl From<Remedy> for RemedyJ {
    fn from(r: Remedy) -> Self {
        match r {
            Remedy::None => RemedyJ::None,
            Remedy::OptionalSecondReviewer => RemedyJ::OptionalSecondReviewer,
            Remedy::AddReviewerWithWriteAccess => RemedyJ::AddReviewerWithWriteAccess,
            Remedy::AskAuthorToCommitSuggestion => RemedyJ::AskAuthorToCommitSuggestion,
            Remedy::ReApproveLatestCommit => RemedyJ::ReApproveLatestCommit,
            Remedy::ResolveIdentity => RemedyJ::ResolveIdentity,
            Remedy::OpenPullRequest => RemedyJ::OpenPullRequest,
        }
    }
}

// ---------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum VerificationJ {
    Verified { source: &'static str },
    Unverified { reason: &'static str, cache_age_secs: Option<Secs>, message: Option<String> },
    NotConsulted,
}

impl From<&Verification> for VerificationJ {
    fn from(v: &Verification) -> Self {
        match v {
            Verification::Verified { source: VerifiedSource::Live } => VerificationJ::Verified { source: "live" },
            Verification::Verified { source: VerifiedSource::Fixture } => VerificationJ::Verified { source: "fixture" },
            Verification::Unverified(UnverifiedReason::NoToken { cache_age }) => {
                VerificationJ::Unverified { reason: "no_token", cache_age_secs: *cache_age, message: None }
            }
            Verification::Unverified(UnverifiedReason::NoRemote { cache_age }) => {
                VerificationJ::Unverified { reason: "no_remote", cache_age_secs: *cache_age, message: None }
            }
            Verification::Unverified(UnverifiedReason::GitHubUnreachable { message }) => VerificationJ::Unverified {
                reason: "github_unreachable",
                cache_age_secs: None,
                message: Some(message.clone()),
            },
            Verification::Unverified(UnverifiedReason::RateLimited) => {
                VerificationJ::Unverified { reason: "rate_limited", cache_age_secs: None, message: None }
            }
            Verification::NotConsulted => VerificationJ::NotConsulted,
        }
    }
}

#[derive(Serialize)]
struct ThisPrJ<'a> {
    outcome: &'a OutcomeRecord,
    replay: Option<&'a Replay>,
}

#[derive(Serialize)]
struct RowJ<'a> {
    id: &'a str,
    path: &'a str,
    title: &'a str,
    kind: KindJ,
    severity: SeverityJ,
    /// `verified`, `git_only` (a proposal git alone establishes),
    /// `rate_limited`, `no_token`, or `github_unreachable`.
    verification: &'static str,
    message: Option<String>,
    word: String,
    modifiers: Vec<&'static str>,
    reason: String,
    this_pr: String,
    affected: Option<bool>,
    outcome: Option<OutcomeJ>,
    grace: Option<GraceJ>,
    status: Option<StatusJ>,
    verdict: Option<VerdictJ>,
    provisional: Option<ThisPrJ<'a>>,
}

fn row_json(r: &Row) -> RowJ<'_> {
    let (verification, message) = match &r.verdict {
        RowVerdict::Derived(_) => ("verified", None),
        RowVerdict::Proposal => ("git_only", None),
        RowVerdict::Unverified(RowUnverified::RateLimited) => ("rate_limited", None),
        RowVerdict::Unverified(RowUnverified::NoToken) => ("no_token", None),
        RowVerdict::Unverified(RowUnverified::Unreachable { message }) => ("github_unreachable", Some(message.clone())),
    };
    let derived = match &r.verdict {
        RowVerdict::Derived(d) => Some(d),
        _ => None,
    };
    RowJ {
        id: &r.id,
        path: &r.path,
        title: &r.title,
        kind: r.kind.into(),
        severity: r.severity.into(),
        verification,
        message,
        word: vocab::row_word(&r.verdict),
        modifiers: derived.map(|d| vocab::modifiers(&d.verdict)).unwrap_or_default(),
        reason: vocab::row_reason(&r.verdict, Some(r.kind)),
        this_pr: vocab::this_pr(r.this_pr.as_ref()),
        affected: r.affected,
        outcome: derived.and_then(|d| d.outcome).map(OutcomeJ::from),
        grace: derived.and_then(|d| d.grace).map(|g| match g {
            Grace::Within => GraceJ::Within,
            Grace::After => GraceJ::After,
        }),
        status: derived.map(|d| StatusJ::from(&d.status)),
        verdict: derived.map(|d| VerdictJ::from(d.verdict)),
        provisional: r.this_pr.as_ref().map(|rec| ThisPrJ { outcome: &rec.outcome, replay: rec.replay.as_ref() }),
    }
}

#[derive(Serialize)]
struct IfMergedJ<'a> {
    path: &'a str,
    approval: Option<ApprovalJ>,
    remedy: Option<RemedyJ>,
    approval_words: String,
    remedy_words: Option<&'static str>,
    unverified: Option<String>,
}

fn if_merged_json(r: &IfMerged) -> IfMergedJ<'_> {
    match &r.preview {
        Ok(p) => IfMergedJ {
            path: &r.path,
            approval: Some(p.approval.into()),
            remedy: Some(p.remedy.into()),
            approval_words: vocab::approval_words(p.approval),
            remedy_words: Some(vocab::remedy_words(p.remedy)),
            unverified: None,
        },
        Err(u) => IfMergedJ {
            path: &r.path,
            approval: None,
            remedy: None,
            approval_words: "unverified".to_string(),
            remedy_words: None,
            unverified: Some(u.reason()),
        },
    }
}

#[derive(Serialize)]
struct CountsJ {
    holds: usize,
    checked: usize,
    not_checked: usize,
    re_approve: usize,
    broken: usize,
    proposed: usize,
    waived: usize,
    unverified: usize,
    total: usize,
}

#[derive(Serialize)]
struct FooterJ {
    approvals_verified: &'static str,
    branch_requires_review: KnownJ,
    gate_required: KnownJ,
    codeowners: KnownJ,
    scheduled_evidence_age_secs: Option<Secs>,
}

#[derive(Serialize)]
struct InputsJ<'a> {
    default_branch: &'a str,
    default_head: Option<String>,
    base: Option<&'a str>,
    head: Option<&'a str>,
    now: Secs,
    calls_spent: u32,
}

#[derive(Serialize)]
struct ConfigJ {
    in_force: &'static str,
    why: Option<&'static str>,
}

#[derive(Serialize)]
pub struct ReportJ<'a> {
    schema_version: u32,
    mode: &'static str,
    inputs: InputsJ<'a>,
    verification: VerificationJ,
    headline: String,
    counts: CountsJ,
    config: ConfigJ,
    amendments: Vec<ReApproveWhyJ>,
    governance: Option<GovernanceJ>,
    properties: Vec<RowJ<'a>>,
    if_merged_now: Vec<IfMergedJ<'a>>,
    pull_request: Option<u64>,
    footer: FooterJ,
    notes: &'a [String],
    /// Changed source files no property covers; absent when the run did not look.
    #[serde(skip_serializing_if = "Option::is_none")]
    uncovered: Option<&'a [String]>,
    exit: u8,
}

pub fn report_json(r: &Report, headline: String, exit: u8) -> ReportJ<'_> {
    let (mode, base, head) = match &r.mode {
        Mode::Status => ("status", None, None),
        Mode::Check { base, head } => ("check", Some(base.as_str()), Some(head.as_str())),
        Mode::Bootstrap { base, head } => ("bootstrap", Some(base.as_str()), Some(head.as_str())),
    };
    let c = r.counts();
    let (in_force, why) = match r.config {
        ConfigInForce::Approved => ("approved", None),
        ConfigInForce::Defaults(DefaultsWhy::Unapproved) => ("defaults", Some("unapproved")),
        ConfigInForce::Defaults(DefaultsWhy::Missing) => ("defaults", Some("missing")),
        ConfigInForce::Defaults(DefaultsWhy::Bootstrap) => ("defaults", Some("bootstrap")),
        ConfigInForce::Unknown => ("unknown", None),
    };
    ReportJ {
        schema_version: SCHEMA_VERSION,
        mode,
        inputs: InputsJ {
            default_branch: &r.default_branch,
            default_head: r.default_head.map(|h| hex::encode_oid(&h)),
            base,
            head,
            now: crate::cli::workspace::clock(),
            calls_spent: r.calls_spent,
        },
        verification: VerificationJ::from(&r.verification),
        headline,
        counts: CountsJ {
            holds: c.holds,
            checked: c.checked,
            not_checked: c.not_checked,
            re_approve: c.re_approve,
            broken: c.broken,
            proposed: c.proposed,
            waived: c.waived,
            unverified: c.unverified,
            total: c.total(),
        },
        config: ConfigJ { in_force, why },
        amendments: r.amendments.iter().map(|a| ReApproveWhyJ::from(*a)).collect(),
        governance: r.governance.as_ref().map(GovernanceJ::from),
        properties: r.rows.iter().map(row_json).collect(),
        if_merged_now: r.if_merged_now.iter().map(if_merged_json).collect(),
        pull_request: r.pull_request,
        footer: FooterJ {
            approvals_verified: match r.footer.approvals {
                ApprovalsVerified::Yes => "yes",
                ApprovalsVerified::No => "no",
                ApprovalsVerified::RateLimited => "rate_limited",
            },
            branch_requires_review: r.footer.branch_requires_review.into(),
            gate_required: r.footer.gate_required.into(),
            codeowners: r.footer.codeowners.into(),
            scheduled_evidence_age_secs: r.footer.scheduled_evidence_age,
        },
        notes: &r.notes,
        uncovered: r.uncovered.as_deref(),
        exit,
    }
}
