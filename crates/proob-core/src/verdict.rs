//! The verdict: the closed word a status renders as.
//!
//! `verdict` is a total, exhaustive function of `Status` and `Governance`. Its
//! `ensures` carry assertions 1 to 5 (ASSERTIONS.md), the meaning of each word
//! (a word implies its reason), and the precedence below, which decides what a
//! reader sees when more than one thing is true of a property. The table is
//! `spec_verdict`; the exec functions mirror it row for row and are proven
//! equal to it.
//!
//! Precedence (first row that applies wins; `ss` is the self-signed modifier:
//! the statement, the binding set, the config, or the evidence workflow is
//! `SelfSigned`; `tested` is the modifier for a test-backed property, whose
//! sampled run cannot speak for every input, so only a proof or a model
//! reaches `Holds`):
//!
//! | # | Condition | Verdict |
//! |---|---|---|
//! | 1 | decl `Proposed(NeverReviewed)` | `Proposed` |
//! | 2 | decl `Proposed(Amended)` | `ReApprove(StatementAmended)` |
//! | 3 | decl `Proposed(ReviewerAccessRevoked)` | `ReApprove(ReviewerAccessRevoked)` |
//! | 4 | decl `Waived`, window expired or issue closed | `Broken(WaiverExpired)` |
//! | 5 | `ScopeEmpty`, decl `Approved` or `Waived` | `Broken(ScopeEmpty)` |
//! | 6 | decl `Waived{Team, Active, open}`, binding approval `Unreviewed(ReviewerAccessRevoked)` | `ReApprove(ReviewerAccessRevoked)` |
//! | 7 | decl `Waived{Team, Active, open}`, binding `Unreviewed` and previously approved | `ReApprove(CheckAmended)` |
//! | 8 | decl `Waived{Team, Active, open}` | `Waived{until}` |
//! | 9 | this PR's run negative for the kind | `Broken(Negative(ThisPr))` |
//! | 10 | this PR's run errored | `Broken(RunnerErrored)` |
//! | 11 | kind `Model`, decl `Approved`, `drift.commits > 0` | `ReApprove(SpecDrifted(commits))` |
//! | 12 | binding approval `Unreviewed(ReviewerAccessRevoked)` | `ReApprove(ReviewerAccessRevoked)` |
//! | 13 | binding `Unreviewed` and previously approved | `ReApprove(CheckAmended)` |
//! | 14 | `Unbound(NoAdapter)` | `NotChecked{NoAdapter, ss}` |
//! | 15 | `Unbound(NoCheck)` | `NotChecked{NoCheck, ss}` |
//! | 16 | binding `Unreviewed`, never approved | `NotChecked{CheckUnreviewed, ss}` |
//! | 17 | primary `NoEvidence` or `Pending` | `NotChecked{EvidencePending, ss}` |
//! | 18 | primary `Stale` | `NotChecked{EvidenceStale, ss}` |
//! | 19 | primary `Fresh(Errored)`, or an outcome the kind cannot produce | `NotChecked{AdapterErrored, ss}` |
//! | 20 | primary `Fresh(negative)`, this PR's run positive | `NotChecked{EvidencePending, ss}` |
//! | 21 | primary `Fresh(negative)` | `Broken(Negative(DefaultBranch))` |
//! | 22 | primary `Fresh(positive)`, config `Unreviewed` | `ReApprove(ConfigAmended)` |
//! | 23 | primary `Fresh(positive)`, evidence workflow `Unreviewed` | `ReApprove(WorkflowAmended)` |
//! | 24 | primary `Fresh(positive)`, kind `Proof` or `Model`, no self-signature, score fresh at the floor | `Holds` |
//! | 25 | primary `Fresh(positive)` | `Checked{tested: kind is Test, strength_pending: score not at floor, self_signed: ss}` |
//!
//! Rows 9 to 25 apply to a signed statement: decl `Approved{sig}`, or decl
//! `Waived{SelfSigned, Active, open}`, which is read as `Approved{SelfSigned}`
//! without drift (a self-signed waiver has no second signature, so it shields
//! nothing and cannot carry a model's drift).
//!
//! Choices the plan left open, fixed here:
//!
//! - An active team-approved waiver (rows 6 to 8) sits above this PR's own
//!   negative or errored run: the waiver exists because the property fails,
//!   and a PR touching the scope reproduces that failure; blocking it would
//!   make waivers useless. An expired or closed waiver is `Broken` first (4).
//! - A default-branch negative (21) ranks below the re-approval rows because
//!   that evidence is about the approved check; an amended check or a drifted
//!   spec supersedes it. It ranks below row 20 because this PR's positive run
//!   on the same check is a fix in flight: the honest word is "evidence
//!   pending" until the post-merge run. A PR that affects a property always
//!   has a provisional result, so row 21 blocks nothing a PR did not do.
//! - `ConfigAmended` and `WorkflowAmended` are per-property words only where
//!   the property would otherwise be `Holds` or `Checked` (22, 23): those two
//!   words promise approved governance. Everywhere else the property keeps its
//!   own word and `governance_verdicts` lists the amendments once for the
//!   headline. A `SelfSigned` config or workflow is not an amendment; it joins
//!   the self-signed modifier, so a single-maintainer repository reads
//!   `Checked (self-signed)`, never `Holds`.
//! - `NotChecked(GitHubUnreachable | RateLimited)` is never produced here; the
//!   CLI (U4) sets it from an `Unverified` view, where there are no facts.
//! - A `Fresh` primary outcome the kind cannot produce (a kind mismatch, which
//!   `derive` and the facts crate already exclude) reads `AdapterErrored`.
// Spec predicates are erased by a plain `cargo build`; the guard keeps the
// import to verification only.
#[cfg(verus_only)]
use crate::specs::*;
use crate::types::*;
use vstd::prelude::*;

verus! {

// ---------------------------------------------------------------------------
// Deciders for the outcome vocabulary
// ---------------------------------------------------------------------------

/// Decides `specs::positive`.
pub fn positive_exec(kind: Kind, o: PrimaryOutcome) -> (r: bool)
    ensures
        r == positive(kind, o),
{
    matches!(
        (kind, o),
        (Kind::Test, PrimaryOutcome::NoCounterexample)
            | (Kind::Proof, PrimaryOutcome::Verified)
            | (Kind::Model, PrimaryOutcome::NoViolation)
    )
}

/// Decides `specs::negative`.
pub fn negative_exec(kind: Kind, o: PrimaryOutcome) -> (r: bool)
    ensures
        r == negative(kind, o),
{
    matches!(
        (kind, o),
        (Kind::Test, PrimaryOutcome::Counterexample)
            | (Kind::Proof, PrimaryOutcome::ProofFailed)
            | (Kind::Model, PrimaryOutcome::Violation)
    )
}

// ---------------------------------------------------------------------------
// The table, as specification
// ---------------------------------------------------------------------------

/// The waiver no longer covers the property: its window has passed or its
/// issue is closed.
pub open spec fn waiver_lapsed(d: Decl) -> bool {
    match d {
        Decl::Waived { window: Window::Expired { .. }, .. } => true,
        Decl::Waived { window: Window::Active { issue_open, .. }, .. } => !issue_open,
        _ => false,
    }
}

/// The statement carries a self-signature.
pub open spec fn self_signed_decl(d: Decl) -> bool {
    match d {
        Decl::Approved { sig: Sig::SelfSigned, .. } => true,
        Decl::Waived { sig: Sig::SelfSigned, .. } => true,
        _ => false,
    }
}

/// Rows 1 to 3.
pub open spec fn spec_verdict_proposed(why: ProposedWhy) -> Verdict {
    match why {
        ProposedWhy::NeverReviewed => Verdict::Proposed,
        ProposedWhy::Amended => Verdict::ReApprove(ReApproveWhy::StatementAmended),
        ProposedWhy::ReviewerAccessRevoked => Verdict::ReApprove(ReApproveWhy::ReviewerAccessRevoked),
    }
}

/// Rows 9 and 10: what this PR's own run broke, if anything.
pub open spec fn spec_this_pr(kind: Kind, b: Binding) -> Option<BrokenWhy> {
    match b {
        Binding::Bound { provisional: Provisional::Provisional(o), .. } => if negative(kind, o) {
            Some(BrokenWhy::Negative(Source::ThisPr))
        } else if o is Errored {
            Some(BrokenWhy::RunnerErrored)
        } else {
            None
        },
        _ => None,
    }
}

/// Row 11: a model's scope commits since its approval, when non-zero.
pub open spec fn spec_drifted(kind: Kind, drift: Option<Drift>) -> Option<u64> {
    match (kind, drift) {
        (Kind::Model, Some(d)) => if d.commits > 0 {
            Some(d.commits)
        } else {
            None
        },
        _ => None,
    }
}

/// A model-backed, approved property whose scope has moved since approval and
/// whose own PR run has not broken it: the commits, when row 11 applies.
pub open spec fn model_drift_unbroken(s: Status) -> Option<u64> {
    match s {
        Status::Scoped { kind: Kind::Model, decl: Decl::Approved { drift, .. }, binding, .. } => if drift.commits > 0
            && spec_this_pr(Kind::Model, binding) is None {
            Some(drift.commits)
        } else {
            None
        },
        _ => None,
    }
}

/// Rows 6, 7, 12, 13: the binding axis's own re-approval reason.
pub open spec fn spec_binding_reapprove(b: Binding) -> Option<ReApproveWhy> {
    match b {
        Binding::Bound { approval: Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked), .. } => Some(
            ReApproveWhy::ReviewerAccessRevoked,
        ),
        Binding::Bound { approval: Approval::Unreviewed(_), previously_approved: true, .. } => Some(
            ReApproveWhy::CheckAmended,
        ),
        _ => None,
    }
}

/// This PR's run is positive for the kind.
pub open spec fn provisional_positive(kind: Kind, p: Provisional) -> bool {
    match p {
        Provisional::Provisional(o) => positive(kind, o),
        Provisional::None => false,
    }
}

/// The config or the evidence workflow is self-signed.
pub open spec fn governance_self_signed(g: Governance) -> bool {
    self_signed(g.config) || self_signed(g.evidence_workflow)
}

/// Fresh score evidence at or above a floor of at least one.
pub open spec fn spec_at_floor(score: Score, floor: u32) -> bool {
    1 <= floor && match score {
        Score::Fresh(ScoreOutcome::Scored { killed, .. }) => floor <= killed,
        _ => false,
    }
}

/// Rows 22 to 25.
pub open spec fn spec_verdict_positive(kind: Kind, ss: bool, score: Score, g: Governance) -> Verdict {
    if g.config is Unreviewed {
        Verdict::ReApprove(ReApproveWhy::ConfigAmended)
    } else if g.evidence_workflow is Unreviewed {
        Verdict::ReApprove(ReApproveWhy::WorkflowAmended)
    } else {
        let tested = kind is Test;
        let self_signed = ss || governance_self_signed(g);
        let at_floor = spec_at_floor(score, g.floor);
        if !tested && !self_signed && at_floor {
            Verdict::Holds
        } else {
            Verdict::Checked { tested, strength_pending: !at_floor, self_signed }
        }
    }
}

/// Rows 17 to 25.
pub open spec fn spec_verdict_evidence(
    kind: Kind,
    ss: bool,
    primary: Primary,
    provisional: Provisional,
    score: Score,
    g: Governance,
) -> Verdict {
    match primary {
        Primary::NoEvidence => Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: ss },
        Primary::Pending => Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: ss },
        Primary::Stale => Verdict::NotChecked { reason: NotCheckedWhy::EvidenceStale, self_signed: ss },
        Primary::Fresh(o) => if negative(kind, o) {
            if provisional_positive(kind, provisional) {
                Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: ss }
            } else {
                Verdict::Broken(BrokenWhy::Negative(Source::DefaultBranch))
            }
        } else if positive(kind, o) {
            spec_verdict_positive(kind, ss, score, g)
        } else {
            Verdict::NotChecked { reason: NotCheckedWhy::AdapterErrored, self_signed: ss }
        },
    }
}

/// Rows 14 to 25.
pub open spec fn spec_verdict_binding(kind: Kind, ss: bool, b: Binding, g: Governance) -> Verdict {
    match b {
        Binding::Unbound(UnboundWhy::NoAdapter) => Verdict::NotChecked {
            reason: NotCheckedWhy::NoAdapter,
            self_signed: ss,
        },
        Binding::Unbound(UnboundWhy::NoCheck) => Verdict::NotChecked { reason: NotCheckedWhy::NoCheck, self_signed: ss },
        Binding::Bound { approval: Approval::Unreviewed(_), .. } => Verdict::NotChecked {
            reason: NotCheckedWhy::CheckUnreviewed,
            self_signed: ss,
        },
        Binding::Bound { approval: Approval::Approved { sig, .. }, primary, provisional, score, .. } => spec_verdict_evidence(
            kind,
            ss || sig is SelfSigned,
            primary,
            provisional,
            score,
            g,
        ),
    }
}

/// Rows 9 to 25, for a signed statement.
pub open spec fn spec_verdict_signed(kind: Kind, sig: Sig, drift: Option<Drift>, b: Binding, g: Governance) -> Verdict {
    match spec_this_pr(kind, b) {
        Some(why) => Verdict::Broken(why),
        None => match spec_drifted(kind, drift) {
            Some(n) => Verdict::ReApprove(ReApproveWhy::SpecDrifted(n)),
            None => match spec_binding_reapprove(b) {
                Some(why) => Verdict::ReApprove(why),
                None => spec_verdict_binding(kind, sig is SelfSigned, b, g),
            },
        },
    }
}

/// Rows 6 to 8.
pub open spec fn spec_verdict_waived(until: Secs, b: Binding) -> Verdict {
    match spec_binding_reapprove(b) {
        Some(why) => Verdict::ReApprove(why),
        None => Verdict::Waived { until },
    }
}

/// A `Scoped` status.
pub open spec fn spec_verdict_scoped(kind: Kind, decl: Decl, b: Binding, g: Governance) -> Verdict {
    match decl {
        Decl::Proposed(why) => spec_verdict_proposed(why),
        Decl::Waived { sig, window, .. } => if waiver_lapsed(decl) {
            Verdict::Broken(BrokenWhy::WaiverExpired)
        } else {
            match (sig, window) {
                (Sig::Team, Window::Active { until, .. }) => spec_verdict_waived(until, b),
                _ => spec_verdict_signed(kind, Sig::SelfSigned, None, b, g),
            }
        },
        Decl::Approved { sig, drift, .. } => spec_verdict_signed(kind, sig, Some(drift), b, g),
    }
}

/// A `ScopeEmpty` status: rows 1 to 5.
pub open spec fn spec_verdict_scope_empty(decl: Decl) -> Verdict {
    match decl {
        Decl::Proposed(why) => spec_verdict_proposed(why),
        Decl::Waived { .. } => if waiver_lapsed(decl) {
            Verdict::Broken(BrokenWhy::WaiverExpired)
        } else {
            Verdict::Broken(BrokenWhy::ScopeEmpty)
        },
        Decl::Approved { .. } => Verdict::Broken(BrokenWhy::ScopeEmpty),
    }
}

/// The table.
pub open spec fn spec_verdict(s: Status, g: Governance) -> Verdict {
    match s {
        Status::ScopeEmpty { decl } => spec_verdict_scope_empty(decl),
        Status::Scoped { kind, decl, binding, .. } => spec_verdict_scoped(kind, decl, binding, g),
    }
}

// ---------------------------------------------------------------------------
// The table, as code
// ---------------------------------------------------------------------------

fn waiver_lapsed_exec(d: Decl) -> (r: bool)
    ensures
        r == waiver_lapsed(d),
{
    match d {
        Decl::Waived { window: Window::Expired { .. }, .. } => true,
        Decl::Waived { window: Window::Active { issue_open, .. }, .. } => !issue_open,
        _ => false,
    }
}

fn verdict_proposed(why: ProposedWhy) -> (v: Verdict)
    ensures
        v == spec_verdict_proposed(why),
{
    match why {
        ProposedWhy::NeverReviewed => Verdict::Proposed,
        ProposedWhy::Amended => Verdict::ReApprove(ReApproveWhy::StatementAmended),
        ProposedWhy::ReviewerAccessRevoked => Verdict::ReApprove(ReApproveWhy::ReviewerAccessRevoked),
    }
}

fn this_pr(kind: Kind, b: Binding) -> (r: Option<BrokenWhy>)
    ensures
        r == spec_this_pr(kind, b),
{
    match b {
        Binding::Bound { provisional: Provisional::Provisional(o), .. } => {
            if negative_exec(kind, o) {
                Some(BrokenWhy::Negative(Source::ThisPr))
            } else if o == PrimaryOutcome::Errored {
                Some(BrokenWhy::RunnerErrored)
            } else {
                None
            }
        },
        _ => None,
    }
}

fn drifted(kind: Kind, drift: Option<Drift>) -> (r: Option<u64>)
    ensures
        r == spec_drifted(kind, drift),
{
    match (kind, drift) {
        (Kind::Model, Some(d)) => if d.commits > 0 {
            Some(d.commits)
        } else {
            None
        },
        _ => None,
    }
}

fn binding_reapprove(b: Binding) -> (r: Option<ReApproveWhy>)
    ensures
        r == spec_binding_reapprove(b),
{
    match b {
        Binding::Bound { approval: Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked), .. } => Some(
            ReApproveWhy::ReviewerAccessRevoked,
        ),
        Binding::Bound { approval: Approval::Unreviewed(_), previously_approved: true, .. } => Some(
            ReApproveWhy::CheckAmended,
        ),
        _ => None,
    }
}

fn provisional_positive_exec(kind: Kind, p: Provisional) -> (r: bool)
    ensures
        r == provisional_positive(kind, p),
{
    match p {
        Provisional::Provisional(o) => positive_exec(kind, o),
        Provisional::None => false,
    }
}

fn is_self_signed(a: Approval) -> (r: bool)
    ensures
        r == self_signed(a),
{
    matches!(a, Approval::Approved { sig: Sig::SelfSigned, .. })
}

fn at_floor(score: Score, floor: u32) -> (r: bool)
    ensures
        r == spec_at_floor(score, floor),
{
    match score {
        Score::Fresh(ScoreOutcome::Scored { killed, .. }) => 1 <= floor && floor <= killed,
        _ => false,
    }
}

// proob:01M622RQ74SFCQ8Y800YE3XNS9
fn verdict_positive(kind: Kind, ss: bool, score: Score, g: &Governance) -> (v: Verdict)
    ensures
        v == spec_verdict_positive(kind, ss, score, *g),
{
    if matches!(g.config, Approval::Unreviewed(_)) {
        return Verdict::ReApprove(ReApproveWhy::ConfigAmended);
    }
    if matches!(g.evidence_workflow, Approval::Unreviewed(_)) {
        return Verdict::ReApprove(ReApproveWhy::WorkflowAmended);
    }
    let tested = matches!(kind, Kind::Test);
    let self_signed = ss || is_self_signed(g.config) || is_self_signed(g.evidence_workflow);
    let strong = at_floor(score, g.floor);
    if !tested && !self_signed && strong {
        Verdict::Holds
    } else {
        Verdict::Checked { tested, strength_pending: !strong, self_signed }
    }
}

fn verdict_evidence(
    kind: Kind,
    ss: bool,
    primary: Primary,
    provisional: Provisional,
    score: Score,
    g: &Governance,
) -> (v: Verdict)
    ensures
        v == spec_verdict_evidence(kind, ss, primary, provisional, score, *g),
{
    match primary {
        Primary::NoEvidence => Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: ss },
        Primary::Pending => Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: ss },
        Primary::Stale => Verdict::NotChecked { reason: NotCheckedWhy::EvidenceStale, self_signed: ss },
        Primary::Fresh(o) => {
            if negative_exec(kind, o) {
                if provisional_positive_exec(kind, provisional) {
                    Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: ss }
                } else {
                    Verdict::Broken(BrokenWhy::Negative(Source::DefaultBranch))
                }
            } else if positive_exec(kind, o) {
                verdict_positive(kind, ss, score, g)
            } else {
                Verdict::NotChecked { reason: NotCheckedWhy::AdapterErrored, self_signed: ss }
            }
        },
    }
}

// proob:01M622RQ72HFMBG9RTQBMB7YBE
fn verdict_binding(kind: Kind, ss: bool, b: Binding, g: &Governance) -> (v: Verdict)
    ensures
        v == spec_verdict_binding(kind, ss, b, *g),
{
    match b {
        Binding::Unbound(UnboundWhy::NoAdapter) => Verdict::NotChecked {
            reason: NotCheckedWhy::NoAdapter,
            self_signed: ss,
        },
        Binding::Unbound(UnboundWhy::NoCheck) => Verdict::NotChecked { reason: NotCheckedWhy::NoCheck, self_signed: ss },
        Binding::Bound { approval: Approval::Unreviewed(_), .. } => Verdict::NotChecked {
            reason: NotCheckedWhy::CheckUnreviewed,
            self_signed: ss,
        },
        Binding::Bound { approval: Approval::Approved { sig, .. }, primary, provisional, score, .. } => {
            verdict_evidence(kind, ss || sig == Sig::SelfSigned, primary, provisional, score, g)
        },
    }
}

fn verdict_signed(kind: Kind, sig: Sig, drift: Option<Drift>, b: Binding, g: &Governance) -> (v: Verdict)
    ensures
        v == spec_verdict_signed(kind, sig, drift, b, *g),
{
    if let Some(why) = this_pr(kind, b) {
        return Verdict::Broken(why);
    }
    if let Some(n) = drifted(kind, drift) {
        return Verdict::ReApprove(ReApproveWhy::SpecDrifted(n));
    }
    if let Some(why) = binding_reapprove(b) {
        return Verdict::ReApprove(why);
    }
    verdict_binding(kind, sig == Sig::SelfSigned, b, g)
}

// proob:01M622RQ75T3CFHXPNAAHR3PPD
fn verdict_scoped(kind: Kind, decl: Decl, b: Binding, g: &Governance) -> (v: Verdict)
    ensures
        v == spec_verdict_scoped(kind, decl, b, *g),
{
    match decl {
        Decl::Proposed(why) => verdict_proposed(why),
        Decl::Waived { sig, window, .. } => {
            if waiver_lapsed_exec(decl) {
                return Verdict::Broken(BrokenWhy::WaiverExpired);
            }
            match (sig, window) {
                (Sig::Team, Window::Active { until, .. }) => match binding_reapprove(b) {
                    Some(why) => Verdict::ReApprove(why),
                    None => Verdict::Waived { until },
                },
                _ => verdict_signed(kind, Sig::SelfSigned, None, b, g),
            }
        },
        Decl::Approved { sig, drift, .. } => verdict_signed(kind, sig, Some(drift), b, g),
    }
}

fn verdict_scope_empty(decl: Decl) -> (v: Verdict)
    ensures
        v == spec_verdict_scope_empty(decl),
{
    match decl {
        Decl::Proposed(why) => verdict_proposed(why),
        Decl::Waived { .. } => {
            if waiver_lapsed_exec(decl) {
                Verdict::Broken(BrokenWhy::WaiverExpired)
            } else {
                Verdict::Broken(BrokenWhy::ScopeEmpty)
            }
        },
        Decl::Approved { .. } => Verdict::Broken(BrokenWhy::ScopeEmpty),
    }
}

/// The verdict of one property.
// proob:01M622RQ71P6ZXCKVVTRCA4DSE
pub fn verdict(s: &Status, g: &Governance) -> (v: Verdict)
    ensures
        // The table.
        v == spec_verdict(*s, *g),
        // Assertions 1 to 4, 5, and the well-formedness of `Checked`.
        v is Holds ==> holds_requirements(*s, *g),
        v is Waived ==> waived_requirements(*s),
        verdict_wf(v),
        // This PR's own run (rows 9 and 10), and what the words mean.
        (*s matches Status::Scoped { kind, decl: Decl::Approved { .. }, binding: Binding::Bound { provisional: Provisional::Provisional(o), .. }, .. }
            && negative(kind, o)) ==> v == Verdict::Broken(BrokenWhy::Negative(Source::ThisPr)),
        (*s matches Status::Scoped { decl: Decl::Approved { .. }, binding: Binding::Bound { provisional: Provisional::Provisional(PrimaryOutcome::Errored), .. }, .. })
            ==> v == Verdict::Broken(BrokenWhy::RunnerErrored),
        v == Verdict::Broken(BrokenWhy::Negative(Source::ThisPr)) ==> (*s matches Status::Scoped { kind, binding: Binding::Bound { provisional: Provisional::Provisional(o), .. }, .. }
            && negative(kind, o)),
        v == Verdict::Broken(BrokenWhy::RunnerErrored) ==> (*s matches Status::Scoped { binding: Binding::Bound { provisional: Provisional::Provisional(PrimaryOutcome::Errored), .. }, .. }),
        v == Verdict::Broken(BrokenWhy::Negative(Source::DefaultBranch)) ==> (*s matches Status::Scoped { kind, binding: Binding::Bound { primary: Primary::Fresh(o), .. }, .. }
            && negative(kind, o)),
        // Self-signed content is never `Holds` and never `Proposed`.
        self_signed_decl(decl_of(*s)) ==> !(v is Holds) && !(v is Proposed),
        // An empty scope and a lapsed waiver are broken (rows 4 and 5).
        (decl_of(*s) is Approved && s is ScopeEmpty) ==> v == Verdict::Broken(BrokenWhy::ScopeEmpty),
        v == Verdict::Broken(BrokenWhy::ScopeEmpty) ==> s is ScopeEmpty,
        waiver_lapsed(decl_of(*s)) ==> v == Verdict::Broken(BrokenWhy::WaiverExpired),
        v == Verdict::Broken(BrokenWhy::WaiverExpired) ==> waiver_lapsed(decl_of(*s)),
        // The statement axis (rows 1 to 3).
        v is Proposed <==> decl_of(*s) == Decl::Proposed(ProposedWhy::NeverReviewed),
        v == Verdict::ReApprove(ReApproveWhy::StatementAmended) <==> decl_of(*s) == Decl::Proposed(ProposedWhy::Amended),
        decl_of(*s) == Decl::Proposed(ProposedWhy::ReviewerAccessRevoked) ==> v == Verdict::ReApprove(ReApproveWhy::ReviewerAccessRevoked),
        // A drifted model (row 11), once this PR's run has not broken it.
        model_drift_unbroken(*s) matches Some(n) ==> v == Verdict::ReApprove(ReApproveWhy::SpecDrifted(n)),
        v matches Verdict::ReApprove(ReApproveWhy::SpecDrifted(n)) ==> (*s matches Status::Scoped { kind: Kind::Model, decl: Decl::Approved { drift, .. }, .. }
            && drift.commits == n && n > 0),
        // The binding axis's amendments (rows 7 and 13).
        v == Verdict::ReApprove(ReApproveWhy::CheckAmended) ==> (*s matches Status::Scoped { binding: Binding::Bound { approval: Approval::Unreviewed(_), previously_approved: true, .. }, .. }),
        // Governance (rows 22 and 23): `Holds` and `Checked` promise approved config and workflow.
        v == Verdict::ReApprove(ReApproveWhy::ConfigAmended) ==> g.config is Unreviewed,
        v == Verdict::ReApprove(ReApproveWhy::WorkflowAmended) ==> g.evidence_workflow is Unreviewed,
        (v is Holds || v is Checked) ==> !(g.config is Unreviewed) && !(g.evidence_workflow is Unreviewed),
{
    match *s {
        Status::ScopeEmpty { decl } => verdict_scope_empty(decl),
        Status::Scoped { kind, decl, binding, .. } => verdict_scoped(kind, decl, binding, g),
    }
}

/// The repository-wide amendments for the headline: `ConfigAmended` when the
/// config is `Unreviewed`, `WorkflowAmended` when the evidence workflow is.
/// The gate workflow is a footer fact, not a gate on any verdict.
pub fn governance_verdicts(g: &Governance) -> (r: Vec<ReApproveWhy>)
    ensures
        r@.contains(ReApproveWhy::ConfigAmended) <==> g.config is Unreviewed,
        r@.contains(ReApproveWhy::WorkflowAmended) <==> g.evidence_workflow is Unreviewed,
        forall|i: int|
            0 <= i < r@.len() ==> #[trigger] r@[i] == ReApproveWhy::ConfigAmended || r@[i]
                == ReApproveWhy::WorkflowAmended,
        r@.len() <= 2,
{
    let mut out: Vec<ReApproveWhy> = Vec::new();
    if matches!(g.config, Approval::Unreviewed(_)) {
        out.push(ReApproveWhy::ConfigAmended);
    }
    if matches!(g.evidence_workflow, Approval::Unreviewed(_)) {
        out.push(ReApproveWhy::WorkflowAmended);
    }
    proof {
        if g.config is Unreviewed {
            assert(out@[0] == ReApproveWhy::ConfigAmended);
        }
        if g.evidence_workflow is Unreviewed {
            assert(out@[out@.len() - 1] == ReApproveWhy::WorkflowAmended);
        }
    }
    out
}

} // verus!
