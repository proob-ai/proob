//! Evidence acceptance and retrieval over `tests/fixtures/runs/`: each
//! scenario's first run through the verified `accept_run`, and the whole
//! scenario through `evidence::retrieve`.
mod common;

use common::*;
use proob_core::evidence_accept::accept_run;
use proob_core::types::{Kind, Measured, RunExpectation, ScoreOutcome};
use proob_effects::error::EvidenceError;
use proob_effects::evidence::{EvidenceQuery, retrieve};
use proob_effects::fixture::{FixtureApi, FixtureRepo};
use proob_effects::git::Repo;
use proob_effects::github::CallBudget;
use proob_facts::convert::run_facts;
use proob_facts::paths::EVIDENCE_WORKFLOW;

fn expectation(approval: proob_core::types::Approval) -> RunExpectation {
    RunExpectation {
        workflow_path: EVIDENCE_WORKFLOW.as_bytes().to_vec(),
        default_branch: b"main".to_vec(),
        workflow_blob: sha('8'),
        workflow_approval: approval,
    }
}

fn accepted(name: &str, approval: proob_core::types::Approval) -> bool {
    let scenario = run_scenario(name);
    let repo = FixtureRepo::new(&scenario);
    let run = scenario.workflow_runs.first().expect("a run");
    let blob = repo.blob_at(&run.head_sha, EVIDENCE_WORKFLOW).expect("blob lookup");
    let facts = run_facts(run, blob).expect("run facts");
    accept_run(&facts, &expectation(approval))
}

fn retrieved(
    name: &str,
    approval: proob_core::types::Approval,
) -> Result<proob_effects::evidence::EvidenceSet, EvidenceError> {
    let scenario = run_scenario(name);
    let budget = CallBudget::new(1_000);
    let api = FixtureApi::new(&scenario, &budget);
    let repo = FixtureRepo::new(&scenario);
    let expected = expectation(approval);
    let wanted = [(property_id(), Kind::Test)];
    let query =
        EvidenceQuery { expectation: &expected, now: NOW, freshness_secs: 14 * DAY, wanted: &wanted, max_runs: 50 };
    retrieve(&api, &repo, &query)
}

#[test]
fn acceptance_per_scenario() {
    let table = [
        ("push-on-default", true),
        ("schedule-on-default", true),
        ("feature-branch", false),
        ("other-workflow-path", false),
        ("failed-conclusion", false),
        ("unapproved-workflow-blob", false),
        ("matching-record-two-runs-back", true),
    ];
    for (name, want) in table {
        assert_eq!(accepted(name, team(1, '0')), want, "{name}");
    }
    // An approved blob from a workflow whose approval is gone accepts nothing.
    assert!(!accepted("push-on-default", no_eligible(proob_core::types::Ineligible::NoReview)));
    // A self-signed workflow approval still accepts; the verdict adds the modifier.
    assert!(accepted("push-on-default", self_signed(1, '0')));
}

#[test]
fn push_run_yields_its_record_and_head() {
    let set = retrieved("push-on-default", team(1, '0')).expect("retrieve");
    assert_eq!(set.runs_accepted, 1);
    assert_eq!(set.latest_run_head, Some(sha('f')));
    let records = set.records_for(property_id());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].measured, Measured::Primary(Kind::Test, proob_core::types::PrimaryOutcome::NoCounterexample));
    assert_eq!(records[0].run_id, 100);
    assert_eq!(records[0].workflow_blob, sha('8'));
}

#[test]
fn scheduled_run_yields_check_and_score() {
    let set = retrieved("schedule-on-default", team(1, '0')).expect("retrieve");
    let records = set.records_for(property_id());
    assert_eq!(records.len(), 2);
    assert!(records.iter().any(|r| matches!(r.measured, Measured::Score(ScoreOutcome::Scored { killed: 3, .. }))));
}

// proob:01M622RQ7C066YFA9NDMB7EG6K
#[test]
fn rejected_runs_yield_nothing() {
    for name in ["feature-branch", "other-workflow-path", "failed-conclusion", "unapproved-workflow-blob"] {
        let set = retrieved(name, team(1, '0')).expect("retrieve");
        assert!(set.records.is_empty(), "{name}");
        assert_eq!(set.latest_run_head, None, "{name}");
        assert_eq!(set.runs_accepted, 0, "{name}");
    }
}

#[test]
fn walks_back_to_a_matching_record_and_tolerates_unknown_kinds() {
    let set = retrieved("matching-record-two-runs-back", team(1, '0')).expect("retrieve");
    assert_eq!(set.runs_accepted, 3);
    // The newest accepted run's head, even though it carried no record for us.
    assert_eq!(set.latest_run_head, Some(sha2("2f")));
    let records = set.records_for(property_id());
    assert_eq!(records.len(), 2, "check and score from run 101");
    assert!(records.iter().all(|r| r.run_id == 101));
    // Records for other properties are not collected.
    assert_eq!(set.records.len(), 2);
}

#[test]
fn malformed_line_is_refused_by_number() {
    let err = retrieved("malformed-line", team(1, '0')).unwrap_err();
    match err {
        EvidenceError::MalformedLine { run_id, artifact, line, .. } => {
            assert_eq!((run_id, artifact, line), (104, 9004, 2));
        }
        other => panic!("expected a malformed line, got {other:?}"),
    }
}

#[test]
fn runs_older_than_the_window_are_skipped() {
    let scenario = run_scenario("push-on-default");
    let budget = CallBudget::new(1_000);
    let api = FixtureApi::new(&scenario, &budget);
    let repo = FixtureRepo::new(&scenario);
    let expected = expectation(team(1, '0'));
    let wanted = [(property_id(), Kind::Test)];
    let query = EvidenceQuery {
        expectation: &expected,
        now: NOW + 30 * DAY,
        freshness_secs: 14 * DAY,
        wanted: &wanted,
        max_runs: 50,
    };
    let set = retrieve(&api, &repo, &query).expect("retrieve");
    assert!(set.records.is_empty());
    assert!(set.notes.iter().any(|n| n.contains("freshness")), "{:?}", set.notes);
}
