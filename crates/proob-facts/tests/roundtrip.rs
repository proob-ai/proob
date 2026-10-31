//! Round trips, property-based: ULID text, calendar dates, property files and
//! configs through TOML; verified evidence records of either measure through
//! JSON; and the agreement of the verified glob checker with
//! an independent plain-Rust reference.
use proob_core::grammar::glob_ok_exec;
use proob_core::types::{Kind, Measured, PrimaryOutcome};
use proob_facts::config::{Adapter, Config, Language, OutcomeName, PolicyTable, Witness, parse_config};
use proob_facts::convert::{self, GlobRule};
use proob_facts::date::Date;
use proob_facts::evidence::{OutcomeRecord, parse_evidence};
use proob_facts::property::{KindName, PropertyFile, SeverityName, WaiverFile, parse_property};
use proob_facts::ulid::{self, UlidError};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

const EVIDENCE: &str = include_str!("../../../tests/fixtures/properties/evidence.json");

// ---------------------------------------------------------------------------
// Independent references, written from the prose and not from the core
// ---------------------------------------------------------------------------

fn reference_glob_ok(bytes: &[u8]) -> bool {
    const EXTRA: &[u8] = b"/._-*?[]{},@+~";
    match bytes {
        [] | [b'/', ..] | [b'-', ..] => false,
        _ => {
            bytes.iter().all(|b| b.is_ascii_alphanumeric() || EXTRA.contains(b))
                && !bytes.split(|&b| b == b'/').any(|segment| segment == b"..")
        }
    }
}

fn reference_leap(y: u16) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn reference_days_in_month(y: u16, m: u8) -> u8 {
    match m {
        2 if reference_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn reference_days_since_epoch(y: u16, m: u8, d: u8) -> u64 {
    let years: u64 = (1970..y).map(|y| if reference_leap(y) { 366 } else { 365 }).sum();
    let months: u64 = (1..m).map(|m| u64::from(reference_days_in_month(y, m))).sum();
    years + months + u64::from(d) - 1
}

fn fail(e: impl std::fmt::Display) -> TestCaseError {
    TestCaseError::fail(e.to_string())
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

fn ulid_text() -> impl Strategy<Value = String> {
    ("[0-7]", "[0-9A-HJKMNP-TV-Z]{25}").prop_map(|(first, rest)| format!("{first}{rest}"))
}

fn overflowing_ulid_text() -> impl Strategy<Value = String> {
    ("[89A-HJKMNP-TV-Z]", "[0-9A-HJKMNP-TV-Z]{25}").prop_map(|(first, rest)| format!("{first}{rest}"))
}

fn glob_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_][a-zA-Z0-9_./*?\\[\\]{},@+~-]{0,24}".prop_filter("in the grammar", |g| reference_glob_ok(g.as_bytes()))
}

/// Bytes biased towards the interesting ones: slashes, dots, dashes, and the
/// occasional byte from the whole range.
fn glob_bytes() -> impl Strategy<Value = Vec<u8>> {
    let byte = prop_oneof![4 => b'a'..=b'z', 2 => Just(b'/'), 2 => Just(b'.'), 1 => Just(b'-'), 1 => any::<u8>()];
    prop::collection::vec(byte, 0..24)
}

fn date() -> impl Strategy<Value = Date> {
    (1970u16..=9999, 1u8..=12)
        .prop_flat_map(|(y, m)| (Just(y), Just(m), 1..=reference_days_in_month(y, m)))
        .prop_map(|(y, m, d)| Date::new(y, m, d).expect("a calendar day"))
}

fn text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ,.:;!?'()-]{1,40}"
}

fn severity() -> impl Strategy<Value = SeverityName> {
    prop_oneof![
        Just(SeverityName::Low),
        Just(SeverityName::Medium),
        Just(SeverityName::High),
        Just(SeverityName::Critical)
    ]
}

fn kind_name() -> impl Strategy<Value = KindName> {
    prop_oneof![Just(KindName::Test), Just(KindName::Proof), Just(KindName::Model)]
}

fn waiver() -> impl Strategy<Value = WaiverFile> {
    (date(), "#[1-9][0-9]{0,4}", text()).prop_map(|(until, issue, reason)| WaiverFile { until, issue, reason })
}

fn property_file() -> impl Strategy<Value = PropertyFile> {
    (
        ulid_text(),
        text(),
        text(),
        prop::collection::vec(glob_text(), 1..4),
        severity(),
        text(),
        prop::option::of(kind_name()),
        prop::option::of(date()),
        prop::option::of(waiver()),
    )
        .prop_map(|(id, title, statement, scope, severity, rationale, kind, reviewed, waiver)| PropertyFile {
            id,
            title,
            statement,
            scope,
            severity,
            rationale,
            kind,
            reviewed,
            waiver,
        })
}

fn outcome_name() -> impl Strategy<Value = OutcomeName> {
    prop_oneof![Just(OutcomeName::Pass), Just(OutcomeName::Warn), Just(OutcomeName::Block)]
}

/// `pass` or `warn`: what every row but `broken_affected` may say (R10).
fn non_blocking_outcome_name() -> impl Strategy<Value = OutcomeName> {
    prop_oneof![Just(OutcomeName::Pass), Just(OutcomeName::Warn)]
}

fn policy_table() -> impl Strategy<Value = PolicyTable> {
    (prop::collection::vec(non_blocking_outcome_name(), 9), outcome_name()).prop_map(|(o, broken_affected)| {
        PolicyTable {
            holds: o[0],
            checked: o[1],
            tested: o[8],
            proposed: o[2],
            waived: o[3],
            not_checked_within_grace: o[4],
            not_checked_after_grace: o[5],
            re_approve: o[6],
            broken_affected,
            broken_unaffected: o[7],
        }
    })
}

fn language() -> impl Strategy<Value = Language> {
    prop_oneof![Just(Language::Rust), Just(Language::Verus), Just(Language::Python), Just(Language::Tla)]
}

fn adapter() -> impl Strategy<Value = Adapter> {
    let features = proptest::collection::vec("[a-z_][a-z0-9_+.-]{0,8}(/[a-z0-9_][a-z0-9_-]{0,6})?", 0..3);
    let flag = || proptest::option::of(proptest::bool::ANY);
    (
        glob_text(),
        language(),
        text(),
        text(),
        proptest::option::of(1u32..200),
        proptest::option::of(1u32..64),
        features,
        flag(),
        flag(),
    )
        .prop_map(
            |(
                root,
                language,
                runner,
                mutation_tool,
                max_mutants,
                jobs,
                features,
                all_features,
                no_default_features,
            )| Adapter {
                root,
                language,
                runner,
                mutation_tool,
                max_mutants,
                jobs,
                features,
                all_features,
                no_default_features,
            },
        )
}

fn witness() -> impl Strategy<Value = Witness> {
    prop_oneof![Just(Witness::Full), Just(Witness::SeedOnly)]
}

fn config() -> impl Strategy<Value = Config> {
    (
        prop::collection::vec(adapter(), 0..3),
        policy_table(),
        (any::<u32>(), any::<u32>(), 1u32..=u32::MAX, any::<u32>(), any::<u32>(), any::<u32>()),
        any::<bool>(),
        witness(),
        any::<u32>(),
    )
        .prop_map(
            |(adapters, policy, (grace, fresh, floor, cap, drift, budget), self_signed, witness, max_days)| Config {
                schema_version: 1,
                adapters,
                policy,
                grace_days: grace,
                freshness_days: fresh,
                kill_floor: floor,
                proposal_cap: cap,
                drift_threshold: drift,
                call_budget: budget,
                allow_self_signed: self_signed,
                witness,
                waiver_max_days: max_days,
                scheduled_cases: 2000,
                uncovered_ignore: Vec::new(),
            },
        )
}

/// A `verified` outcome with exactly one measure: Verus's or Kani's.
fn verified_outcome() -> impl Strategy<Value = OutcomeRecord> {
    prop_oneof![
        any::<u32>().prop_map(|r| OutcomeRecord::Verified { rlimit: Some(r), checks: None }),
        any::<u32>().prop_map(|c| OutcomeRecord::Verified { rlimit: None, checks: Some(c) }),
    ]
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn ulid_value_round_trips(value in any::<u128>()) {
        prop_assert_eq!(ulid::decode(&ulid::encode(value)), Ok(value));
    }

    #[test]
    fn ulid_text_round_trips_and_is_case_insensitive(text in ulid_text()) {
        let value = ulid::decode(&text).map_err(fail)?;
        prop_assert_eq!(ulid::encode(value), text.clone());
        prop_assert_eq!(ulid::decode(&text.to_ascii_lowercase()), Ok(value));
    }

    #[test]
    fn ulid_text_above_128_bits_is_rejected(text in overflowing_ulid_text()) {
        prop_assert_eq!(ulid::decode(&text), Err(UlidError::Overflow));
    }

    #[test]
    fn date_round_trips_and_matches_the_reference(d in date()) {
        let expected = reference_days_since_epoch(d.year(), d.month(), d.day()) * 86_400;
        prop_assert_eq!(d.to_secs(), expected);
        prop_assert_eq!(Date::from_secs(d.to_secs()), Some(d));
        prop_assert_eq!(Date::from_secs(d.to_secs() + 86_399), Some(d));
    }

    #[test]
    fn verified_glob_checker_agrees_with_the_reference(bytes in glob_bytes()) {
        let expected = reference_glob_ok(&bytes);
        prop_assert_eq!(glob_ok_exec(&bytes), expected);
        match convert::glob(&bytes) {
            Ok(()) => prop_assert!(expected),
            Err(rule) => {
                prop_assert!(!expected);
                prop_assert_ne!(rule, GlobRule::Rejected);
            }
        }
    }

    #[test]
    fn generated_globs_pass_the_verified_checker(glob in glob_text()) {
        prop_assert!(glob_ok_exec(glob.as_bytes()));
    }

    // proob:01M622RQ7AM90T2YNQHJ6P58PW
    #[test]
    fn property_file_round_trips_through_toml_and_converts(file in property_file()) {
        let text = toml::to_string(&file).map_err(fail)?;
        let back = parse_property(&text).map_err(fail)?;
        prop_assert_eq!(&back, &file);
        let p = convert::property(&back, 0, u32::MAX).map_err(fail)?;
        prop_assert_eq!(ulid::encode(p.id), file.id.clone());
        prop_assert_eq!(p.scope, file.scope.iter().map(|g| g.as_bytes().to_vec()).collect::<Vec<_>>());
        prop_assert_eq!(p.waiver.map(|w| w.until), file.waiver.as_ref().map(|w| w.until.to_secs()));
        prop_assert_eq!(p.reviewed, file.reviewed.map(Date::to_secs));
        prop_assert_eq!(p.kind.is_some(), file.kind.is_some());
    }

    #[test]
    fn verified_evidence_round_trips_through_json_and_converts(outcome in verified_outcome()) {
        let mut record = parse_evidence(EVIDENCE).map_err(fail)?;
        record.outcome = outcome;
        let back = parse_evidence(&serde_json::to_string(&record).map_err(fail)?).map_err(fail)?;
        prop_assert_eq!(&back, &record);
        let e = convert::evidence(&back).map_err(fail)?;
        prop_assert_eq!(e.measured, Measured::Primary(Kind::Proof, PrimaryOutcome::Verified));
    }

    #[test]
    fn config_round_trips_through_toml(c in config()) {
        let text = toml::to_string(&c).map_err(fail)?;
        prop_assert_eq!(parse_config(&text).map_err(fail)?, c);
    }
}
