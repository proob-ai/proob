//! One scenario per rule of the eligibility derivation (`tests/fixtures/reviews/`),
//! each run through the real assembly (`facts::introduction`, `gather::approval_of`)
//! over the fixture fakes and judged by the verified `eligibility` and `preview`.
//! The expected answers live here, not in the fixtures: a fixture holds facts.
mod common;

use common::*;
use proob_core::preview::{Remedy, preview};
use proob_core::types::{Approval, Ineligible::*};
use proob_effects::facts::{GovernedSet, IntroducingCommit, introduction, introduction_if_merged};
use proob_effects::fixture::{FixtureApi, FixtureRepo};
use proob_effects::gather::approval_of;
use proob_effects::git::Repo;
use proob_effects::github::CallBudget;
use proob_facts::hex;

struct Case {
    name: &'static str,
    allow_self_signed: bool,
    approval: Approval,
    remedy: Remedy,
    previously_approved: bool,
}

const fn case(name: &'static str, allow_self_signed: bool, approval: Approval, remedy: Remedy) -> Case {
    Case { name, allow_self_signed, approval, remedy, previously_approved: false }
}

fn cases() -> Vec<Case> {
    vec![
        case("non-author-approval-with-write", false, team(7, 'a'), Remedy::None),
        case("author-only", false, no_eligible(NoReview), Remedy::AddReviewerWithWriteAccess),
        case("author-only", true, self_signed(7, 'a'), Remedy::OptionalSecondReviewer),
        case("approve-then-changes-requested", false, no_eligible(NoReview), Remedy::AddReviewerWithWriteAccess),
        case("dismissed", false, no_eligible(NoReview), Remedy::AddReviewerWithWriteAccess),
        case("earlier-commit-blob-unchanged", false, team(7, 'a'), Remedy::None),
        case("earlier-commit-blob-changed", false, no_eligible(ApprovalPredatesChange), Remedy::ReApproveLatestCommit),
        case(
            "reviewed-commit-blob-unavailable",
            false,
            no_eligible(ApprovalPredatesChange),
            Remedy::ReApproveLatestCommit,
        ),
        case("bot-approver", false, no_eligible(BotOnly), Remedy::AddReviewerWithWriteAccess),
        case("approver-without-write", false, no_eligible(NoWriteAccess), Remedy::AddReviewerWithWriteAccess),
        case(
            "reviewer-pushed-governed-edit",
            false,
            no_eligible(ApproverEditedGoverned),
            Remedy::AskAuthorToCommitSuggestion,
        ),
        case("reviewer-only-deleted-proposal", false, team(7, 'a'), Remedy::None),
        case("reviewer-update-branch", false, team(7, 'a'), Remedy::None),
        case("author-committed-suggestion", false, team(7, 'a'), Remedy::None),
        case("unresolvable-author", false, no_eligible(UnknownIdentity), Remedy::ResolveIdentity),
        case("non-default-base", false, direct_push(), Remedy::OpenPullRequest),
        case("merge-commit-merge", false, team(7, 'a'), Remedy::None),
        case("squash-merge", false, team(7, 'b'), Remedy::None),
        // The merge sha is the introducing (rebased) commit, not the PR's last rebased commit.
        case("rebase-merge", false, team(7, 'c'), Remedy::None),
        case("no-reviews", false, no_eligible(NoReview), Remedy::AddReviewerWithWriteAccess),
        case("no-reviews", true, self_signed(7, 'a'), Remedy::OptionalSecondReviewer),
        case("direct-push", false, direct_push(), Remedy::OpenPullRequest),
        case("direct-push", true, direct_push(), Remedy::OpenPullRequest),
        Case {
            previously_approved: true,
            ..case("reintroduced-by-direct-push", false, direct_push(), Remedy::OpenPullRequest)
        },
        case("approver-permission-removed", false, revoked(), Remedy::AddReviewerWithWriteAccess),
        // Revocation ranks above a self-signed downgrade.
        case("approver-permission-removed", true, revoked(), Remedy::AddReviewerWithWriteAccess),
    ]
}

#[test]
fn every_review_scenario_derives_its_rule() {
    for c in cases() {
        let scenario = review_scenario(c.name);
        let budget = CallBudget::new(1_000);
        let api = FixtureApi::new(&scenario, &budget);
        let repo = FixtureRepo::new(&scenario);
        let governed = GovernedSet::default();
        let ctx = ctx(&api, &repo, &governed, c.allow_self_signed);

        let got = approval_of(&ctx, &[PROPERTY_PATH]).unwrap_or_else(|e| panic!("{}: {e}", c.name));
        assert_eq!(got.approval, c.approval, "{} (allow_self_signed = {})", c.name, c.allow_self_signed);
        assert_eq!(got.previously_approved, c.previously_approved, "{}: previously_approved", c.name);

        // The same facts through the preview give the same approval and the table's remedy.
        let intro_sha = repo.introducing_commit("origin/main", &[PROPERTY_PATH]).expect("history").expect("introduced");
        let blob = repo.blob_at(&hex::encode_oid(&intro_sha), PROPERTY_PATH).expect("blob").expect("present");
        let intro = introduction(
            &api,
            &governed,
            "main",
            &[PROPERTY_PATH],
            &IntroducingCommit { sha: intro_sha, blob_at_merge: blob },
        )
        .unwrap_or_else(|e| panic!("{}: {e}", c.name));
        let p = preview(&intro, c.allow_self_signed);
        assert_eq!(p.approval, c.approval, "{}: preview approval", c.name);
        assert_eq!(p.remedy, c.remedy, "{}: remedy", c.name);
    }
}

#[test]
fn every_review_fixture_is_covered_by_a_case() {
    let covered: std::collections::BTreeSet<&str> = cases().iter().map(|c| c.name).collect();
    for name in scenario_names("reviews") {
        let is_open_pr = name.starts_with("open-pr-");
        assert!(is_open_pr || covered.contains(name.as_str()), "fixture {name} has no expected outcome");
    }
}

#[test]
fn open_pr_preview_names_the_remedy_before_merge() {
    let expectations = [
        ("open-pr-approval-predates-head", no_eligible(ApprovalPredatesChange), Remedy::ReApproveLatestCommit),
        ("open-pr-ready", team(9, '1'), Remedy::None),
    ];
    for (name, approval, remedy) in expectations {
        let scenario = review_scenario(name);
        let budget = CallBudget::new(1_000);
        let api = FixtureApi::new(&scenario, &budget);
        let governed = GovernedSet::default();
        let pr = scenario.pulls.get("9").expect("open PR #9");
        let intro = introduction_if_merged(&api, &governed, "main", &[PROPERTY_PATH], pr)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let p = preview(&intro, false);
        assert_eq!(p.approval, approval, "{name}");
        assert_eq!(p.remedy, remedy, "{name}");
    }
}

#[test]
fn a_pr_into_another_branch_has_no_preview() {
    let mut scenario = review_scenario("open-pr-ready");
    let pr = scenario.pulls.get_mut("9").expect("open PR #9");
    pr.base.name = "develop".to_string();
    let budget = CallBudget::new(10);
    let api = FixtureApi::new(&scenario, &budget);
    let err = introduction_if_merged(
        &api,
        &GovernedSet::default(),
        "main",
        &[PROPERTY_PATH],
        scenario.pulls.get("9").expect("pr"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("develop"), "{err}");
}
