//! Spec validation for assertion 6: over generated review histories, the
//! runtime `eligibility` satisfies a hand-written reading of
//! `eligibility_sound` and of the reason table, and `preview` agrees with it.
//! The proof is in `proob-core`; this catches an `ensures` that says the
//! wrong thing, written independently of `specs.rs`.
use proob_core::eligibility::eligibility;
use proob_core::preview::{Remedy, preview};
use proob_core::types::*;
use proptest::prelude::*;

fn reviewer() -> impl Strategy<Value = Reviewer> {
    (1u64..4, prop_oneof![Just(UserKind::Human), Just(UserKind::Bot), Just(UserKind::Unknown)], any::<bool>())
        .prop_map(|(id, kind, has_write_now)| Reviewer { id, kind, has_write_now })
}

fn state() -> impl Strategy<Value = ReviewState> {
    prop_oneof![
        Just(ReviewState::Approved),
        Just(ReviewState::ChangesRequested),
        Just(ReviewState::Commented),
        Just(ReviewState::Dismissed),
        Just(ReviewState::Pending),
    ]
}

fn blob() -> impl Strategy<Value = Option<Oid>> {
    prop_oneof![Just(None), Just(Some([1u8; 32])), Just(Some([2u8; 32]))]
}

fn review() -> impl Strategy<Value = Review> {
    (reviewer(), state(), blob()).prop_map(|(user, state, blob_at_commit)| Review { user, state, blob_at_commit })
}

/// A clonable stand-in for `PrFacts`, which the core keeps plain and un-`Clone`.
#[derive(Debug, Clone)]
struct Gen {
    number: u64,
    author: u64,
    authors: Vec<u64>,
    unresolved: bool,
    blob_at_merge: Oid,
    reviews: Vec<Review>,
}

impl Gen {
    fn facts(&self) -> PrFacts {
        PrFacts {
            number: self.number,
            merge_sha: [self.number as u8; 32],
            author: self.author,
            governed_change_authors: self.authors.clone(),
            unresolved_governed_author: self.unresolved,
            blob_at_merge: self.blob_at_merge,
            reviews: self.reviews.clone(),
        }
    }
}

fn generated() -> impl Strategy<Value = Gen> {
    (
        1u64..100,
        1u64..4,
        prop::collection::btree_set(1u64..4, 0..3),
        any::<bool>(),
        prop_oneof![Just([1u8; 32]), Just([2u8; 32])],
        prop::collection::vec(review(), 0..5),
    )
        .prop_map(|(number, author, authors, unresolved, blob_at_merge, reviews)| Gen {
            number,
            author,
            authors: authors.into_iter().collect(),
            unresolved,
            blob_at_merge,
            reviews,
        })
}

/// `None` is a direct push.
fn intro() -> impl Strategy<Value = Option<Gen>> {
    prop_oneof![1 => Just(None), 6 => generated().prop_map(Some)]
}

fn introduction(g: &Option<Gen>) -> Introduction {
    match g {
        None => Introduction::DirectPush,
        Some(g) => Introduction::Merged(g.facts()),
    }
}

/// The plan's prose for an eligible review, written without `specs.rs`.
fn eligible(f: &PrFacts, r: &Review) -> bool {
    r.state == ReviewState::Approved
        && r.user.kind == UserKind::Human
        && r.user.has_write_now
        && r.user.id != f.author
        && !f.governed_change_authors.contains(&r.user.id)
        && !f.unresolved_governed_author
        && r.blob_at_commit == Some(f.blob_at_merge)
}

fn revoked(f: &PrFacts, r: &Review) -> bool {
    r.state == ReviewState::Approved
        && r.user.kind == UserKind::Human
        && !r.user.has_write_now
        && r.user.id != f.author
        && !f.governed_change_authors.contains(&r.user.id)
        && !f.unresolved_governed_author
        && r.blob_at_commit == Some(f.blob_at_merge)
}

fn approving(f: &PrFacts) -> impl Iterator<Item = &Review> {
    f.reviews.iter().filter(|r| r.state == ReviewState::Approved)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    fn eligibility_is_sound(g in intro(), allow in any::<bool>()) {
        let intro = introduction(&g);
        let a = eligibility(&intro, allow);
        match (&intro, a) {
            (Introduction::DirectPush, a) => prop_assert_eq!(a, Approval::Unreviewed(UnreviewedReason::DirectPush)),
            (Introduction::Merged(f), Approval::Approved { sig, pr, merge_sha }) => {
                prop_assert_eq!(pr, f.number);
                prop_assert_eq!(merge_sha, f.merge_sha);
                let some_eligible = f.reviews.iter().any(|r| eligible(f, r));
                match sig {
                    Sig::Team => prop_assert!(some_eligible, "Team without an eligible review"),
                    Sig::SelfSigned => prop_assert!(allow && !some_eligible, "SelfSigned when not allowed or when eligible"),
                }
            }
            (Introduction::Merged(f), Approval::Unreviewed(why)) => {
                prop_assert!(!f.reviews.iter().any(|r| eligible(f, r)), "an eligible review was not honoured");
                match why {
                    UnreviewedReason::DirectPush => prop_assert!(false, "DirectPush for a merged PR"),
                    UnreviewedReason::ReviewerAccessRevoked => prop_assert!(f.reviews.iter().any(|r| revoked(f, r))),
                    UnreviewedReason::NoEligibleReview(_) => prop_assert!(!allow, "self-signed was allowed"),
                }
            }
        }
    }

    #[test]
    fn reason_consequences_hold(g in generated()) {
        let f = g.facts();
        let a = eligibility(&Introduction::Merged(g.facts()), false);
        if let Approval::Unreviewed(UnreviewedReason::NoEligibleReview(why)) = a {
            match why {
                Ineligible::NoReview => prop_assert_eq!(approving(&f).count(), 0),
                Ineligible::BotOnly => prop_assert!(approving(&f).all(|r| r.user.kind != UserKind::Human)),
                Ineligible::NoWriteAccess => prop_assert!(approving(&f).all(|r| !(r.user.kind == UserKind::Human && r.user.has_write_now))),
                Ineligible::ApproverEditedGoverned => prop_assert!(approving(&f)
                    .filter(|r| r.user.kind == UserKind::Human && r.user.has_write_now)
                    .all(|r| r.user.id == f.author || f.governed_change_authors.contains(&r.user.id))),
                Ineligible::ApprovalPredatesChange => prop_assert!(approving(&f)
                    .filter(|r| r.user.kind == UserKind::Human && r.user.has_write_now && r.user.id != f.author && !f.governed_change_authors.contains(&r.user.id))
                    .all(|r| r.blob_at_commit != Some(f.blob_at_merge))),
                Ineligible::UnknownIdentity => prop_assert!(f.unresolved_governed_author || approving(&f).all(|r| r.user.kind == UserKind::Unknown)),
            }
        }
    }

    #[test]
    fn preview_agrees_with_eligibility_and_its_table(g in intro(), allow in any::<bool>()) {
        let intro = introduction(&g);
        let p = preview(&intro, allow);
        prop_assert_eq!(p.approval, eligibility(&intro, allow));
        let want = match p.approval {
            Approval::Approved { sig: Sig::Team, .. } => Remedy::None,
            Approval::Approved { sig: Sig::SelfSigned, .. } => Remedy::OptionalSecondReviewer,
            Approval::Unreviewed(UnreviewedReason::DirectPush) => Remedy::OpenPullRequest,
            Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked) => Remedy::AddReviewerWithWriteAccess,
            Approval::Unreviewed(UnreviewedReason::NoEligibleReview(why)) => match why {
                Ineligible::NoReview | Ineligible::BotOnly | Ineligible::NoWriteAccess => Remedy::AddReviewerWithWriteAccess,
                Ineligible::ApproverEditedGoverned => Remedy::AskAuthorToCommitSuggestion,
                Ineligible::ApprovalPredatesChange => Remedy::ReApproveLatestCommit,
                Ineligible::UnknownIdentity => Remedy::ResolveIdentity,
            },
        };
        prop_assert_eq!(p.remedy, want);
    }
}
