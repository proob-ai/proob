//! `gather` and `governance_facts` end to end over a composite scenario: the
//! facts reach the verified `derive` and `verdict`, and what they say matches
//! the scenario.
mod common;

use common::*;
use proob_core::derive::derive;
use proob_core::types::*;
use proob_core::verdict::verdict;
use proob_effects::evidence::EvidenceSet;
use proob_effects::facts::GovernedSet;
use proob_effects::fixture::{FixtureApi, FixtureRepo, Scenario};
use proob_effects::gather::{
    BindingSet, PropertyRequest, evidence_expectation, gather, governance_facts, issue_number,
};
use proob_effects::github::CallBudget;
use proob_facts::config::Config;
use proob_facts::convert;
use proob_facts::github::{Branch, BranchRule, Issue, IssueState, RequiredCheck, RuleParameters};
use proob_facts::paths::{CONFIG, EVIDENCE_WORKFLOW, GATE_WORKFLOW};
use proob_facts::property::parse_property;

const PROPERTY_TOML: &str = r#"
id = "01J9Z3W1V8Q4K7N2M5R8T1X4C6"
title = "Lib never panics"
statement = "No public function of the library panics on any input."
scope = ["src/**/*.rs"]
severity = "high"
rationale = "A panic in the library takes the caller down."
kind = "test"
"#;

/// The approved-PR scenario, with a working tree, governance files, and the
/// evidence workflow's blob, all introduced by the same approved PR.
fn composite() -> Scenario {
    let mut s = review_scenario("non-author-approval-with-write");
    let merge = "a".repeat(40);
    let c1 = "1".repeat(40);
    s.files.insert(PROPERTY_PATH.to_string(), PROPERTY_TOML.to_string());
    s.files.insert("src/lib.rs".to_string(), "pub fn f() {}\n".to_string());
    s.files.insert("src/util/mod.rs".to_string(), "pub fn g() {}\n".to_string());
    s.files.insert("tests/lib_test.rs".to_string(), "#[test] fn t() {}\n".to_string());
    s.files.insert("README.md".to_string(), "# r\n".to_string());
    for (i, path) in [CONFIG, EVIDENCE_WORKFLOW, GATE_WORKFLOW, "tests/lib_test.rs"].iter().enumerate() {
        let blob = format!("{:x}", i + 10).repeat(40);
        s.history.insert(path.to_string(), vec![merge.clone()]);
        s.blobs.insert(format!("{merge}:{path}"), Some(blob.clone()));
        s.blobs.insert(format!("{c1}:{path}"), Some(blob));
    }
    // A drift commit on main touching the scope after the merge.
    let d = "d".repeat(40);
    s.history.insert("src/lib.rs".to_string(), vec![d.clone(), c1.clone()]);
    s.commit_times.insert(d, 1_760_400_000 + 3 * DAY);
    s.branch = Some(Branch { name: "main".into(), protected: true });
    s.branch_rules = vec![
        BranchRule { kind: "pull_request".into(), parameters: None },
        BranchRule {
            kind: "required_status_checks".into(),
            parameters: Some(RuleParameters {
                required_status_checks: vec![RequiredCheck { context: "proob gate".into() }],
            }),
        },
    ];
    s.blobs.insert("main:.github/CODEOWNERS".into(), Some("e".repeat(40)));
    s.blobs.insert("main:CODEOWNERS".into(), None);
    s.blobs.insert("main:docs/CODEOWNERS".into(), None);
    s.issues.insert("12".into(), Issue { number: 12, state: IssueState::Open });
    s
}

fn request<'a>(file: &'a proob_facts::property::PropertyFile, binding: Option<&'a BindingSet>) -> PropertyRequest<'a> {
    PropertyRequest {
        path: PROPERTY_PATH,
        file,
        kind_default: Kind::Test,
        adapter_available: true,
        binding,
        provisional: None,
        waiver_max_days: 90,
    }
}

#[test]
fn gather_feeds_derive_and_verdict() {
    let scenario = composite();
    let budget = CallBudget::new(1_000);
    let api = FixtureApi::new(&scenario, &budget);
    let repo = FixtureRepo::new(&scenario);
    let governed = GovernedSet::new(["tests/lib_test.rs".to_string()]);
    let ctx = ctx(&api, &repo, &governed, true);
    let file = parse_property(PROPERTY_TOML).expect("property");
    let binding = BindingSet { files: vec!["tests/lib_test.rs".to_string()] };

    let input = gather(&ctx, &request(&file, Some(&binding)), &EvidenceSet::default(), sha('a')).expect("gather");

    assert_eq!(input.statement, team(7, 'a'));
    assert!(!input.previously_approved);
    let mut scope: Vec<String> = input.scope.iter().map(|p| String::from_utf8_lossy(&p.path).into_owned()).collect();
    scope.sort();
    assert_eq!(
        scope,
        vec!["src/lib.rs".to_string(), "src/util/mod.rs".to_string()],
        "globs resolve against tracked files only"
    );
    assert_eq!(input.drift, Drift { commits: 1, days: 3 });
    assert!(!input.waiver_issue_open);
    let binding_facts = input.binding.expect("bound");
    assert_eq!(binding_facts.approval, team(7, 'a'));
    assert_eq!(input.kind, Kind::Test);
    assert_eq!(input.default_head, sha('a'));
    assert_eq!(input.evidence_run_head, None);

    let cfg = convert::gate_config(&Config::defaults()).expect("config");
    let status = derive(input, &cfg, NOW);
    let g = governance_facts(&ctx, &Config::defaults(), "proob gate").expect("governance");
    assert_eq!(g.config, team(7, 'a'));
    assert_eq!(g.evidence_workflow, team(7, 'a'));
    assert_eq!(g.gate_workflow, team(7, 'a'));
    assert_eq!((g.branch_requires_review, g.gate_required, g.codeowners), (Known::Yes, Known::Yes, Known::Yes));
    assert_eq!(g.floor, 1);

    // Approved statement and check, no evidence yet: not checked, evidence pending.
    assert_eq!(
        verdict(&status, &g),
        Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: false }
    );

    let expectation = evidence_expectation(&ctx, &g).expect("expectation");
    assert_eq!(expectation.workflow_blob, sha('b'), "the evidence workflow blob at the default head");
    assert_eq!(expectation.workflow_approval, team(7, 'a'));
}

#[test]
fn a_local_proposal_is_unreviewed_with_no_review() {
    let mut scenario = composite();
    scenario.history.remove(PROPERTY_PATH);
    let budget = CallBudget::new(1_000);
    let api = FixtureApi::new(&scenario, &budget);
    let repo = FixtureRepo::new(&scenario);
    let governed = GovernedSet::default();
    let ctx = ctx(&api, &repo, &governed, true);
    let file = parse_property(PROPERTY_TOML).expect("property");
    let input = gather(&ctx, &request(&file, None), &EvidenceSet::default(), sha('a')).expect("gather");
    assert_eq!(input.statement, no_eligible(Ineligible::NoReview));
    assert_eq!(input.drift, Drift { commits: 0, days: 0 });
    let cfg = convert::gate_config(&Config::defaults()).expect("config");
    let status = derive(input, &cfg, NOW);
    let g = governance_facts(&ctx, &Config::defaults(), "proob gate").expect("governance");
    assert_eq!(verdict(&status, &g), Verdict::Proposed);
}

#[test]
fn unreadable_branch_facts_are_unknown_not_yes() {
    let governance_of = |scenario: &Scenario| {
        let budget = CallBudget::new(1_000);
        let api = FixtureApi::new(scenario, &budget);
        let repo = FixtureRepo::new(scenario);
        let governed = GovernedSet::default();
        let ctx = ctx(&api, &repo, &governed, true);
        governance_facts(&ctx, &Config::defaults(), "proob gate").expect("governance")
    };
    let mut scenario = composite();
    scenario.branch_rules.clear();
    let g = governance_of(&scenario);
    assert_eq!(
        (g.branch_requires_review, g.gate_required),
        (Known::Unknown, Known::Unknown),
        "protected, rules unreadable"
    );

    scenario.branch = Some(Branch { name: "main".into(), protected: false });
    let g = governance_of(&scenario);
    assert_eq!((g.branch_requires_review, g.gate_required), (Known::No, Known::No));
}

#[test]
fn waiver_issue_state_is_read_from_the_issues_api() {
    let scenario = composite();
    let budget = CallBudget::new(1_000);
    let api = FixtureApi::new(&scenario, &budget);
    let repo = FixtureRepo::new(&scenario);
    let governed = GovernedSet::default();
    let ctx = ctx(&api, &repo, &governed, true);
    // The window is measured from the approving merge (2025-10-14 in the fixture), so it must end within 90 days of it.
    let waived = format!("{PROPERTY_TOML}\n[waiver]\nuntil = 2025-12-01\nissue = \"#12\"\nreason = \"known\"\n");
    let file = parse_property(&waived).expect("property");
    let input = gather(&ctx, &request(&file, None), &EvidenceSet::default(), sha('a')).expect("gather");
    assert!(input.waiver_issue_open);

    let closed = waived.replace("#12", "https://github.com/o/r/issues/13");
    let file = parse_property(&closed).expect("property");
    let err = gather(&ctx, &request(&file, None), &EvidenceSet::default(), sha('a')).unwrap_err();
    assert!(
        err.to_string().contains("issue 13") || err.to_string().contains("issues/13") || err.to_string().contains("13"),
        "{err}"
    );

    assert_eq!(issue_number("#12"), Some(12));
    assert_eq!(issue_number("https://github.com/o/r/issues/345"), Some(345));
    assert_eq!(issue_number("12"), Some(12));
    assert_eq!(issue_number("twelve"), None);
}

#[test]
fn duplicate_resolved_paths_are_an_error_not_a_panic() {
    let scenario = composite();
    let budget = CallBudget::new(1_000);
    let api = FixtureApi::new(&scenario, &budget);
    let repo = FixtureRepo::new(&scenario);
    let governed = GovernedSet::default();
    let ctx = ctx(&api, &repo, &governed, true);
    let file = parse_property(PROPERTY_TOML).expect("property");
    let binding = BindingSet { files: vec!["tests/lib_test.rs".to_string(), "tests/lib_test.rs".to_string()] };
    let err = gather(&ctx, &request(&file, Some(&binding)), &EvidenceSet::default(), sha('a')).unwrap_err();
    assert!(matches!(err, proob_effects::error::GatherError::DuplicatePath { .. }), "{err:?}");
}
