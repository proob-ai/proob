//! Generators for the verified core's types, so the spec-validation tests can
//! build statuses, governance, policies, and derive inputs directly.
#![allow(dead_code)]
use proob_core::scope_hash::PathDigest;
use proob_core::types::*;
use proptest::prelude::*;

pub const ID: PropertyId = 0x0192_3b4c_5d6e_7f80_9192_a3b4_c5d6_e7f8;
pub const NOW: Secs = 1_793_145_600;
pub const DAY: Secs = 86_400;

pub fn digest() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

pub fn sig() -> impl Strategy<Value = Sig> {
    prop_oneof![Just(Sig::Team), Just(Sig::SelfSigned)]
}

pub fn ineligible() -> impl Strategy<Value = Ineligible> {
    prop_oneof![
        Just(Ineligible::NoReview),
        Just(Ineligible::ApproverEditedGoverned),
        Just(Ineligible::ApprovalPredatesChange),
        Just(Ineligible::BotOnly),
        Just(Ineligible::NoWriteAccess),
        Just(Ineligible::UnknownIdentity),
    ]
}

pub fn unreviewed_reason() -> impl Strategy<Value = UnreviewedReason> {
    prop_oneof![
        Just(UnreviewedReason::DirectPush),
        ineligible().prop_map(UnreviewedReason::NoEligibleReview),
        Just(UnreviewedReason::ReviewerAccessRevoked),
    ]
}

pub fn approval() -> impl Strategy<Value = Approval> {
    prop_oneof![
        (sig(), any::<u64>(), digest()).prop_map(|(sig, pr, merge_sha)| Approval::Approved { sig, pr, merge_sha }),
        unreviewed_reason().prop_map(Approval::Unreviewed),
    ]
}

pub fn team() -> Approval {
    Approval::Approved { sig: Sig::Team, pr: 7, merge_sha: [7; 32] }
}

pub fn self_signed() -> Approval {
    Approval::Approved { sig: Sig::SelfSigned, pr: 7, merge_sha: [7; 32] }
}

pub fn unreviewed() -> Approval {
    Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::NoReview))
}

pub fn known() -> impl Strategy<Value = Known> {
    prop_oneof![Just(Known::Yes), Just(Known::No), Just(Known::Unknown)]
}

pub fn governance() -> impl Strategy<Value = Governance> {
    (approval(), approval(), approval(), known(), known(), known(), 0u32..4).prop_map(
        |(config, evidence_workflow, gate_workflow, branch_requires_review, gate_required, codeowners, floor)| {
            Governance {
                config,
                evidence_workflow,
                gate_workflow,
                branch_requires_review,
                gate_required,
                codeowners,
                floor,
            }
        },
    )
}

/// Team-approved config and evidence workflow, floor 1.
pub fn approved_governance() -> Governance {
    Governance {
        config: team(),
        evidence_workflow: team(),
        gate_workflow: team(),
        branch_requires_review: Known::Yes,
        gate_required: Known::Yes,
        codeowners: Known::Yes,
        floor: 1,
    }
}

pub fn kind() -> impl Strategy<Value = Kind> {
    prop_oneof![Just(Kind::Test), Just(Kind::Proof), Just(Kind::Model)]
}

pub fn proposed_why() -> impl Strategy<Value = ProposedWhy> {
    prop_oneof![Just(ProposedWhy::NeverReviewed), Just(ProposedWhy::Amended), Just(ProposedWhy::ReviewerAccessRevoked)]
}

pub fn drift() -> impl Strategy<Value = Drift> {
    (0u64..4, 0u64..100).prop_map(|(commits, days)| Drift { commits, days })
}

pub fn window() -> impl Strategy<Value = Window> {
    prop_oneof![
        (any::<Secs>(), any::<bool>()).prop_map(|(until, issue_open)| Window::Active { until, issue_open }),
        any::<Secs>().prop_map(|until| Window::Expired { until }),
    ]
}

pub fn decl() -> impl Strategy<Value = Decl> {
    prop_oneof![
        proposed_why().prop_map(Decl::Proposed),
        (sig(), digest(), drift()).prop_map(|(sig, since, drift)| Decl::Approved { sig, since, drift }),
        (sig(), digest(), window()).prop_map(|(sig, since, window)| Decl::Waived { sig, since, window }),
    ]
}

pub fn primary_outcome() -> impl Strategy<Value = PrimaryOutcome> {
    prop_oneof![
        Just(PrimaryOutcome::NoCounterexample),
        Just(PrimaryOutcome::Counterexample),
        Just(PrimaryOutcome::Verified),
        Just(PrimaryOutcome::ProofFailed),
        Just(PrimaryOutcome::NoViolation),
        Just(PrimaryOutcome::Violation),
        Just(PrimaryOutcome::Errored),
    ]
}

pub fn score_outcome() -> impl Strategy<Value = ScoreOutcome> {
    prop_oneof![
        (0u32..4, 0u32..4, 0u32..2, 0u32..2).prop_map(|(killed, survived, timeout, unviable)| ScoreOutcome::Scored {
            killed,
            survived,
            timeout,
            unviable
        }),
        Just(ScoreOutcome::NoMutants),
        Just(ScoreOutcome::Errored),
    ]
}

pub fn primary() -> impl Strategy<Value = Primary> {
    prop_oneof![
        Just(Primary::NoEvidence),
        Just(Primary::Pending),
        Just(Primary::Stale),
        primary_outcome().prop_map(Primary::Fresh),
    ]
}

pub fn provisional() -> impl Strategy<Value = Provisional> {
    prop_oneof![Just(Provisional::None), primary_outcome().prop_map(Provisional::Provisional)]
}

pub fn score() -> impl Strategy<Value = Score> {
    prop_oneof![Just(Score::NoEvidence), Just(Score::Stale), score_outcome().prop_map(Score::Fresh)]
}

pub fn unbound_why() -> impl Strategy<Value = UnboundWhy> {
    prop_oneof![Just(UnboundWhy::NoAdapter), Just(UnboundWhy::NoCheck)]
}

pub fn binding() -> impl Strategy<Value = Binding> {
    prop_oneof![
        unbound_why().prop_map(Binding::Unbound),
        (digest(), approval(), any::<bool>(), primary(), provisional(), score()).prop_map(
            |(binding_hash, approval, previously_approved, primary, provisional, score)| Binding::Bound {
                binding_hash,
                approval,
                previously_approved,
                primary,
                provisional,
                score,
            },
        ),
    ]
}

pub fn status() -> impl Strategy<Value = Status> {
    prop_oneof![
        decl().prop_map(|decl| Status::ScopeEmpty { decl }),
        (kind(), digest(), decl(), binding()).prop_map(|(kind, scope_hash, decl, binding)| Status::Scoped {
            kind,
            scope_hash,
            decl,
            binding
        }),
    ]
}

/// The positive outcome for a kind.
pub fn positive_for(kind: Kind) -> PrimaryOutcome {
    match kind {
        Kind::Test => PrimaryOutcome::NoCounterexample,
        Kind::Proof => PrimaryOutcome::Verified,
        Kind::Model => PrimaryOutcome::NoViolation,
    }
}

/// The negative outcome for a kind.
pub fn negative_for(kind: Kind) -> PrimaryOutcome {
    match kind {
        Kind::Test => PrimaryOutcome::Counterexample,
        Kind::Proof => PrimaryOutcome::ProofFailed,
        Kind::Model => PrimaryOutcome::Violation,
    }
}

/// A status that renders `top(kind)` under `approved_governance()`:
/// team-approved statement and binding set, fresh positive primary evidence,
/// fresh score of one kill, no drift.
pub fn holds_ready(kind: Kind) -> Status {
    Status::Scoped {
        kind,
        scope_hash: [1; 32],
        decl: Decl::Approved { sig: Sig::Team, since: [7; 32], drift: Drift { commits: 0, days: 0 } },
        binding: Binding::Bound {
            binding_hash: [2; 32],
            approval: team(),
            previously_approved: true,
            primary: Primary::Fresh(positive_for(kind)),
            provisional: Provisional::None,
            score: Score::Fresh(ScoreOutcome::Scored { killed: 1, survived: 0, timeout: 0, unviable: 0 }),
        },
    }
}

/// The best verdict a kind can earn: `Holds` for a proof or a model; a test
/// samples its inputs, so it tops out at `Checked (tested)`.
pub fn top(kind: Kind) -> Verdict {
    match kind {
        Kind::Test => Verdict::Checked { tested: true, strength_pending: false, self_signed: false },
        Kind::Proof | Kind::Model => Verdict::Holds,
    }
}

/// `holds_ready` with its binding replaced by `f(binding)`.
pub fn with_binding(s: Status, f: impl FnOnce(Binding) -> Binding) -> Status {
    match s {
        Status::Scoped { kind, scope_hash, decl, binding } => {
            Status::Scoped { kind, scope_hash, decl, binding: f(binding) }
        }
        other => other,
    }
}

/// `holds_ready` with its declaration replaced.
pub fn with_decl(s: Status, decl: Decl) -> Status {
    match s {
        Status::Scoped { kind, scope_hash, binding, .. } => Status::Scoped { kind, scope_hash, decl, binding },
        Status::ScopeEmpty { .. } => Status::ScopeEmpty { decl },
    }
}

pub fn set_primary(b: Binding, primary: Primary) -> Binding {
    match b {
        Binding::Bound { binding_hash, approval, previously_approved, provisional, score, .. } => {
            Binding::Bound { binding_hash, approval, previously_approved, primary, provisional, score }
        }
        other => other,
    }
}

pub fn set_provisional(b: Binding, provisional: Provisional) -> Binding {
    match b {
        Binding::Bound { binding_hash, approval, previously_approved, primary, score, .. } => {
            Binding::Bound { binding_hash, approval, previously_approved, primary, provisional, score }
        }
        other => other,
    }
}

pub fn set_score(b: Binding, score: Score) -> Binding {
    match b {
        Binding::Bound { binding_hash, approval, previously_approved, primary, provisional, .. } => {
            Binding::Bound { binding_hash, approval, previously_approved, primary, provisional, score }
        }
        other => other,
    }
}

pub fn set_binding_approval(b: Binding, approval: Approval) -> Binding {
    match b {
        Binding::Bound { binding_hash, previously_approved, primary, provisional, score, .. } => {
            Binding::Bound { binding_hash, approval, previously_approved, primary, provisional, score }
        }
        other => other,
    }
}

pub fn outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![Just(Outcome::Pass), Just(Outcome::Warn), Just(Outcome::Block)]
}

pub fn any_policy() -> impl Strategy<Value = Policy> {
    prop::collection::vec(outcome(), 9).prop_map(|o| Policy {
        holds: o[0],
        checked: o[1],
        proposed: o[2],
        waived: o[3],
        not_checked_within_grace: o[4],
        not_checked_after_grace: o[5],
        re_approve: o[6],
        broken_affected: o[7],
        broken_unaffected: o[8],
    })
}

/// The plan's default table.
pub fn default_policy() -> Policy {
    Policy {
        holds: Outcome::Pass,
        checked: Outcome::Pass,
        proposed: Outcome::Pass,
        waived: Outcome::Pass,
        not_checked_within_grace: Outcome::Pass,
        not_checked_after_grace: Outcome::Warn,
        re_approve: Outcome::Warn,
        broken_affected: Outcome::Block,
        broken_unaffected: Outcome::Pass,
    }
}

pub fn gate_config() -> GateConfig {
    GateConfig {
        policy: default_policy(),
        grace_secs: 7 * DAY,
        freshness_secs: 14 * DAY,
        floor: 1,
        allow_self_signed: true,
    }
}

pub fn grace() -> impl Strategy<Value = Grace> {
    prop_oneof![Just(Grace::Within), Just(Grace::After)]
}

pub fn not_checked_why() -> impl Strategy<Value = NotCheckedWhy> {
    prop_oneof![
        Just(NotCheckedWhy::NoAdapter),
        Just(NotCheckedWhy::NoCheck),
        Just(NotCheckedWhy::CheckUnreviewed),
        Just(NotCheckedWhy::EvidencePending),
        Just(NotCheckedWhy::EvidenceStale),
        Just(NotCheckedWhy::AdapterErrored),
        Just(NotCheckedWhy::GitHubUnreachable),
        Just(NotCheckedWhy::RateLimited),
    ]
}

pub fn re_approve_why() -> impl Strategy<Value = ReApproveWhy> {
    prop_oneof![
        Just(ReApproveWhy::StatementAmended),
        Just(ReApproveWhy::CheckAmended),
        Just(ReApproveWhy::ConfigAmended),
        Just(ReApproveWhy::WorkflowAmended),
        Just(ReApproveWhy::ReviewerAccessRevoked),
        any::<u64>().prop_map(ReApproveWhy::SpecDrifted),
    ]
}

pub fn broken_why() -> impl Strategy<Value = BrokenWhy> {
    prop_oneof![
        Just(BrokenWhy::Negative(Source::ThisPr)),
        Just(BrokenWhy::Negative(Source::DefaultBranch)),
        Just(BrokenWhy::RunnerErrored),
        Just(BrokenWhy::ScopeEmpty),
        Just(BrokenWhy::WaiverExpired),
    ]
}

pub fn any_verdict() -> impl Strategy<Value = Verdict> {
    prop_oneof![
        Just(Verdict::Holds),
        (any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(tested, strength_pending, self_signed)| {
            Verdict::Checked { tested, strength_pending, self_signed }
        }),
        (not_checked_why(), any::<bool>())
            .prop_map(|(reason, self_signed)| Verdict::NotChecked { reason, self_signed }),
        Just(Verdict::Proposed),
        re_approve_why().prop_map(Verdict::ReApprove),
        broken_why().prop_map(Verdict::Broken),
        any::<Secs>().prop_map(|until| Verdict::Waived { until }),
    ]
}

// ---------------------------------------------------------------------------
// Paths, scopes, evidence, derive inputs
// ---------------------------------------------------------------------------

pub fn path() -> impl Strategy<Value = Vec<u8>> {
    "[a-z]{1,4}(/[a-z]{1,4}){0,2}\\.rs".prop_map(String::into_bytes)
}

pub fn path_digest() -> impl Strategy<Value = PathDigest> {
    (path(), digest()).prop_map(|(path, digest)| PathDigest { path, digest })
}

/// A scope of distinct paths, as resolved files are.
pub fn scope() -> impl Strategy<Value = Vec<PathDigest>> {
    prop::collection::btree_map(path(), digest(), 0..6)
        .prop_map(|m| m.into_iter().map(|(path, digest)| PathDigest { path, digest }).collect())
}

pub fn clone_scope(scope: &[PathDigest]) -> Vec<PathDigest> {
    scope.iter().map(|p| PathDigest { path: p.path.clone(), digest: p.digest }).collect()
}

pub fn property(kind: Option<Kind>, waiver: Option<Waiver>) -> Property {
    Property { id: ID, kind, scope: vec![b"src/**/*.rs".to_vec()], severity: Severity::High, waiver, reviewed: None }
}

pub fn record(measured: Measured, scope_hash: [u8; 32], binding_hash: [u8; 32], at: Secs) -> Evidence {
    Evidence {
        property: ID,
        content_hash: [3; 32],
        binding_hash,
        scope_hash,
        spec_hash: None,
        measured,
        run_id: 1,
        head_sha: [9; 32],
        workflow_blob: [8; 32],
        at,
    }
}

pub fn run_result(measured: Measured, scope_hash: [u8; 32], binding_hash: [u8; 32]) -> RunResult {
    RunResult { property: ID, content_hash: [3; 32], binding_hash, scope_hash, measured }
}

pub const BINDING_HASH: [u8; 32] = [2; 32];
pub const DEFAULT_HEAD: [u8; 32] = [9; 32];

/// A derive input for an approved, bound property with no evidence yet and the
/// latest run at the default head.
pub fn base_input(kind: Kind, scope: Vec<PathDigest>) -> DeriveInput {
    DeriveInput {
        property: property(Some(kind), None),
        kind,
        scope,
        statement: team(),
        previously_approved: true,
        drift: Drift { commits: 0, days: 0 },
        waiver_issue_open: false,
        binding: Some(BindingFacts { binding_hash: BINDING_HASH, approval: team(), previously_approved: true }),
        adapter_available: true,
        records: Vec::new(),
        evidence_run_head: Some(DEFAULT_HEAD),
        default_head: DEFAULT_HEAD,
        provisional: None,
    }
}

/// The scope hash `derive` will compute for these files, read back from a
/// derivation (so tests can build records "at the current hashes").
pub fn scope_hash_of(scope: &[PathDigest]) -> [u8; 32] {
    match proob_core::scope_hash::scope_hash(clone_scope(scope)) {
        proob_core::scope_hash::ScopeHash::Ok(h) => h,
        proob_core::scope_hash::ScopeHash::DuplicatePath => panic!("test scopes have distinct paths"),
    }
}
