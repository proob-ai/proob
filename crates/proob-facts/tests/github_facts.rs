//! The facts crate's U9 additions: GitHub conversions, object-id text,
//! timestamps, record lines, and governed paths.
use proob_core::types::{
    ReviewState as CoreReviewState, RunConclusion as CoreConclusion, RunEvent as CoreEvent, UserKind,
};
use proob_facts::convert::{has_write, review_state, run_conclusion, run_event, user_kind};
use proob_facts::date::parse_timestamp;
use proob_facts::evidence::{Line, parse_line, parse_lines};
use proob_facts::github::{AccountType, Permission, PermissionLevel, ReviewState, RunConclusion, RunEvent, User};
use proob_facts::hex::{decode_oid, encode_oid};
use proob_facts::paths::{is_fixed_governed, is_property_file};

fn user(login: &str, kind: AccountType) -> User {
    User { login: login.into(), id: 1, kind }
}

#[test]
fn account_kinds() {
    assert_eq!(user_kind(Some(&user("alice", AccountType::User))), UserKind::Human);
    assert_eq!(user_kind(Some(&user("dependabot[bot]", AccountType::User))), UserKind::Bot);
    assert_eq!(user_kind(Some(&user("github-actions[bot]", AccountType::Bot))), UserKind::Bot);
    assert_eq!(user_kind(Some(&user("org", AccountType::Organization))), UserKind::Unknown);
    assert_eq!(user_kind(Some(&user("x", AccountType::Unknown))), UserKind::Unknown);
    assert_eq!(user_kind(None), UserKind::Unknown);
}

#[test]
fn write_access_levels() {
    let p =
        |level, role: Option<&str>| Permission { permission: level, role_name: role.map(str::to_string), user: None };
    assert!(has_write(&p(PermissionLevel::Admin, Some("admin"))));
    assert!(has_write(&p(PermissionLevel::Write, Some("write"))));
    assert!(has_write(&p(PermissionLevel::Write, Some("maintain"))));
    assert!(!has_write(&p(PermissionLevel::Read, Some("read"))));
    assert!(!has_write(&p(PermissionLevel::Read, Some("triage"))));
    assert!(!has_write(&p(PermissionLevel::None, None)));
    assert!(!has_write(&p(PermissionLevel::Unknown, Some("custom"))));
}

#[test]
fn review_states_and_run_facts() {
    assert_eq!(review_state(ReviewState::Approved), Some(CoreReviewState::Approved));
    assert_eq!(review_state(ReviewState::Dismissed), Some(CoreReviewState::Dismissed));
    assert_eq!(review_state(ReviewState::Unknown), None);
    assert_eq!(run_event(RunEvent::Push), CoreEvent::Push);
    assert_eq!(run_event(RunEvent::PullRequestTarget), CoreEvent::PullRequest);
    assert_eq!(run_event(RunEvent::Other), CoreEvent::Other);
    assert_eq!(run_conclusion(Some(RunConclusion::Success)), CoreConclusion::Success);
    assert_eq!(run_conclusion(Some(RunConclusion::Failure)), CoreConclusion::NotSuccess);
    assert_eq!(run_conclusion(None), CoreConclusion::NotSuccess);
}

#[test]
fn object_ids_round_trip_to_git_text() {
    let sha1 = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(encode_oid(&decode_oid(sha1).expect("sha1")), sha1);
    let sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    assert_eq!(encode_oid(&decode_oid(sha256).expect("sha256")), sha256);
}

#[test]
fn timestamps() {
    assert_eq!(parse_timestamp("2026-10-28T00:00:00Z"), Ok(1_793_145_600));
    assert_eq!(parse_timestamp("2026-10-28T00:00:00.123Z"), Ok(1_793_145_600));
    assert_eq!(parse_timestamp("2026-10-28T02:00:00+02:00"), Ok(1_793_145_600));
    assert_eq!(parse_timestamp("2026-10-27T22:30:00-01:30"), Ok(1_793_145_600));
    assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Ok(0));
    assert!(parse_timestamp("2026-10-28").is_err());
    assert!(parse_timestamp("2026-13-01T00:00:00Z").is_err());
    assert!(parse_timestamp("2026-10-28T25:00:00Z").is_err());
}

#[test]
fn record_lines() {
    let good = r#"{"schema_version":1,"id":"01J9Z3W1V8Q4K7N2M5R8T1X4C6","kind":"check","algo":"sha256","content_hash":"00","binding_hash":"00","scope_hash":"00","outcome":{"result":"no_counterexample","budget":1},"run_id":1,"head_sha":"00","workflow_blob":"00","at":1}"#;
    assert!(matches!(parse_line(good), Ok(Line::Record(_))));
    assert_eq!(parse_line(""), Ok(Line::Blank));
    assert_eq!(parse_line(r#"{"kind":"fuzz","anything":true}"#), Ok(Line::UnknownKind("fuzz".into())));
    assert!(parse_line(r#"{"kind":"check","schema_version":"one"}"#).is_err());
    assert!(parse_line("{not json").is_err());
    let numbered: Vec<usize> = parse_lines("\n{\"kind\":\"fuzz\"}\n\n").map(|(n, _)| n).collect();
    assert_eq!(numbered, vec![1, 2, 3]);
}

#[test]
fn governed_paths() {
    assert!(is_property_file("proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4C6.toml"));
    assert!(!is_property_file("proob/properties/nested/x.toml"));
    assert!(!is_property_file("proob/properties/x.md"));
    assert!(is_fixed_governed("proob/config.toml"));
    assert!(is_fixed_governed(".github/workflows/proob-evidence.yml"));
    assert!(is_fixed_governed(".github/workflows/proob-gate.yml"));
    assert!(!is_fixed_governed("src/lib.rs"));
}
