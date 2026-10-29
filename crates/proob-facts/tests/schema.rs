//! The U2 schema scenarios over the fixtures in `tests/fixtures/properties`.
use proob_core::types::{GateConfig, Kind, Measured, Outcome, Policy, PrimaryOutcome, Property, Severity, Waiver};
use proob_facts::config::{Adapter, Config, Language, OutcomeName, PolicyTable, Witness, parse_config};
use proob_facts::convert::{self, GlobRule};
use proob_facts::date::Date;
use proob_facts::error::{ConvertError, ConvertErrorKind, ParseError};
use proob_facts::evidence::{EvidenceKind, parse_evidence};
use proob_facts::property::{PropertyFile, content_hash_of, normalise, parse_property};
use proob_facts::ulid;
use std::fs;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/properties");
/// 2026-10-28T00:00:00Z: the instant waiver windows are measured from.
const AT: u64 = 1793145600;
const MAX_DAYS: u32 = 90;

fn fixture(name: &str) -> String {
    fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn parsed(name: &str) -> PropertyFile {
    parse_property(&fixture(name)).unwrap_or_else(|e| panic!("{name} should parse: {e}"))
}

fn converted(name: &str) -> Result<Property, ConvertError> {
    convert::property(&parsed(name), AT, MAX_DAYS)
}

fn convert_error(name: &str) -> ConvertError {
    converted(name).expect_err(&format!("{name} should not convert"))
}

#[test]
fn valid_fixture_parses_and_converts() {
    let file = parsed("valid.toml");
    let p = converted("valid.toml").expect("converts");
    assert_eq!(ulid::encode(p.id), file.id);
    assert_eq!(p.kind, Some(Kind::Proof));
    assert_eq!(p.severity, Severity::High);
    assert_eq!(p.scope, file.scope.iter().map(|g| g.as_bytes().to_vec()).collect::<Vec<_>>());
    assert_eq!(p.reviewed, Some(AT));
    assert_eq!(p.waiver, None);
    assert_eq!(Date::new(2026, 10, 28).expect("date").to_secs(), AT);
}

#[test]
fn status_key_is_rejected_naming_the_key() {
    let err = parse_property(&fixture("status-key.toml")).expect_err("status key");
    assert_eq!(err, ParseError::StatusKey);
    let message = err.to_string();
    assert!(message.contains("`status`"), "{message}");
    assert!(message.contains("never stores status"), "{message}");
}

#[test]
fn status_key_is_named_before_any_other_schema_error() {
    let text = format!("{}owner = \"x\"\n", fixture("status-key.toml"));
    assert_eq!(parse_property(&text).expect_err("rejects"), ParseError::StatusKey);
}

#[test]
fn unknown_key_is_rejected_by_the_schema() {
    match parse_property(&fixture("unknown-key.toml")).expect_err("unknown key") {
        ParseError::Schema(m) => assert!(m.contains("unknown field `owner`"), "{m}"),
        other => panic!("expected a schema error, got {other:?}"),
    }
}

#[test]
fn empty_scope_is_rejected() {
    let err = convert_error("empty-scope.toml");
    assert_eq!(err, ConvertError::at("scope", ConvertErrorKind::EmptyScope));
    assert!(err.to_string().contains("scope is empty"), "{err}");
}

#[test]
fn parent_segment_glob_is_rejected_naming_the_rule() {
    let err = convert_error("bad-glob-dotdot.toml");
    assert_eq!(err, ConvertError::at("scope[0]", GlobRule::ParentSegment { at: 0 }));
    assert!(err.to_string().contains("`..` segment"), "{err}");
}

#[test]
fn leading_dash_glob_is_rejected_naming_the_rule() {
    let err = convert_error("bad-glob-dash.toml");
    assert_eq!(err, ConvertError::at("scope[0]", GlobRule::LeadingDash));
    assert!(err.to_string().contains("leading `-`"), "{err}");
}

#[test]
fn disallowed_byte_is_rejected_naming_the_rule() {
    let err = convert_error("bad-glob-byte.toml");
    assert_eq!(err, ConvertError::at("scope[0]", GlobRule::Byte { at: 8, byte: b' ' }));
    assert!(err.to_string().contains("outside the glob alphabet"), "{err}");
}

#[test]
fn the_offending_glob_is_named_by_index() {
    let mut file = parsed("valid.toml");
    file.scope = vec!["src/a.rs".into(), "/etc/passwd".into()];
    let err = convert::property(&file, AT, MAX_DAYS).expect_err("rejects");
    assert_eq!(err, ConvertError::at("scope[1]", GlobRule::LeadingSlash));
}

#[test]
fn glob_rules_by_example() {
    assert_eq!(convert::glob(b""), Err(GlobRule::Empty));
    assert_eq!(convert::glob(b"-x"), Err(GlobRule::LeadingDash));
    assert_eq!(convert::glob(b"/x"), Err(GlobRule::LeadingSlash));
    assert_eq!(convert::glob(b".."), Err(GlobRule::ParentSegment { at: 0 }));
    assert_eq!(convert::glob(b"a/.."), Err(GlobRule::ParentSegment { at: 2 }));
    assert_eq!(convert::glob(b"a/../b"), Err(GlobRule::ParentSegment { at: 2 }));
    assert_eq!(convert::glob(b"a;b"), Err(GlobRule::Byte { at: 1, byte: b';' }));
    assert_eq!(convert::glob("src/ü.rs".as_bytes()), Err(GlobRule::Byte { at: 4, byte: 0xc3 }));
    // `..` inside a segment is not a parent segment.
    assert_eq!(convert::glob(b"a..b"), Ok(()));
    assert_eq!(convert::glob(b"..a"), Ok(()));
    assert_eq!(convert::glob(b"a/..b"), Ok(()));
    assert_eq!(convert::glob(b"src/**/*.rs"), Ok(()));
    assert_eq!(convert::glob(b"a-b/{x,y}/[0-9]?.rs"), Ok(()));
    assert_eq!(convert::glob(b"a~b@c+d"), Ok(()));
}

#[test]
fn trailing_whitespace_does_not_change_the_content_hash() {
    let text = fixture("valid.toml");
    let noisy = text.lines().map(|l| format!("{l}  \t")).collect::<Vec<_>>().join("\r\n") + "\r\n\r\n";
    assert_ne!(text, noisy);
    assert_eq!(normalise(&text), normalise(&noisy));
    assert_eq!(content_hash_of(&text), content_hash_of(&noisy));
    assert_eq!(parse_property(&noisy).expect("still parses"), parsed("valid.toml"));
}

#[test]
fn adding_a_waiver_changes_the_content_hash() {
    assert_ne!(content_hash_of(&fixture("valid.toml")), content_hash_of(&fixture("waived.toml")));
}

#[test]
fn normalise_by_example() {
    assert_eq!(normalise(""), "");
    assert_eq!(normalise("\n\n"), "");
    assert_eq!(normalise("a"), "a\n");
    assert_eq!(normalise("a  \r\nb\t\n\n\n"), "a\nb\n");
    assert_eq!(normalise("a\n\nb\n"), "a\n\nb\n");
}

#[test]
fn content_hash_is_the_core_hash_of_the_normal_form() {
    let text = fixture("valid.toml");
    assert_eq!(content_hash_of(&text), proob_core::content_hash(normalise(&text).as_bytes()));
}

#[test]
fn waiver_inside_the_window_converts() {
    let p = converted("waived.toml").expect("converts");
    assert_eq!(p.waiver, Some(Waiver { until: Date::new(2026, 12, 31).expect("date").to_secs() }));
}

#[test]
fn waiver_longer_than_the_maximum_is_rejected() {
    let err = convert_error("waiver-too-long.toml");
    assert_eq!(err.field, "waiver.until");
    assert!(matches!(err.kind, ConvertErrorKind::WaiverTooLong { max_days: 90, days } if days > 90), "{err:?}");
    assert!(err.to_string().contains("waiver_max_days"), "{err}");
    // The same file converts when measured from inside the maximum.
    let near = Date::new(2027, 10, 15).expect("date").to_secs();
    assert!(convert::property(&parsed("waiver-too-long.toml"), near, MAX_DAYS).is_ok());
    // A waiver already in the past is not an error here; it renders as expired.
    let late = Date::new(2028, 1, 1).expect("date").to_secs();
    assert!(convert::property(&parsed("waiver-too-long.toml"), late, MAX_DAYS).is_ok());
}

#[test]
fn kill_floor_zero_is_rejected_naming_the_field() {
    let err = parse_config(&fixture("config-floor-zero.toml")).expect_err("floor zero");
    assert!(matches!(err, ParseError::Invalid { field: "kill_floor", .. }), "{err:?}");
    assert!(err.to_string().contains("kill_floor"), "{err}");
    // A hand-built config with floor zero has no core meaning either.
    let zero = Config { kill_floor: 0, ..Config::defaults() };
    assert_eq!(convert::gate_config(&zero), Err(ConvertError::at("kill_floor", ConvertErrorKind::FloorZero)));
}

#[test]
fn config_missing_a_policy_row_is_rejected_naming_the_row() {
    match parse_config(&fixture("config-missing-row.toml")).expect_err("missing row") {
        ParseError::Schema(m) => assert!(m.contains("missing field `broken_unaffected`"), "{m}"),
        other => panic!("expected a schema error, got {other:?}"),
    }
}

#[test]
fn config_blocking_a_row_other_than_broken_affected_is_rejected_naming_the_row() {
    let text = fixture("config.toml").replace("re_approve = \"warn\"", "re_approve = \"block\"");
    assert_ne!(text, fixture("config.toml"), "the fixture should carry re_approve = \"warn\"");
    let err = parse_config(&text).expect_err("blocking re_approve");
    assert!(matches!(err, ParseError::Invalid { field: "policy.re_approve", .. }), "{err:?}");
    assert!(err.to_string().contains("only broken_affected may block"), "{err}");
    // The one row that may block, `broken_affected`, does so in the fixture and parses.
    assert!(parse_config(&fixture("config.toml")).is_ok());
}

#[test]
fn uncovered_ignore_takes_scope_globs_only() {
    let ok = format!("uncovered_ignore = [\"src/generated/**\", \"vendor/*.py\"]\n{}", fixture("config.toml"));
    assert_eq!(parse_config(&ok).expect("parses").uncovered_ignore, vec!["src/generated/**", "vendor/*.py"]);
    assert!(parse_config(&fixture("config.toml")).expect("parses").uncovered_ignore.is_empty(), "absent means none");
    let bad = format!("uncovered_ignore = [\"src/**\", \"../outside/**\"]\n{}", fixture("config.toml"));
    match parse_config(&bad).expect_err("a `..` glob") {
        ParseError::Schema(m) => assert!(m.starts_with("uncovered_ignore[1]: not a scope glob: `..` segment"), "{m}"),
        other => panic!("expected a schema error, got {other:?}"),
    }
}

#[test]
fn config_unknown_key_is_rejected() {
    let text = format!("retries = 3\n{}", fixture("config.toml"));
    match parse_config(&text).expect_err("unknown key") {
        ParseError::Schema(m) => assert!(m.contains("unknown field `retries`"), "{m}"),
        other => panic!("expected a schema error, got {other:?}"),
    }
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let text = fixture("config.toml").replace("schema_version = 1", "schema_version = 2");
    let err = parse_config(&text).expect_err("schema version");
    assert!(matches!(err, ParseError::Invalid { field: "schema_version", .. }), "{err:?}");
}

#[test]
fn defaults_equal_the_plan_table() {
    use OutcomeName::{Block, Pass, Warn};
    let expected = Config {
        schema_version: 1,
        adapters: vec![],
        policy: PolicyTable {
            holds: Pass,
            checked: Pass,
            proposed: Pass,
            waived: Pass,
            not_checked_within_grace: Pass,
            not_checked_after_grace: Warn,
            re_approve: Warn,
            broken_affected: Block,
            broken_unaffected: Pass,
        },
        grace_days: 7,
        freshness_days: 14,
        kill_floor: 1,
        proposal_cap: 5,
        drift_threshold: 10,
        call_budget: 200,
        allow_self_signed: true,
        witness: Witness::Full,
        waiver_max_days: 90,
        scheduled_cases: 2000,
        uncovered_ignore: Vec::new(),
    };
    assert_eq!(Config::defaults(), expected);
}

#[test]
fn the_init_fixture_is_the_defaults_plus_its_adapter() {
    let adapter = Adapter {
        root: ".".into(),
        language: Language::Rust,
        runner: "nextest".into(),
        mutation_tool: "cargo-mutants".into(),
        max_mutants: None,
        jobs: None,
        features: Vec::new(),
        all_features: None,
        no_default_features: None,
    };
    assert_eq!(
        parse_config(&fixture("config.toml")).expect("parses"),
        Config { adapters: vec![adapter], ..Config::defaults() }
    );
}

fn with_rust_adapter(options: &str) -> String {
    fixture("config.toml").replacen(
        "mutation_tool = \"cargo-mutants\"\n",
        &format!("mutation_tool = \"cargo-mutants\"\n{options}"),
        1,
    )
}

#[test]
fn cargo_feature_options_parse_and_feature_names_are_checked() {
    let c = parse_config(&with_rust_adapter(
        "features = [\"sqlite\", \"serde/std\", \"large-dates\"]\nall_features = true\nno_default_features = false\n",
    ))
    .expect("parses");
    let a = &c.adapters[0];
    assert_eq!(a.features, vec!["sqlite", "serde/std", "large-dates"]);
    assert_eq!((a.all_features, a.no_default_features), (Some(true), Some(false)));
    assert!(a.sets_cargo_features());
    assert!(!parse_config(&with_rust_adapter("")).expect("parses").adapters[0].sets_cargo_features());
    for bad in ["--all-features", "a b", "a,b", "", "x/", "a/b/c", "$(rm)", "-x"] {
        let err = parse_config(&with_rust_adapter(&format!("features = [{bad:?}]\n"))).expect_err(bad);
        assert!(err.to_string().contains("adapters[0].features[0]"), "{bad}: {err}");
    }
    assert!(parse_config(&with_rust_adapter("all_features = \"yes\"\n")).is_err(), "a boolean");
}

#[test]
fn a_minimal_config_fills_in_the_defaults() {
    let minimal = "schema_version = 1\n\n[policy]\nholds = \"pass\"\nchecked = \"pass\"\nproposed = \"pass\"\n                   waived = \"pass\"\nnot_checked_within_grace = \"pass\"\nnot_checked_after_grace = \"warn\"\n                   re_approve = \"warn\"\nbroken_affected = \"block\"\nbroken_unaffected = \"pass\"\n";
    assert_eq!(parse_config(minimal).expect("parses"), Config::defaults());
}

#[test]
fn the_core_slice_of_the_defaults() {
    use Outcome::{Block, Pass, Warn};
    let expected = GateConfig {
        policy: Policy {
            holds: Pass,
            checked: Pass,
            proposed: Pass,
            waived: Pass,
            not_checked_within_grace: Pass,
            not_checked_after_grace: Warn,
            re_approve: Warn,
            broken_affected: Block,
            broken_unaffected: Pass,
        },
        grace_secs: 7 * 86_400,
        freshness_secs: 14 * 86_400,
        floor: 1,
        allow_self_signed: true,
    };
    assert_eq!(convert::gate_config(&Config::defaults()), Ok(expected));
}

#[test]
fn evidence_fixture_parses_and_converts() {
    let record = parse_evidence(&fixture("evidence.json")).expect("parses");
    let e = convert::evidence(&record).expect("converts");
    assert_eq!(ulid::encode(e.property), record.id);
    assert_eq!(e.measured, Measured::Primary(Kind::Proof, PrimaryOutcome::Verified));
    assert_eq!(e.run_id, 1_234_567_890);
    assert_eq!(e.head_sha[..4], [0x1e, 0xca, 0xe0, 0x00]);
    assert_eq!(e.head_sha[20..], [0u8; 12]);
    assert_eq!(e.spec_hash, None);
    assert_eq!(e.at, AT);
}

#[test]
fn incoherent_evidence_is_rejected() {
    let mut record = parse_evidence(&fixture("evidence.json")).expect("parses");
    record.kind = EvidenceKind::Check;
    let err = convert::evidence(&record).expect_err("a check does not verify");
    assert_eq!(err, ConvertError::at("outcome", ConvertErrorKind::Incoherent { kind: "check", outcome: "verified" }));
}

#[test]
fn evidence_with_a_status_key_is_not_the_schema() {
    let text = fixture("evidence.json").replace("\"at\":", "\"status\": \"holds\",\n  \"at\":");
    match parse_evidence(&text).expect_err("unknown key") {
        ParseError::Schema(m) => assert!(m.contains("unknown field `status`"), "{m}"),
        other => panic!("expected a schema error, got {other:?}"),
    }
}
