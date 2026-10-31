//! The six assertions as specification predicates.
//!
//! ASSERTIONS.md is the authoritative prose; each predicate here is named
//! there, and U3 (`verdict`, `derive`) and U9 (`eligibility`) carry them as
//! `ensures` clauses. Everything in this module is spec-only: definitions the
//! verifier accepts as written and that the exec functions must then satisfy.
//!
//! Also here: the well-formedness predicates that state the invariants the
//! types in `types.rs` cannot express structurally.
// Everything here is spec-only and erased by a plain `cargo build`; the guards
// keep the imports to verification only.
#[cfg(verus_only)]
use crate::grammar::scope_ok;
#[cfg(verus_only)]
use crate::types::*;
use vstd::prelude::*;

verus! {

// ---------------------------------------------------------------------------
// Pieces
// ---------------------------------------------------------------------------

/// An eligible reviewer approved the file's current content.
pub open spec fn team_approved(a: Approval) -> bool {
    match a {
        Approval::Approved { sig: Sig::Team, .. } => true,
        _ => false,
    }
}

/// The file was merged without an eligible review and the repository counts
/// that as a weaker signature.
pub open spec fn self_signed(a: Approval) -> bool {
    match a {
        Approval::Approved { sig: Sig::SelfSigned, .. } => true,
        _ => false,
    }
}

/// The declaration axis of either status constructor.
pub open spec fn decl_of(s: Status) -> Decl {
    match s {
        Status::ScopeEmpty { decl } => decl,
        Status::Scoped { decl, .. } => decl,
    }
}

/// The outcome that counts as positive for a kind. Any other pairing,
/// including a kind mismatch, is not positive. A test's positive is evidence
/// for `Checked (tested)`, never for `Holds`: see `verified_kind`.
pub open spec fn positive(kind: Kind, o: PrimaryOutcome) -> bool {
    match (kind, o) {
        (Kind::Test, PrimaryOutcome::NoCounterexample) => true,
        (Kind::Proof, PrimaryOutcome::Verified) => true,
        (Kind::Model, PrimaryOutcome::NoViolation) => true,
        _ => false,
    }
}

/// The outcome that counts as negative for a kind: a counterexample, a failed
/// proof, a violation. Any other pairing, including a kind mismatch, is not
/// negative; `verdict` turns a negative into `Broken(Negative(_))`.
pub open spec fn negative(kind: Kind, o: PrimaryOutcome) -> bool {
    match (kind, o) {
        (Kind::Test, PrimaryOutcome::Counterexample) => true,
        (Kind::Proof, PrimaryOutcome::ProofFailed) => true,
        (Kind::Model, PrimaryOutcome::Violation) => true,
        _ => false,
    }
}

/// The outcome is one a check of this kind can produce (positive, negative,
/// or errored). Well-formedness for records; never needed for soundness,
/// because `positive` is already false off the diagonal.
pub open spec fn coherent(kind: Kind, o: PrimaryOutcome) -> bool {
    match (kind, o) {
        (_, PrimaryOutcome::Errored) => true,
        (Kind::Test, PrimaryOutcome::NoCounterexample) => true,
        (Kind::Test, PrimaryOutcome::Counterexample) => true,
        (Kind::Proof, PrimaryOutcome::Verified) => true,
        (Kind::Proof, PrimaryOutcome::ProofFailed) => true,
        (Kind::Model, PrimaryOutcome::NoViolation) => true,
        (Kind::Model, PrimaryOutcome::Violation) => true,
        _ => false,
    }
}

/// The binding set is team-approved.
pub open spec fn binding_team_approved(b: Binding) -> bool {
    match b {
        Binding::Bound { approval, .. } => team_approved(approval),
        Binding::Unbound(_) => false,
    }
}

/// The config file and the evidence workflow are team-approved. The gate
/// workflow is reported in the footer but does not gate `Holds`: a tampered
/// gate can only fail to run, never manufacture evidence.
pub open spec fn governance_team_approved(g: Governance) -> bool {
    team_approved(g.config) && team_approved(g.evidence_workflow)
}

/// Fresh positive primary evidence for the kind.
pub open spec fn primary_fresh_positive(b: Binding, kind: Kind) -> bool {
    match b {
        Binding::Bound { primary: Primary::Fresh(o), .. } => positive(kind, o),
        _ => false,
    }
}

/// Fresh score evidence showing the check can fail: kills at or above a floor
/// that is itself at least one. `NoMutants` and `Errored` never satisfy this.
pub open spec fn score_at_floor(b: Binding, floor: u32) -> bool {
    1 <= floor && match b {
        Binding::Bound { score: Score::Fresh(ScoreOutcome::Scored { killed, .. }), .. } => floor <= killed,
        _ => false,
    }
}

/// For a model-backed property, no commit has touched the modelled scope
/// since the spec's approval. Vacuous for other kinds.
pub open spec fn model_undrifted(s: Status) -> bool {
    match s {
        Status::Scoped { kind: Kind::Model, decl: Decl::Approved { drift, .. }, .. } => drift.commits == 0,
        Status::Scoped { kind: Kind::Model, .. } => false,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// The six assertions
// ---------------------------------------------------------------------------

/// Assertion 1: the property's content is approved by an eligible review.
pub open spec fn statement_team_approved(s: Status) -> bool {
    match decl_of(s) {
        Decl::Approved { sig: Sig::Team, .. } => true,
        _ => false,
    }
}

/// Assertion 2: the bound check's binding set, the config, and the evidence
/// workflow are each approved by an eligible review.
pub open spec fn check_and_governance_team_approved(s: Status, g: Governance) -> bool {
    match s {
        Status::Scoped { binding, .. } => binding_team_approved(binding) && governance_team_approved(g),
        Status::ScopeEmpty { .. } => false,
    }
}

/// Assertion 3, the kind: a proof or a model, whose positive speaks for every
/// input. A test samples, so its strongest word is `Checked (tested)`.
pub open spec fn verified_kind(s: Status) -> bool {
    match s {
        Status::Scoped { kind: Kind::Proof, .. } => true,
        Status::Scoped { kind: Kind::Model, .. } => true,
        _ => false,
    }
}

/// Assertion 3: the scope is non-empty (`Scoped`) and the latest default-branch
/// primary evidence for the kind is fresh at the current hashes and positive.
/// "At the current hashes" is `derive`'s `ensures` on `Primary::Fresh`.
pub open spec fn evidence_fresh_positive(s: Status) -> bool {
    match s {
        Status::Scoped { kind, binding, .. } => primary_fresh_positive(binding, kind),
        Status::ScopeEmpty { .. } => false,
    }
}

/// Assertion 4: the latest default-branch score at the current binding hash,
/// within the freshness window, kills at least the floor; and a model-backed
/// property has zero drift. "Within the window" is `derive`'s `ensures` on
/// `Score::Fresh`.
pub open spec fn strength_at_floor(s: Status, g: Governance) -> bool {
    match s {
        Status::Scoped { binding, .. } => score_at_floor(binding, g.floor) && model_undrifted(s),
        Status::ScopeEmpty { .. } => false,
    }
}

/// Assertions 1 to 4 together: what `verdict` must establish to answer `Holds`.
pub open spec fn holds_requirements(s: Status, g: Governance) -> bool {
    statement_team_approved(s) && check_and_governance_team_approved(s, g) && verified_kind(s)
        && evidence_fresh_positive(s) && strength_at_floor(s, g)
}

/// Assertion 5: `Waived` requires waiver content approved by an eligible
/// review, an open issue, and an unexpired window.
pub open spec fn waived_requirements(s: Status) -> bool {
    match decl_of(s) {
        Decl::Waived { sig: Sig::Team, window: Window::Active { issue_open: true, .. }, .. } => true,
        _ => false,
    }
}

/// Assertion 6, the rule: a review on the introducing PR is eligible when it
/// approves, its reviewer is a human with write access now who is not the PR
/// author and did not author or commit a governed change in the PR, every
/// governed change's author is resolved, and the review is on a commit at
/// which the file's blob equals its blob at merge.
pub open spec fn eligible_review(f: PrFacts, r: Review) -> bool {
    r.state is Approved && r.user.kind is Human && r.user.has_write_now && r.user.id != f.author
        && !f.governed_change_authors@.contains(r.user.id) && !f.unresolved_governed_author
        && r.blob_at_commit == Some(f.blob_at_merge)
}

/// Some review in the facts is eligible.
pub open spec fn has_eligible_review(f: PrFacts) -> bool {
    exists|i: int| 0 <= i < f.reviews@.len() && eligible_review(f, #[trigger] f.reviews@[i])
}

/// Assertion 6, soundness: what `eligibility` must establish about the
/// approval it returns. `Team` only with an eligible review; `SelfSigned` only
/// when allowed and no review is eligible; a direct push is never approved.
pub open spec fn eligibility_sound(intro: Introduction, allow_self_signed: bool, a: Approval) -> bool {
    &&& (match a {
        Approval::Approved { sig: Sig::Team, .. } => intro matches Introduction::Merged(f) && has_eligible_review(f),
        _ => true,
    })
    &&& (match a {
        Approval::Approved { sig: Sig::SelfSigned, .. } => allow_self_signed && (intro matches Introduction::Merged(f) && !has_eligible_review(f)),
        _ => true,
    })
    &&& (intro is DirectPush ==> a == Approval::Unreviewed(UnreviewedReason::DirectPush))
    &&& (match a {
        Approval::Approved { pr, merge_sha, .. } => intro matches Introduction::Merged(f) && pr == f.number && merge_sha == f.merge_sha,
        _ => true,
    })
}

// ---------------------------------------------------------------------------
// Well-formedness
// ---------------------------------------------------------------------------

/// A property file's core content: a non-empty scope of grammatical globs.
pub open spec fn property_wf(p: Property) -> bool {
    scope_ok(p.scope@)
}

/// The kill floor cannot be lowered below one.
pub open spec fn gate_config_wf(c: GateConfig) -> bool {
    1 <= c.floor
}

/// `Checked` carries at least one modifier; with none it would be `Holds`.
/// `tested` is not a defect, the other two are.
pub open spec fn verdict_wf(v: Verdict) -> bool {
    match v {
        Verdict::Checked { tested, strength_pending, self_signed } => tested || strength_pending || self_signed,
        _ => true,
    }
}

/// A record's measurement is one its kind can produce.
pub open spec fn measured_wf(m: Measured) -> bool {
    match m {
        Measured::Primary(kind, o) => coherent(kind, o),
        Measured::Score(_) => true,
    }
}

// ---------------------------------------------------------------------------
// Sanity: the predicates mean what the prose says on the obvious cases
// ---------------------------------------------------------------------------

/// Self-signed content never meets assertion 1, whatever else is true.
proof fn self_signed_never_holds(s: Status, g: Governance)
    requires
        self_signed(match decl_of(s) {
            Decl::Approved { sig, since, .. } => Approval::Approved { sig, pr: 0, merge_sha: since },
            _ => Approval::Unreviewed(UnreviewedReason::DirectPush),
        }),
    ensures
        !holds_requirements(s, g),
{
}

/// A test-backed property never meets assertion 3, whatever else is true.
proof fn test_never_holds(scope_hash: Digest, decl: Decl, binding: Binding, g: Governance)
    ensures
        !holds_requirements(Status::Scoped { kind: Kind::Test, scope_hash, decl, binding }, g),
{
}

/// An empty scope never meets assertions 2 to 4.
proof fn scope_empty_never_holds(decl: Decl, g: Governance)
    ensures
        !holds_requirements(Status::ScopeEmpty { decl }, g),
{
}

/// `NoMutants` never meets assertion 4.
proof fn no_mutants_never_at_floor(b: Binding, floor: u32)
    requires
        b matches Binding::Bound { score: Score::Fresh(ScoreOutcome::NoMutants), .. },
    ensures
        !score_at_floor(b, floor),
{
}

/// A kind mismatch is never positive.
proof fn mismatch_never_positive()
    ensures
        !positive(Kind::Test, PrimaryOutcome::Verified),
        !positive(Kind::Proof, PrimaryOutcome::NoCounterexample),
        !positive(Kind::Model, PrimaryOutcome::NoCounterexample),
{
}

} // verus!
