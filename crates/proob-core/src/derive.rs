//! Status derivation: facts in, `Status` out.
//!
//! `derive` builds the nested `Status` of one property from `DeriveInput`
//! (types.rs) and the gate config. It is the only producer of `Primary::Fresh`
//! and `Score::Fresh`, so the "at the current hashes" and "within the window"
//! halves of assertions 3 and 4 are its `ensures`:
//!
//! - `Primary::Fresh(o)` only when the latest default-branch record of the
//!   property's kind (latest by `at`; a later index wins a tie) has the current
//!   scope hash and binding hash and measured `Primary(kind, o)`.
//! - `Score::Fresh(o)` only when the latest score record has the current
//!   binding hash and `at <= now && now - at <= freshness_secs`; a record dated
//!   after `now` is not fresh.
//!
//! The rest of the derivation is `spec_decl` for the declaration axis and the
//! rules below for the binding axis:
//!
//! - `scope` empty is `ScopeEmpty`; otherwise the scope hash is the verified
//!   `scope_hash::scope_hash` of the resolved files, which requires pairwise
//!   distinct paths (`paths_distinct`), a fact resolved files have by
//!   construction and `paths_distinct_exec` decides. The requirement is a
//!   precondition rather than a status: a duplicate path is a defect in the
//!   effects layer, not a state of the property.
//! - `Unbound(NoAdapter)` when no adapter covers the repository, else
//!   `Unbound(NoCheck)` when no binding facts exist, else `Bound`.
//! - Primary: `Fresh` as above; otherwise `Pending` when the latest accepted
//!   evidence run is not at the default head (a run is outstanding), else
//!   `Stale` when some record of the kind exists, else `NoEvidence`.
//! - Score: `Fresh` as above, else `Stale` when a score record exists, else
//!   `NoEvidence`.
//! - Provisional: this PR's `RunResult` when it is for this property and kind.
//!   A negative or errored result counts at any hashes (dropping it would fail
//!   open); a positive one counts only at the current hashes, since a positive
//!   result can suppress a default-branch negative (verdict row 20).
//! - Records for another property id are ignored; the facts layer filters by
//!   id, and the check here keeps the `ensures` honest without trusting it.
use crate::bytes::digest_eq;
use crate::scope_hash::{ScopeHash, scope_hash};
use crate::types::*;
use crate::verdict::positive_exec;
// Spec functions are erased by a plain `cargo build`; the guards keep these
// imports to verification only.
#[cfg(verus_only)]
use crate::scope_hash::{paths_distinct, spec_scope_hash_of};
#[cfg(verus_only)]
use crate::specs::decl_of;
use vstd::prelude::*;

verus! {

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// `m` is a primary measurement of this kind.
pub open spec fn is_primary_of(m: Measured, kind: Kind) -> bool {
    match m {
        Measured::Primary(k, _) => k == kind,
        Measured::Score(_) => false,
    }
}

/// Record `i` is a primary record for this property and kind.
pub open spec fn primary_candidate(records: Seq<Evidence>, id: PropertyId, kind: Kind, i: int) -> bool {
    0 <= i < records.len() && records[i].property == id && is_primary_of(records[i].measured, kind)
}

/// Record `i` is a primary candidate and no candidate is newer.
pub open spec fn latest_primary(records: Seq<Evidence>, id: PropertyId, kind: Kind, i: int) -> bool {
    primary_candidate(records, id, kind, i) && forall|j: int|
        #[trigger] primary_candidate(records, id, kind, j) ==> records[j].at <= records[i].at
}

/// Record `i` is a score record for this property.
pub open spec fn score_candidate(records: Seq<Evidence>, id: PropertyId, i: int) -> bool {
    0 <= i < records.len() && records[i].property == id && records[i].measured is Score
}

/// Record `i` is a score candidate and no candidate is newer.
pub open spec fn latest_score(records: Seq<Evidence>, id: PropertyId, i: int) -> bool {
    score_candidate(records, id, i) && forall|j: int|
        #[trigger] score_candidate(records, id, j) ==> records[j].at <= records[i].at
}

/// The waiver window at `now`.
pub open spec fn spec_window(until: Secs, issue_open: bool, now: Secs) -> Window {
    if now < until {
        Window::Active { until, issue_open }
    } else {
        Window::Expired { until }
    }
}

/// The declaration axis from the statement's approval and the waiver.
pub open spec fn spec_decl(
    statement: Approval,
    previously_approved: bool,
    drift: Drift,
    waiver: Option<Waiver>,
    issue_open: bool,
    now: Secs,
) -> Decl {
    match statement {
        Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked) => Decl::Proposed(
            ProposedWhy::ReviewerAccessRevoked,
        ),
        Approval::Unreviewed(_) => if previously_approved {
            Decl::Proposed(ProposedWhy::Amended)
        } else {
            Decl::Proposed(ProposedWhy::NeverReviewed)
        },
        Approval::Approved { sig, merge_sha, .. } => match waiver {
            None => Decl::Approved { sig, since: merge_sha, drift },
            Some(w) => Decl::Waived { sig, since: merge_sha, window: spec_window(w.until, issue_open, now) },
        },
    }
}

// ---------------------------------------------------------------------------
// The declaration axis
// ---------------------------------------------------------------------------

fn decl_of_facts(
    statement: Approval,
    previously_approved: bool,
    drift: Drift,
    waiver: Option<Waiver>,
    issue_open: bool,
    now: Secs,
) -> (d: Decl)
    ensures
        d == spec_decl(statement, previously_approved, drift, waiver, issue_open, now),
{
    match statement {
        Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked) => Decl::Proposed(
            ProposedWhy::ReviewerAccessRevoked,
        ),
        Approval::Unreviewed(_) => {
            if previously_approved {
                Decl::Proposed(ProposedWhy::Amended)
            } else {
                Decl::Proposed(ProposedWhy::NeverReviewed)
            }
        },
        Approval::Approved { sig, merge_sha, .. } => match waiver {
            None => Decl::Approved { sig, since: merge_sha, drift },
            Some(w) => {
                let window = if now < w.until {
                    Window::Active { until: w.until, issue_open }
                } else {
                    Window::Expired { until: w.until }
                };
                Decl::Waived { sig, since: merge_sha, window }
            },
        },
    }
}

// ---------------------------------------------------------------------------
// Evidence selection
// ---------------------------------------------------------------------------

fn is_primary_of_exec(m: Measured, kind: Kind) -> (r: bool)
    ensures
        r == is_primary_of(m, kind),
{
    match m {
        Measured::Primary(k, _) => k == kind,
        Measured::Score(_) => false,
    }
}

/// The index of the latest primary record of the kind, if any.
fn find_latest_primary(records: &Vec<Evidence>, id: PropertyId, kind: Kind) -> (r: Option<usize>)
    ensures
        r matches Some(i) ==> latest_primary(records@, id, kind, i as int),
        r is None ==> forall|j: int| !primary_candidate(records@, id, kind, j),
{
    let n = records.len();
    let mut best: Option<usize> = None;
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == records@.len(),
            best matches Some(b) ==> b < i && primary_candidate(records@, id, kind, b as int) && forall|j: int|
                0 <= j < i && #[trigger] primary_candidate(records@, id, kind, j) ==> records@[j].at
                    <= records@[b as int].at,
            best is None ==> forall|j: int| 0 <= j < i ==> !#[trigger] primary_candidate(records@, id, kind, j),
        decreases n - i,
    {
        let r = records[i];
        if r.property == id && is_primary_of_exec(r.measured, kind) {
            best = match best {
                Some(b) => if r.at >= records[b].at {
                    Some(i)
                } else {
                    Some(b)
                },
                None => Some(i),
            };
        }
        i += 1;
    }
    best
}

/// The index of the latest score record, if any.
fn find_latest_score(records: &Vec<Evidence>, id: PropertyId) -> (r: Option<usize>)
    ensures
        r matches Some(i) ==> latest_score(records@, id, i as int),
        r is None ==> forall|j: int| !score_candidate(records@, id, j),
{
    let n = records.len();
    let mut best: Option<usize> = None;
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == records@.len(),
            best matches Some(b) ==> b < i && score_candidate(records@, id, b as int) && forall|j: int|
                0 <= j < i && #[trigger] score_candidate(records@, id, j) ==> records@[j].at <= records@[b as int].at,
            best is None ==> forall|j: int| 0 <= j < i ==> !#[trigger] score_candidate(records@, id, j),
        decreases n - i,
    {
        let r = records[i];
        if r.property == id && matches!(r.measured, Measured::Score(_)) {
            best = match best {
                Some(b) => if r.at >= records[b].at {
                    Some(i)
                } else {
                    Some(b)
                },
                None => Some(i),
            };
        }
        i += 1;
    }
    best
}

/// The latest accepted evidence run is not at the default head.
fn run_outstanding(evidence_run_head: Option<Oid>, default_head: &Oid) -> (r: bool)
    ensures
        r == (evidence_run_head != Some(*default_head)),
{
    match evidence_run_head {
        None => true,
        Some(h) => !digest_eq(&h, default_head),
    }
}

/// Default-branch primary evidence relative to the current hashes.
fn primary_of(
    records: &Vec<Evidence>,
    id: PropertyId,
    kind: Kind,
    scope_hash: &Digest,
    binding_hash: &Digest,
    evidence_run_head: Option<Oid>,
    default_head: &Oid,
) -> (p: Primary)
    ensures
        p matches Primary::Fresh(o) ==> exists|i: int|
            latest_primary(records@, id, kind, i) && (#[trigger] records@[i]).scope_hash == *scope_hash
                && records@[i].binding_hash == *binding_hash && records@[i].measured == Measured::Primary(kind, o),
        p is Pending ==> evidence_run_head != Some(*default_head),
        p is NoEvidence ==> forall|j: int| !primary_candidate(records@, id, kind, j),
{
    match find_latest_primary(records, id, kind) {
        Some(i) => {
            let r = records[i];
            let at_hashes = digest_eq(&r.scope_hash, scope_hash) && digest_eq(&r.binding_hash, binding_hash);
            match (at_hashes, r.measured) {
                (true, Measured::Primary(_, o)) => Primary::Fresh(o),
                _ => if run_outstanding(evidence_run_head, default_head) {
                    Primary::Pending
                } else {
                    Primary::Stale
                },
            }
        },
        None => if run_outstanding(evidence_run_head, default_head) {
            Primary::Pending
        } else {
            Primary::NoEvidence
        },
    }
}

/// Default-branch score evidence relative to the current binding hash and the
/// freshness window.
fn score_of(records: &Vec<Evidence>, id: PropertyId, binding_hash: &Digest, now: Secs, freshness: Secs) -> (s: Score)
    ensures
        s matches Score::Fresh(o) ==> exists|i: int|
            latest_score(records@, id, i) && (#[trigger] records@[i]).binding_hash == *binding_hash
                && records@[i].measured == Measured::Score(o) && records@[i].at <= now && now - records@[i].at
                <= freshness,
        s is NoEvidence ==> forall|j: int| !score_candidate(records@, id, j),
{
    match find_latest_score(records, id) {
        Some(i) => {
            let r = records[i];
            let in_window = r.at <= now && now - r.at <= freshness;
            match (digest_eq(&r.binding_hash, binding_hash) && in_window, r.measured) {
                (true, Measured::Score(o)) => Score::Fresh(o),
                _ => Score::Stale,
            }
        },
        None => Score::NoEvidence,
    }
}

/// This PR's own run, when it is about this property and kind.
fn provisional_of(
    run: Option<RunResult>,
    id: PropertyId,
    kind: Kind,
    scope_hash: &Digest,
    binding_hash: &Digest,
) -> (p: Provisional)
    ensures
        p matches Provisional::Provisional(o) ==> (run matches Some(r) && r.property == id && r.measured
            == Measured::Primary(kind, o)),
        (p matches Provisional::Provisional(o) && crate::specs::positive(kind, o)) ==> (run matches Some(r)
            && r.scope_hash == *scope_hash && r.binding_hash == *binding_hash),
{
    match run {
        None => Provisional::None,
        Some(r) => match r.measured {
            Measured::Primary(k, o) => {
                let ours = r.property == id && k == kind;
                let at_hashes = digest_eq(&r.scope_hash, scope_hash) && digest_eq(&r.binding_hash, binding_hash);
                if ours && (!positive_exec(kind, o) || at_hashes) {
                    Provisional::Provisional(o)
                } else {
                    Provisional::None
                }
            },
            Measured::Score(_) => Provisional::None,
        },
    }
}

// ---------------------------------------------------------------------------
// derive
// ---------------------------------------------------------------------------

/// The status of one property from its facts, the gate config, and the instant.
// proob:01M622RQ73XGJ3PRRVCY53FECA
pub fn derive(input: DeriveInput, cfg: &GateConfig, now: Secs) -> (s: Status)
    requires
        paths_distinct(input.scope@),
    ensures
        // Shape.
        s is Scoped <==> input.scope@.len() > 0,
        s matches Status::Scoped { kind, scope_hash, .. } ==> kind == input.kind && scope_hash@ == spec_scope_hash_of(
            input.scope@.to_multiset(),
        ),
        // The declaration axis.
        decl_of(s) == spec_decl(
            input.statement,
            input.previously_approved,
            input.drift,
            input.property.waiver,
            input.waiver_issue_open,
            now,
        ),
        decl_of(s) is Waived ==> input.property.waiver is Some && input.statement is Approved,
        decl_of(s) matches Decl::Waived { window: Window::Active { until, .. }, .. } ==> now < until,
        decl_of(s) matches Decl::Waived { window: Window::Expired { until }, .. } ==> until <= now,
        // The binding axis.
        s matches Status::Scoped { binding: Binding::Unbound(UnboundWhy::NoAdapter), .. } ==> !input.adapter_available,
        s matches Status::Scoped { binding: Binding::Unbound(UnboundWhy::NoCheck), .. } ==> input.adapter_available
            && input.binding is None,
        s matches Status::Scoped { binding: Binding::Bound { binding_hash, approval, previously_approved, .. }, .. }
            ==> input.adapter_available && input.binding == Some(
            BindingFacts { binding_hash, approval, previously_approved },
        ),
        // Assertion 3, derive's half: `Fresh` primary evidence is the latest record
        // of the kind, at the current scope and binding hashes.
        s matches Status::Scoped { kind, scope_hash, binding: Binding::Bound { binding_hash, primary: Primary::Fresh(o), .. }, .. }
            ==> exists|i: int|
            latest_primary(input.records@, input.property.id, kind, i) && (#[trigger] input.records@[i]).scope_hash
                == scope_hash && input.records@[i].binding_hash == binding_hash && input.records@[i].measured
                == Measured::Primary(kind, o),
        // Assertion 4, derive's half: `Fresh` score evidence is the latest score
        // record, at the current binding hash, within the freshness window.
        s matches Status::Scoped { binding: Binding::Bound { binding_hash, score: Score::Fresh(o), .. }, .. }
            ==> exists|i: int|
            latest_score(input.records@, input.property.id, i) && (#[trigger] input.records@[i]).binding_hash
                == binding_hash && input.records@[i].measured == Measured::Score(o) && input.records@[i].at <= now
                && now - input.records@[i].at <= cfg.freshness_secs,
        // This PR's run reaches the status only as `Provisional`, and only its own.
        s matches Status::Scoped { kind, binding: Binding::Bound { provisional: Provisional::Provisional(o), .. }, .. }
            ==> (input.provisional matches Some(r) && r.property == input.property.id && r.measured
            == Measured::Primary(kind, o)),
{
    let kind = input.kind;
    let id = input.property.id;
    let decl = decl_of_facts(
        input.statement,
        input.previously_approved,
        input.drift,
        input.property.waiver,
        input.waiver_issue_open,
        now,
    );
    if input.scope.is_empty() {
        return Status::ScopeEmpty { decl };
    }
    let records = input.records;
    let hash = match scope_hash(input.scope) {
        ScopeHash::Ok(h) => h,
        ScopeHash::DuplicatePath => vstd::pervasive::unreached(),
    };
    let bound = if !input.adapter_available {
        Binding::Unbound(UnboundWhy::NoAdapter)
    } else {
        match input.binding {
            None => Binding::Unbound(UnboundWhy::NoCheck),
            Some(f) => Binding::Bound {
                binding_hash: f.binding_hash,
                approval: f.approval,
                previously_approved: f.previously_approved,
                primary: primary_of(
                    &records,
                    id,
                    kind,
                    &hash,
                    &f.binding_hash,
                    input.evidence_run_head,
                    &input.default_head,
                ),
                provisional: provisional_of(input.provisional, id, kind, &hash, &f.binding_hash),
                score: score_of(&records, id, &f.binding_hash, now, cfg.freshness_secs),
            },
        }
    };
    Status::Scoped { kind, scope_hash: hash, decl, binding: bound }
}

} // verus!
