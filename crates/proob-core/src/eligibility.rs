//! Eligibility: what GitHub's review records say about one governed file.
//!
//! `eligibility` is the verified function behind every approval. It reads an
//! `Introduction` (a direct push, or the facts of the merged PR that
//! introduced the file's current content) and answers an `Approval`.
//! Assertion 6 (`specs::eligibility_sound`) is its first `ensures`; the
//! second, `eligibility_reasoned`, pins the whole answer, reason included, so
//! that the words a reviewer reads are facts rather than guesses.
//!
//! The answer, first row that applies:
//!
//! | # | Condition | Approval |
//! |---|---|---|
//! | 1 | no associated PR | `Unreviewed(DirectPush)` |
//! | 2 | some review is eligible (`specs::eligible_review`) | `Approved { Team, pr, merge_sha }` |
//! | 3 | some review is eligible in every respect but write access now (`revoked_review`) | `Unreviewed(ReviewerAccessRevoked)` |
//! | 4 | the repository allows self-signed merges | `Approved { SelfSigned, pr, merge_sha }` |
//! | 5 | otherwise | `Unreviewed(NoEligibleReview(reason))` |
//!
//! Row 3 before row 4: GitHub keeps no record of permission at review time,
//! so an approver without write access now is read as an approver who lost it
//! (plan assumption A11). The file once carried a team signature, and the
//! honest word for losing one is `Re-approve`, not a silent downgrade to
//! self-signed.
//!
//! The reason in row 5 names the rule that stopped the best approving review,
//! so a reader knows the smallest thing that would change the answer. Each
//! approving review is given the rank of the first rule it fails, rules in
//! the order the eligibility rule checks them (`rank`): an unresolved
//! identity (the reviewer's, or a governed commit author's) is 0, a bot 1, no
//! write access 2, the PR author or a governed-change author 3, a review on a
//! commit whose blob differs from the merge's or cannot be fetched 4, and a
//! review passing every rule 5 (eligible, so never reached in row 5). The
//! reported reason is that of the highest rank attained (`best_rank`): the
//! review closest to eligible decides. With no approving review at all the
//! reason is `NoReview`. `NoWriteAccess` therefore means a non-writer's
//! approval that also fails a later rule; a non-writer's otherwise eligible
//! approval is row 3.
use crate::bytes::digest_eq;
use crate::types::*;
// Spec functions are erased by a plain `cargo build`; the guard keeps the
// import to verification only.
#[cfg(verus_only)]
use crate::specs::{eligibility_sound, eligible_review, has_eligible_review};
use vstd::prelude::*;

verus! {

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// The review approves.
pub open spec fn approving(r: Review) -> bool {
    r.state is Approved
}

/// Some review on the PR approves.
pub open spec fn has_approving_review(f: PrFacts) -> bool {
    exists|i: int| 0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i])
}

/// Eligible in every respect but write access now: the approver lost it.
pub open spec fn revoked_review(f: PrFacts, r: Review) -> bool {
    r.state is Approved && r.user.kind is Human && !r.user.has_write_now && r.user.id != f.author
        && !f.governed_change_authors@.contains(r.user.id) && !f.unresolved_governed_author
        && r.blob_at_commit == Some(f.blob_at_merge)
}

/// Some review on the PR is `revoked_review`.
pub open spec fn has_revoked_review(f: PrFacts) -> bool {
    exists|i: int| 0 <= i < f.reviews@.len() && revoked_review(f, #[trigger] f.reviews@[i])
}

/// The rank of the first rule a review fails, rules in the order the
/// eligibility rule checks them; 5 passes every rule.
pub open spec fn rank(f: PrFacts, r: Review) -> int {
    if f.unresolved_governed_author || r.user.kind is Unknown {
        0
    } else if r.user.kind is Bot {
        1
    } else if !r.user.has_write_now {
        2
    } else if r.user.id == f.author || f.governed_change_authors@.contains(r.user.id) {
        3
    } else if r.blob_at_commit != Some(f.blob_at_merge) {
        4
    } else {
        5
    }
}

/// The reason a rank names.
pub open spec fn reason_of_rank(k: int) -> Ineligible {
    if k == 0 {
        Ineligible::UnknownIdentity
    } else if k == 1 {
        Ineligible::BotOnly
    } else if k == 2 {
        Ineligible::NoWriteAccess
    } else if k == 3 {
        Ineligible::ApproverEditedGoverned
    } else {
        Ineligible::ApprovalPredatesChange
    }
}

/// `k` is the rank of some approving review and no approving review ranks
/// higher: the review closest to eligible has rank `k`.
pub open spec fn best_rank(f: PrFacts, k: int) -> bool {
    &&& exists|i: int| 0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) && rank(f, f.reviews@[i]) == k
    &&& forall|i: int| 0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) ==> rank(f, f.reviews@[i]) <= k
}

/// The reason row 5 reports.
pub open spec fn spec_reason(f: PrFacts, why: Ineligible) -> bool {
    if has_approving_review(f) {
        exists|k: int| #[trigger] best_rank(f, k) && why == reason_of_rank(k)
    } else {
        why == Ineligible::NoReview
    }
}

/// Rows 2 to 5, for a merged PR.
pub open spec fn merged_reasoned(f: PrFacts, allow_self_signed: bool, a: Approval) -> bool {
    if has_eligible_review(f) {
        a == Approval::Approved { sig: Sig::Team, pr: f.number, merge_sha: f.merge_sha }
    } else if has_revoked_review(f) {
        a == Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked)
    } else if allow_self_signed {
        a == Approval::Approved { sig: Sig::SelfSigned, pr: f.number, merge_sha: f.merge_sha }
    } else {
        a matches Approval::Unreviewed(UnreviewedReason::NoEligibleReview(why)) && spec_reason(f, why)
    }
}

/// The whole table: what `eligibility` answers, reason included.
pub open spec fn eligibility_reasoned(intro: Introduction, allow_self_signed: bool, a: Approval) -> bool {
    match intro {
        Introduction::DirectPush => a == Approval::Unreviewed(UnreviewedReason::DirectPush),
        Introduction::Merged(f) => merged_reasoned(f, allow_self_signed, a),
    }
}

// ---------------------------------------------------------------------------
// What the reasons mean
// ---------------------------------------------------------------------------

/// A review of rank 5 that approves is eligible.
proof fn lemma_rank_five_eligible(f: PrFacts, r: Review)
    requires
        approving(r),
        rank(f, r) == 5,
    ensures
        eligible_review(f, r),
{
}

/// The table implies assertion 6.
proof fn lemma_reasoned_sound(intro: Introduction, allow_self_signed: bool, a: Approval)
    requires
        eligibility_reasoned(intro, allow_self_signed, a),
    ensures
        eligibility_sound(intro, allow_self_signed, a),
{
}

/// What each reported reason says about every approving review on the PR.
proof fn lemma_reason_meaning(f: PrFacts, why: Ineligible)
    requires
        !has_eligible_review(f),
        spec_reason(f, why),
    ensures
        why == Ineligible::NoReview ==> !has_approving_review(f),
        why == Ineligible::UnknownIdentity ==> f.unresolved_governed_author || forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) ==> f.reviews@[i].user.kind is Unknown,
        why == Ineligible::BotOnly ==> forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) ==> !(f.reviews@[i].user.kind is Human),
        why == Ineligible::NoWriteAccess ==> forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) ==> !(f.reviews@[i].user.kind is Human
                && f.reviews@[i].user.has_write_now),
        why == Ineligible::ApproverEditedGoverned ==> forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) && f.reviews@[i].user.kind is Human
                && f.reviews@[i].user.has_write_now ==> (f.reviews@[i].user.id == f.author
                || f.governed_change_authors@.contains(f.reviews@[i].user.id)),
        why == Ineligible::ApprovalPredatesChange ==> forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) && f.reviews@[i].user.kind is Human
                && f.reviews@[i].user.has_write_now && f.reviews@[i].user.id != f.author
                && !f.governed_change_authors@.contains(f.reviews@[i].user.id) ==> f.reviews@[i].blob_at_commit
                != Some(f.blob_at_merge),
{
    if has_approving_review(f) {
        let k = choose|k: int| #[trigger] best_rank(f, k) && why == reason_of_rank(k);
        let w = choose|i: int| 0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) && rank(f, f.reviews@[i]) == k;
        if k == 5 {
            lemma_rank_five_eligible(f, f.reviews@[w]);
        }
        // A rank above 0 was attained, so no governed author is unresolved.
        assert(k >= 1 ==> !f.unresolved_governed_author);
    }
}

// ---------------------------------------------------------------------------
// Deciders
// ---------------------------------------------------------------------------

/// Decides `ids@.contains(id)`.
fn contains_id(ids: &Vec<u64>, id: u64) -> (r: bool)
    ensures
        r == ids@.contains(id),
{
    let n = ids.len();
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == ids@.len(),
            forall|k: int| 0 <= k < i ==> ids@[k] != id,
        decreases n - i,
    {
        if ids[i] == id {
            return true;
        }
        i += 1;
    }
    false
}

/// Decides `r.blob_at_commit == Some(f.blob_at_merge)`.
fn blob_matches(f: &PrFacts, r: &Review) -> (b: bool)
    ensures
        b == (r.blob_at_commit == Some(f.blob_at_merge)),
{
    match r.blob_at_commit {
        Some(blob) => digest_eq(&blob, &f.blob_at_merge),
        None => false,
    }
}

/// Decides `specs::eligible_review`.
pub fn is_eligible(f: &PrFacts, r: &Review) -> (b: bool)
    ensures
        b == eligible_review(*f, *r),
{
    r.state == ReviewState::Approved && r.user.kind == UserKind::Human && r.user.has_write_now
        && r.user.id != f.author && !contains_id(&f.governed_change_authors, r.user.id)
        && !f.unresolved_governed_author && blob_matches(f, r)
}

/// Decides `revoked_review`.
fn is_revoked(f: &PrFacts, r: &Review) -> (b: bool)
    ensures
        b == revoked_review(*f, *r),
{
    r.state == ReviewState::Approved && r.user.kind == UserKind::Human && !r.user.has_write_now
        && r.user.id != f.author && !contains_id(&f.governed_change_authors, r.user.id)
        && !f.unresolved_governed_author && blob_matches(f, r)
}

/// Computes `rank`.
fn rank_of(f: &PrFacts, r: &Review) -> (k: u8)
    ensures
        k as int == rank(*f, *r),
{
    if f.unresolved_governed_author || r.user.kind == UserKind::Unknown {
        0
    } else if r.user.kind == UserKind::Bot {
        1
    } else if !r.user.has_write_now {
        2
    } else if r.user.id == f.author || contains_id(&f.governed_change_authors, r.user.id) {
        3
    } else if !blob_matches(f, r) {
        4
    } else {
        5
    }
}

/// Computes `reason_of_rank`.
fn reason_of(k: u8) -> (why: Ineligible)
    ensures
        why == reason_of_rank(k as int),
{
    match k {
        0 => Ineligible::UnknownIdentity,
        1 => Ineligible::BotOnly,
        2 => Ineligible::NoWriteAccess,
        3 => Ineligible::ApproverEditedGoverned,
        _ => Ineligible::ApprovalPredatesChange,
    }
}

/// Decides `specs::has_eligible_review`.
fn any_eligible(f: &PrFacts) -> (r: bool)
    ensures
        r == has_eligible_review(*f),
{
    let n = f.reviews.len();
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == f.reviews@.len(),
            forall|k: int| 0 <= k < i ==> !eligible_review(*f, #[trigger] f.reviews@[k]),
        decreases n - i,
    {
        if is_eligible(f, &f.reviews[i]) {
            return true;
        }
        i += 1;
    }
    false
}

/// Decides `has_revoked_review`.
fn any_revoked(f: &PrFacts) -> (r: bool)
    ensures
        r == has_revoked_review(*f),
{
    let n = f.reviews.len();
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == f.reviews@.len(),
            forall|k: int| 0 <= k < i ==> !revoked_review(*f, #[trigger] f.reviews@[k]),
        decreases n - i,
    {
        if is_revoked(f, &f.reviews[i]) {
            return true;
        }
        i += 1;
    }
    false
}

/// The index of the approving review closest to eligible, if any approves. A
/// later review wins a tie, which changes nothing: equal ranks name one reason.
fn best_approving(f: &PrFacts) -> (r: Option<usize>)
    ensures
        r is None <==> !has_approving_review(*f),
        r matches Some(i) ==> i < f.reviews@.len() && approving(f.reviews@[i as int]) && best_rank(
            *f,
            rank(*f, f.reviews@[i as int]),
        ),
{
    let n = f.reviews.len();
    let mut best: Option<usize> = None;
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == f.reviews@.len(),
            best matches Some(b) ==> b < i && approving(f.reviews@[b as int]) && forall|j: int|
                0 <= j < i && approving(#[trigger] f.reviews@[j]) ==> rank(*f, f.reviews@[j]) <= rank(
                    *f,
                    f.reviews@[b as int],
                ),
            best is None ==> forall|j: int| 0 <= j < i ==> !approving(#[trigger] f.reviews@[j]),
        decreases n - i,
    {
        let r = f.reviews[i];
        if r.state == ReviewState::Approved {
            best = match best {
                Some(b) => if rank_of(f, &r) >= rank_of(f, &f.reviews[b]) {
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

/// Rows 2 to 5.
fn merged(f: &PrFacts, allow_self_signed: bool) -> (a: Approval)
    ensures
        merged_reasoned(*f, allow_self_signed, a),
{
    if any_eligible(f) {
        return Approval::Approved { sig: Sig::Team, pr: f.number, merge_sha: f.merge_sha };
    }
    if any_revoked(f) {
        return Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked);
    }
    if allow_self_signed {
        return Approval::Approved { sig: Sig::SelfSigned, pr: f.number, merge_sha: f.merge_sha };
    }
    let why = match best_approving(f) {
        None => Ineligible::NoReview,
        Some(i) => reason_of(rank_of(f, &f.reviews[i])),
    };
    Approval::Unreviewed(UnreviewedReason::NoEligibleReview(why))
}

// ---------------------------------------------------------------------------
// eligibility
// ---------------------------------------------------------------------------

/// The approval of one governed file's current content from the facts of its
/// introduction and the repository's `allow_self_signed` setting.
// proob:01M622RQ76XZBSVRNHW3PC3Z5S
pub fn eligibility(intro: &Introduction, allow_self_signed: bool) -> (a: Approval)
    ensures
        // Assertion 6.
        eligibility_sound(*intro, allow_self_signed, a),
        // The whole table, reason included.
        eligibility_reasoned(*intro, allow_self_signed, a),
        // Completeness: an eligible review is always honoured.
        *intro matches Introduction::Merged(f) ==> (has_eligible_review(f) ==> a == (Approval::Approved {
            sig: Sig::Team,
            pr: f.number,
            merge_sha: f.merge_sha,
        })),
        // What each word means about the PR's approving reviews.
        a == Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked) ==> (*intro matches Introduction::Merged(f)
            && has_revoked_review(f) && !has_eligible_review(f)),
        a == Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::NoReview)) ==> (*intro matches Introduction::Merged(f)
            && !has_approving_review(f)),
        a == Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::UnknownIdentity)) ==> (*intro matches Introduction::Merged(f)
            && (f.unresolved_governed_author || forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) ==> f.reviews@[i].user.kind is Unknown)),
        a == Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::BotOnly)) ==> (*intro matches Introduction::Merged(f)
            && forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) ==> !(f.reviews@[i].user.kind is Human)),
        a == Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::NoWriteAccess)) ==> (*intro matches Introduction::Merged(f)
            && forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) ==> !(f.reviews@[i].user.kind is Human
                && f.reviews@[i].user.has_write_now)),
        a == Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::ApproverEditedGoverned)) ==> (*intro matches Introduction::Merged(f)
            && forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) && f.reviews@[i].user.kind is Human
                && f.reviews@[i].user.has_write_now ==> (f.reviews@[i].user.id == f.author
                || f.governed_change_authors@.contains(f.reviews@[i].user.id))),
        a == Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::ApprovalPredatesChange)) ==> (*intro matches Introduction::Merged(f)
            && forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) && f.reviews@[i].user.kind is Human
                && f.reviews@[i].user.has_write_now && f.reviews@[i].user.id != f.author
                && !f.governed_change_authors@.contains(f.reviews@[i].user.id) ==> f.reviews@[i].blob_at_commit
                != Some(f.blob_at_merge)),
{
    let a = match intro {
        Introduction::DirectPush => Approval::Unreviewed(UnreviewedReason::DirectPush),
        Introduction::Merged(f) => merged(f, allow_self_signed),
    };
    proof {
        lemma_reasoned_sound(*intro, allow_self_signed, a);
        if let Introduction::Merged(f) = intro {
            if let Approval::Unreviewed(UnreviewedReason::NoEligibleReview(why)) = a {
                lemma_reason_meaning(*f, why);
            }
        }
    }
    a
}

} // verus!
