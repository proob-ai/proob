//! The pre-merge preview: the "if merged now" row of a PR's check.
//!
//! Before a PR merges, the facts layer builds the `Introduction` the governed
//! file would have if the PR merged at this moment (its current head as the
//! merge, its current reviews, permissions read now) and `preview` answers
//! what `eligibility` would say, paired with the one `Remedy` that changes
//! that answer. The approval is the same verified function, so the preview
//! can never promise more than the post-merge derivation will give.
//!
//! Remedies, one per approval, fixed by `spec_remedy` and carried as an
//! `ensures` each:
//!
//! | Approval | Remedy |
//! |---|---|
//! | `Approved { Team }` | `None` |
//! | `Approved { SelfSigned }` | `OptionalSecondReviewer` |
//! | `Unreviewed(DirectPush)` | `OpenPullRequest` |
//! | `Unreviewed(ReviewerAccessRevoked)` | `AddReviewerWithWriteAccess` |
//! | `NoEligibleReview(NoReview \| BotOnly \| NoWriteAccess)` | `AddReviewerWithWriteAccess` |
//! | `NoEligibleReview(ApproverEditedGoverned)` | `AskAuthorToCommitSuggestion` |
//! | `NoEligibleReview(ApprovalPredatesChange)` | `ReApproveLatestCommit` |
//! | `NoEligibleReview(UnknownIdentity)` | `ResolveIdentity` |
use crate::eligibility::eligibility;
use crate::types::*;
// Spec functions are erased by a plain `cargo build`; the guards keep these
// imports to verification only.
#[cfg(verus_only)]
use crate::eligibility::eligibility_reasoned;
#[cfg(verus_only)]
use crate::specs::{eligibility_sound, self_signed, team_approved};
use vstd::prelude::*;

verus! {

/// The smallest thing that changes a preview's approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum Remedy {
    /// The merge will be team-approved: nothing to do.
    None,
    /// The merge will count, self-signed. A second reviewer with write access
    /// who did not edit a governed file would make it team-approved.
    OptionalSecondReviewer,
    /// Request a review from a human with write access who did not edit a
    /// governed file: there is no approving review, only bots approved, no
    /// approver has write access, or the approver lost it.
    AddReviewerWithWriteAccess,
    /// Every approver with write access pushed a governed edit (or is the
    /// author). Redo the edit as a suggestion the author commits, then
    /// re-approve; or add a reviewer who did not edit.
    AskAuthorToCommitSuggestion,
    /// Every otherwise eligible approver reviewed a commit at which the file
    /// differed from what would merge (or whose blob cannot be fetched):
    /// re-approve the latest commit.
    ReApproveLatestCommit,
    /// A reviewer's account or a governed commit's author cannot be resolved:
    /// link the commit email to an account, or add a resolvable reviewer.
    ResolveIdentity,
    /// The content would arrive by direct push: open a pull request.
    OpenPullRequest,
}

/// The "if merged now" row: the approval the merge would get, and its remedy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub struct Preview {
    pub approval: Approval,
    pub remedy: Remedy,
}

/// The remedy table.
pub open spec fn spec_remedy(a: Approval) -> Remedy {
    match a {
        Approval::Approved { sig: Sig::Team, .. } => Remedy::None,
        Approval::Approved { sig: Sig::SelfSigned, .. } => Remedy::OptionalSecondReviewer,
        Approval::Unreviewed(UnreviewedReason::DirectPush) => Remedy::OpenPullRequest,
        Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked) => Remedy::AddReviewerWithWriteAccess,
        Approval::Unreviewed(UnreviewedReason::NoEligibleReview(why)) => match why {
            Ineligible::NoReview => Remedy::AddReviewerWithWriteAccess,
            Ineligible::BotOnly => Remedy::AddReviewerWithWriteAccess,
            Ineligible::NoWriteAccess => Remedy::AddReviewerWithWriteAccess,
            Ineligible::ApproverEditedGoverned => Remedy::AskAuthorToCommitSuggestion,
            Ineligible::ApprovalPredatesChange => Remedy::ReApproveLatestCommit,
            Ineligible::UnknownIdentity => Remedy::ResolveIdentity,
        },
    }
}

/// Computes `spec_remedy`.
pub fn remedy_of(a: Approval) -> (r: Remedy)
    ensures
        r == spec_remedy(a),
{
    match a {
        Approval::Approved { sig: Sig::Team, .. } => Remedy::None,
        Approval::Approved { sig: Sig::SelfSigned, .. } => Remedy::OptionalSecondReviewer,
        Approval::Unreviewed(UnreviewedReason::DirectPush) => Remedy::OpenPullRequest,
        Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked) => Remedy::AddReviewerWithWriteAccess,
        Approval::Unreviewed(UnreviewedReason::NoEligibleReview(why)) => match why {
            Ineligible::NoReview => Remedy::AddReviewerWithWriteAccess,
            Ineligible::BotOnly => Remedy::AddReviewerWithWriteAccess,
            Ineligible::NoWriteAccess => Remedy::AddReviewerWithWriteAccess,
            Ineligible::ApproverEditedGoverned => Remedy::AskAuthorToCommitSuggestion,
            Ineligible::ApprovalPredatesChange => Remedy::ReApproveLatestCommit,
            Ineligible::UnknownIdentity => Remedy::ResolveIdentity,
        },
    }
}

/// The approval a governed file would get if the PR merged now, and the one
/// remedy that would change it.
pub fn preview(intro_if_merged: &Introduction, allow_self_signed: bool) -> (p: Preview)
    ensures
        // The approval is `eligibility`'s, with its guarantees.
        eligibility_sound(*intro_if_merged, allow_self_signed, p.approval),
        eligibility_reasoned(*intro_if_merged, allow_self_signed, p.approval),
        // The remedy is the table's row for that approval.
        p.remedy == spec_remedy(p.approval),
        // Each remedy, and what it is consistent with.
        p.remedy is None <==> team_approved(p.approval),
        p.remedy is OptionalSecondReviewer <==> self_signed(p.approval),
        p.remedy is OpenPullRequest <==> p.approval == Approval::Unreviewed(UnreviewedReason::DirectPush),
        p.remedy is AddReviewerWithWriteAccess <==> (p.approval == Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked)
            || p.approval == Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::NoReview))
            || p.approval == Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::BotOnly))
            || p.approval == Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::NoWriteAccess))),
        p.remedy is AskAuthorToCommitSuggestion <==> p.approval == Approval::Unreviewed(
            UnreviewedReason::NoEligibleReview(Ineligible::ApproverEditedGoverned),
        ),
        p.remedy is ReApproveLatestCommit <==> p.approval == Approval::Unreviewed(
            UnreviewedReason::NoEligibleReview(Ineligible::ApprovalPredatesChange),
        ),
        p.remedy is ResolveIdentity <==> p.approval == Approval::Unreviewed(
            UnreviewedReason::NoEligibleReview(Ineligible::UnknownIdentity),
        ),
{
    let approval = eligibility(intro_if_merged, allow_self_signed);
    Preview { approval, remedy: remedy_of(approval) }
}

} // verus!
