//! The Kani adapter against `tests/fixtures/kani-crate`: discovery and
//! binding sets (pure), the argv it builds (pure), config refusals, the
//! outcome table and the mutant-fate table over the captured Kani 0.68.0
//! outputs in `tests/fixtures/kani`, the mutant listing and its exclusion of
//! harness code, and real runs through `cargo kani` (check and score). Real runs need
//! the pinned toolchain (`cargo-kani` on `PATH` and its bundle); when it does
//! not resolve they say so and return.
use proob_adapters::argv::Checked;
use proob_adapters::kani::check::{judge, outcome, plan, stopped};
use proob_adapters::kani::score::{Fate, OPERATOR_SET, fate, fate_of, harness_code, list_plan, patch};
use proob_adapters::kani::tools::{Resolution, resolve};
use proob_adapters::kani::{KaniAdapter, parse_check_ref};
use proob_adapters::rust::Features;
use proob_adapters::{
    Adapter, Binding, Budget, Check, CheckOutcome, Location, ProofMeasure, RegistryError, Score, ScoreOutcome, Seed,
};
use proob_core::types::Kind;
use proob_facts::parsers::cargo_mutants::{Listed, parse_listing};
use proob_facts::parsers::kani::{Class, Failure};
use proob_facts::ulid;
use proob_spawn::{Output, SpawnError};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The proptest: the Rust adapter's, never bound here.
const PROPTEST: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKA";
const PASSING: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKB";
const GATED: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKC";
/// `#[test]` and `#[cfg_attr(kani, kani::proof)]`.
const TEST_AND_HARNESS: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKD";
const CONTRACT: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKE";
const PLANTED: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKF";
const UNSUPPORTED: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKG";
const SHORT_UNWIND: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKH";
const UNREACHABLE_COVER: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKJ";
/// Asserts nothing about `clamp`'s result.
const NO_ASSERTION: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKK";
/// Only Rust's bounds check looks at `clamp`'s result.
const TABLE_INDEX: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XKM";

fn id(text: &str) -> u128 {
    ulid::decode(text).expect("ulid")
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/kani-crate")
        .canonicalize()
        .expect("fixture exists")
}

fn captures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/kani")
}

fn adapter() -> KaniAdapter {
    KaniAdapter::at(".")
}

fn discover(repo: &Path) -> Vec<Binding> {
    adapter().discover(repo).expect("discovery succeeds")
}

fn binding(text: &str) -> Binding {
    discover(&fixture()).into_iter().find(|b| b.id == id(text)).unwrap_or_else(|| panic!("{text} is bound"))
}

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::write(path, text).expect("write");
}

fn reason(c: &Check) -> &str {
    match &c.outcome {
        CheckOutcome::Errored { reason } => reason,
        other => panic!("expected errored, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

#[test]
fn discovery_binds_every_kani_harness_as_a_proof_and_leaves_the_proptest_and_the_contract() {
    let found = discover(&fixture());
    let ids: Vec<u128> = found.iter().map(|b| b.id).collect();
    let expected = [
        PASSING,
        GATED,
        TEST_AND_HARNESS,
        PLANTED,
        UNSUPPORTED,
        SHORT_UNWIND,
        UNREACHABLE_COVER,
        NO_ASSERTION,
        TABLE_INDEX,
    ];
    assert_eq!(ids, expected.map(id).to_vec(), "sorted by token location");
    assert!(!ids.contains(&id(PROPTEST)) && !ids.contains(&id(CONTRACT)));
    let b = binding(PASSING);
    assert_eq!(b.kind, Kind::Proof);
    assert_eq!(b.check_ref, "kani-crate verification::clamp_stays_within_bounds");
    assert_eq!(b.token.to_string(), "src/lib.rs:19");
    assert_eq!(b.files, paths(&["Cargo.lock", "Cargo.toml", "src/lib.rs"]), "the harness file, manifest, lock");
    assert_eq!(binding(TEST_AND_HARNESS).check_ref, "kani-crate verification::clamp_is_idempotent");
    let uncovered = adapter().uncovered(&fixture()).expect("uncovered succeeds");
    assert_eq!(uncovered.len(), 1);
    assert_eq!(uncovered[0].id, id(CONTRACT));
    assert!(uncovered[0].reason.contains("Kani contracts are not supported yet"), "{}", uncovered[0].reason);
}

/// A workspace of two packages: the harness lives in a `#[cfg(kani)]` file
/// module with a child of its own; a sibling module outside `cfg(kani)` and
/// the other package are not in the set.
#[test]
fn a_file_module_harness_binds_its_cfg_kani_tree_the_manifests_and_the_lock() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n");
    write(root, "Cargo.lock", "version = 4\n");
    write(root, "crates/a/Cargo.toml", "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    write(root, "crates/a/src/lib.rs", "mod util;\n#[cfg(kani)]\nmod proofs;\n");
    write(root, "crates/a/src/util.rs", "pub fn f() {}\n");
    write(
        root,
        "crates/a/src/proofs.rs",
        "mod helpers;\n\n// proob:01K8V5NQ4A0F3R7Y2M9D6P1XWA\n#[kani::proof]\nfn holds() {}\n",
    );
    write(root, "crates/a/src/proofs/helpers.rs", "pub fn any_small() -> u8 { 3 }\n");
    write(root, "crates/a/tests/it.rs", "// proob:01K8V5NQ4A0F3R7Y2M9D6P1XWB\n#[kani::proof]\nfn in_a_test() {}\n");
    write(root, "crates/b/Cargo.toml", "[package]\nname = \"b\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    write(root, "crates/b/src/lib.rs", "pub fn g() {}\n");
    let found = discover(root);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].check_ref, "a proofs::holds");
    assert_eq!(
        found[0].files,
        paths(&[
            "Cargo.lock",
            "Cargo.toml",
            "crates/a/Cargo.toml",
            "crates/a/src/proofs/helpers.rs",
            "crates/a/src/proofs.rs"
        ])
    );
    let before = found[0].binding_hash;
    write(root, "Cargo.lock", "version = 4\n# changed\n");
    assert_ne!(discover(root)[0].binding_hash, before, "Cargo.lock is a member");
    write(root, "crates/a/src/util.rs", "pub fn f() { let _ = 1; }\n");
    let after_lock = discover(root)[0].binding_hash;
    write(root, "crates/a/src/util.rs", "pub fn f() { let _ = 2; }\n");
    assert_eq!(discover(root)[0].binding_hash, after_lock, "a module outside cfg(kani) is not");
    let uncovered = adapter().uncovered(root).expect("uncovered succeeds");
    assert_eq!(uncovered.len(), 1);
    assert_eq!(uncovered[0].token, Location { path: PathBuf::from("crates/a/tests/it.rs"), line: 1 });
    assert!(uncovered[0].reason.contains("integration test"), "{}", uncovered[0].reason);
    // The multi-package argv names its package.
    let r = parse_check_ref(&found[0].check_ref).expect("parses");
    let args = strs(
        plan(Path::new("cargo-kani"), root, Path::new("/t"), Path::new("/s"), &r, &Features::default(), Duration::ZERO)
            .args(),
    );
    assert_eq!(&args[..3], ["kani", "-p", "a"]);
}

// ---------------------------------------------------------------------------
// Argv and config
// ---------------------------------------------------------------------------

fn strs(args: &[OsString]) -> Vec<String> {
    args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

#[test]
fn the_argv_names_one_harness_exactly_and_never_quiets_or_drops_unwinding_checks() {
    let r = parse_check_ref("kani-crate verification::clamp_stays_within_bounds").expect("parses");
    let command = plan(
        Path::new("cargo-kani"),
        &fixture(),
        Path::new("/t/proob-kani"),
        Path::new("/s/k.sarif"),
        &r,
        &Features::default(),
        Duration::ZERO,
    );
    assert_eq!(
        strs(command.args()),
        [
            "kani",
            "-p",
            "kani-crate",
            "--tests",
            "--harness",
            "verification::clamp_stays_within_bounds",
            "--exact",
            "--sarif",
            "/s/k.sarif",
            "--target-dir",
            "/t/proob-kani"
        ]
    );
    assert!(!strs(command.args()).iter().any(|a| a.contains("quiet") || a.contains("unwinding")));
    assert!(parse_check_ref("kani-crate verification::*").is_err(), "a pattern never reaches --harness");
}

#[test]
fn config_refuses_what_the_kani_adapter_does_not_take() {
    use proob_facts::config::parse_config;
    let config = |runner: &str, options: &str| {
        let text = format!(
            "schema_version = 1\n[[adapters]]\nroot = \".\"\nlanguage = \"kani\"\nrunner = \"{runner}\"\nmutation_tool = \"cargo-mutants\"\n{options}\n[policy]\nholds = \"pass\"\nchecked = \"pass\"\nproposed = \"pass\"\nwaived = \"pass\"\nnot_checked_within_grace = \"pass\"\nnot_checked_after_grace = \"warn\"\nre_approve = \"warn\"\nbroken_affected = \"block\"\nbroken_unaffected = \"pass\"\n"
        );
        proob_adapters::adapters_for(&parse_config(&text).expect("parses"))
    };
    let ok = config("kani", "max_mutants = 10").expect("accepted");
    assert_eq!(ok.len(), 1);
    assert_eq!(ok[0].language(), proob_adapters::Language::Kani);
    assert_eq!(proob_adapters::language_name(proob_adapters::Language::Kani), "kani");
    let refused = |runner: &str, options: &str| config(runner, options).err().expect("refused");
    let err = refused("kani", "jobs = 2");
    assert!(matches!(err, RegistryError::UnsupportedOption { option: "jobs", .. }), "{err}");
    assert!(err.to_string().contains("one at a time"), "says why: {err}");
    assert!(matches!(
        refused("kani", "max_mutants = 0"),
        RegistryError::UnsupportedOption { option: "max_mutants", .. }
    ));
    assert!(matches!(refused("nextest", ""), RegistryError::UnsupportedRunner { expected: "kani", .. }));
}

#[test]
fn cargo_features_go_identically_on_every_cargo_invocation_each_name_after_its_flag() {
    let r = parse_check_ref("kani-crate verification::clamp_stays_within_bounds").expect("parses");
    let files = [Checked::path(Path::new("src/lib.rs")).expect("checked")];
    let names = ["sqlite", "serde/std"].map(|n| Checked::feature(n).expect("feature")).to_vec();
    let features = Features { names, all: true, no_default: true };
    let flags = ["--all-features", "--no-default-features", "--features", "sqlite", "--features", "serde/std"];
    // The check's, the probe's, and every score run's `cargo kani` is this one plan (`Lab::harness`).
    let kani =
        plan(Path::new("cargo-kani"), &fixture(), Path::new("/t"), Path::new("/s"), &r, &features, Duration::ZERO);
    let listing = list_plan(Path::new("cargo"), &fixture(), &r.package, &features, &files);
    for (what, command) in [("cargo kani", &kani), ("listing", &listing)] {
        let args = strs(command.args());
        assert_eq!(&args[1..1 + flags.len()], flags, "{what}: right after the subcommand: {args:?}");
        assert_eq!(args.iter().filter(|a| a.contains("feature")).count(), 4, "{what}: once each: {args:?}");
    }
}

#[test]
fn cargo_feature_options_are_read_from_a_kani_row_and_a_bad_name_is_refused() {
    use proob_facts::config::parse_config;
    let text = |options: &str| {
        format!(
            "schema_version = 1\n[[adapters]]\nroot = \".\"\nlanguage = \"kani\"\nrunner = \"kani\"\nmutation_tool = \"cargo-mutants\"\n{options}\n[policy]\nholds = \"pass\"\nchecked = \"pass\"\nproposed = \"pass\"\nwaived = \"pass\"\nnot_checked_within_grace = \"pass\"\nnot_checked_after_grace = \"warn\"\nre_approve = \"warn\"\nbroken_affected = \"block\"\nbroken_unaffected = \"pass\"\n"
        )
    };
    let config = parse_config(&text("features = [\"sqlite\"]\nno_default_features = true")).expect("parses");
    let root = Checked::path(Path::new(".")).expect("root");
    let configured = KaniAdapter::configured(&config.adapters[0], root).expect("kani takes them");
    assert_eq!(
        configured.features(),
        &Features { names: vec![Checked::feature("sqlite").expect("feature")], all: false, no_default: true }
    );
    assert!(proob_adapters::adapters_for(&config).is_ok(), "the registry lets them through");
    let err = parse_config(&text("features = [\"--all-features\"]")).expect_err("not a feature name");
    assert!(err.to_string().contains("adapters[0].features[0]"), "{err}");
}

// ---------------------------------------------------------------------------
// The outcome table over captured outputs
// ---------------------------------------------------------------------------

fn captured(case: &str) -> (Output, Option<String>) {
    let read = |suffix: &str| std::fs::read(captures().join(format!("{case}.{suffix}"))).unwrap_or_default();
    let code =
        std::fs::read_to_string(captures().join(format!("{case}.exit"))).ok().and_then(|t| t.trim().parse().ok());
    let sarif = std::fs::read_to_string(captures().join(format!("{case}.sarif"))).ok();
    (Output { code, stdout: read("stdout.txt"), stderr: read("stderr.txt") }, sarif)
}

fn judged(case: &str, harness: &str) -> CheckOutcome {
    let (out, sarif) = captured(case);
    judge(&out, sarif.as_deref(), &parse_check_ref(&format!("kani-capture verification::{harness}")).expect("ref"))
}

fn errored(case: &str, harness: &str) -> String {
    match judged(case, harness) {
        CheckOutcome::Errored { reason } => reason,
        other => panic!("{case}: expected errored, got {other:?}"),
    }
}

#[test]
fn each_captured_run_lands_on_its_row_of_the_outcome_table() {
    assert_eq!(judged("pass", "pass"), CheckOutcome::Verified { measure: ProofMeasure::Kani { checks: 1 } });
    for case in ["fail_assert", "fail_overflow"] {
        match judged(case, case) {
            CheckOutcome::ProofFailed { diagnostics } => {
                assert!(
                    diagnostics.starts_with("Failed Checks: ") && diagnostics.contains("src/lib.rs"),
                    "{diagnostics}"
                )
            }
            other => panic!("{case}: {other:?}"),
        }
    }
    let rows = [
        (
            "unwind_short",
            "unwind_short",
            "the harness's unwind bound is too small for a loop: unwinding assertion loop 0",
        ),
        ("unsupported", "unsupported", "Kani cannot verify a construct this code reaches: TerminatorKind::InlineAsm"),
        ("cover_unsat", "cover_unsat", "vacuous: the harness cannot reach a case it covers: cover condition: x < 0"),
        ("harness_not_found", "nope", "harness not found: Kani has no harness `verification::nope`"),
        (
            "preprocessor_failed",
            "pass",
            "Kani's C preprocessor failed; a non-system compiler on PATH may shadow the system one: <home>/.alire",
        ),
        ("compile_error", "broken", "the crate did not compile under Kani: error[E0308]: mismatched types"),
    ];
    for (case, harness, start) in rows {
        let reason = errored(case, harness);
        assert!(reason.starts_with(start), "{case}: {reason}");
    }
}

#[test]
fn a_run_that_proves_nothing_or_contradicts_its_exit_code_is_errored() {
    let r = parse_check_ref("p verification::h").expect("ref");
    let reason = |o: CheckOutcome| match o {
        CheckOutcome::Errored { reason } => reason,
        other => panic!("{other:?}"),
    };
    assert!(reason(outcome(Class::Verified { checks: 0 }, Some(0), &r)).starts_with("nothing was checked"));
    assert!(reason(outcome(Class::Verified { checks: 3 }, Some(1), &r)).starts_with("unrecognised Kani output"));
    assert!(reason(outcome(Class::Verified { checks: 3 }, None, &r)).starts_with("unrecognised Kani output"));
    assert_eq!(
        outcome(Class::Timeout, Some(1), &r),
        CheckOutcome::Errored { reason: "Kani timed out: CBMC hit its time limit".into() }
    );
    assert_eq!(
        outcome(Class::OutOfMemory, Some(1), &r),
        CheckOutcome::Errored { reason: "Kani ran out of memory".into() }
    );
    assert!(
        reason(outcome(Class::Unrecognised { detail: "x".into() }, Some(1), &r))
            .starts_with("unrecognised Kani output")
    );
    assert!(reason(outcome(Class::Vacuous { assertion: "a".into() }, Some(0), &r)).starts_with("vacuous: "));
    // Killed mid-run (the system reclaiming memory, a watchdog): no verdict, no exit code.
    assert_eq!(
        reason(outcome(Class::Unrecognised { detail: "no VERIFICATION line".into() }, None, &r)),
        "Kani was killed by a signal before it finished; on a machine short of memory, the system may have stopped CBMC"
    );
}

#[test]
fn a_run_stopped_for_memory_is_errored_never_a_verdict() {
    let oom = SpawnError::OutOfMemory {
        program: PathBuf::from("cargo-kani"),
        limit_bytes: 4096 << 20,
        peak_bytes: 5000 << 20,
    };
    assert_eq!(
        stopped(&oom),
        CheckOutcome::Errored {
            reason: "Kani used more than 4096 MB and was stopped (out of memory); raise PROOB_MEMORY_LIMIT_MB or shrink the harness's bounds".into()
        }
    );
}

#[test]
fn a_missing_toolchain_yields_errored_with_the_stable_hint() {
    let home = tempfile::tempdir().expect("tempdir");
    let env = vec![
        (OsString::from("PATH"), home.path().join("nothing").into_os_string()),
        (OsString::from("HOME"), home.path().as_os_str().to_os_string()),
        (OsString::from("KANI_HOME"), OsString::new()),
    ];
    let none = KaniAdapter::at(".").with_tool_env(env);
    let out = none.check(&fixture(), &binding(PASSING), Budget(1), Seed(1)).expect("check returns a result");
    let reason = reason(&out);
    assert!(reason.starts_with("the Kani toolchain was not found"), "{reason}");
    assert!(
        reason.contains("cargo install --locked --root $HOME/.proob/tools/rust kani-verifier --version 0.68.0"),
        "{reason}"
    );
    assert!(reason.contains("cargo kani setup"), "{reason}");
}

// ---------------------------------------------------------------------------
// Real runs
// ---------------------------------------------------------------------------

/// Whether the pinned toolchain resolves here; a note when it does not.
/// `PROOB_REQUIRE_KANI` (CI's kani job) turns that skip into a failure.
fn kani_present() -> bool {
    match resolve(adapter().tool_env()) {
        Resolution::Ready(_) => true,
        Resolution::Missing { reason, .. } => {
            assert!(std::env::var_os("PROOB_REQUIRE_KANI").is_none(), "PROOB_REQUIRE_KANI is set but {reason}");
            eprintln!("skipped: {reason}");
            false
        }
    }
}

/// Real runs one at a time: Kani's cargo rewrites the fixture's Cargo.lock
/// for the length of a check (`check::restore` puts it back), which a
/// parallel score's before-and-after comparison would otherwise see.
static REAL_RUN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn check(b: &Binding) -> Check {
    let _one = REAL_RUN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let out = adapter().check(&fixture(), b, Budget(1), Seed(1)).expect("check runs");
    eprintln!("check({}) = {:?}", b.check_ref, out.outcome);
    out
}

#[test]
fn a_passing_harness_is_verified_within_its_bounds_and_the_versions_are_recorded() {
    if !kani_present() {
        return;
    }
    for text in [PASSING, TEST_AND_HARNESS] {
        let out = check(&binding(text));
        assert!(
            matches!(out.outcome, CheckOutcome::Verified { measure: ProofMeasure::Kani { checks } } if checks > 0),
            "{text}: {:?}",
            out.outcome
        );
        let version = |k: &str| out.tool_versions.iter().find(|(name, _)| name == k).map(|(_, v)| v.as_str());
        assert_eq!(version("kani"), Some("0.68.0"));
        assert_eq!(version("nightly"), Some("nightly-2026-08-21"));
        assert!(version("cbmc").is_some(), "{:?}", out.tool_versions);
    }
}

#[test]
fn a_planted_bug_fails_the_proof_with_kanis_failed_check() {
    if !kani_present() {
        return;
    }
    match check(&binding(PLANTED)).outcome {
        CheckOutcome::ProofFailed { diagnostics } => {
            assert!(diagnostics.contains("assertion failed: clamp(x, lo, hi) == x"), "{diagnostics}");
            assert!(diagnostics.contains("src/lib.rs"), "{diagnostics}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn runs_kani_cannot_complete_are_errored_with_the_reason() {
    if !kani_present() {
        return;
    }
    let unsupported = check(&binding(UNSUPPORTED));
    assert!(reason(&unsupported).starts_with("Kani cannot verify a construct this code reaches: "), "{unsupported:?}");
    let unwind = check(&binding(SHORT_UNWIND));
    assert!(reason(&unwind).contains("unwind bound is too small for a loop: unwinding assertion loop"), "{unwind:?}");
    let cover = check(&binding(UNREACHABLE_COVER));
    assert!(reason(&cover).starts_with("vacuous: the harness cannot reach a case it covers: "), "{cover:?}");
    let mut gone = binding(PASSING);
    gone.check_ref = "kani-crate verification::no_such_harness".to_string();
    let gone = check(&gone);
    assert!(
        reason(&gone).starts_with("harness not found: Kani has no harness `verification::no_such_harness`"),
        "{gone:?}"
    );
}

#[test]
fn a_check_leaves_cargo_lock_as_it_found_it() {
    use proob_adapters::kani::check::restore;
    let dir = tempfile::tempdir().expect("temp");
    let lock = dir.path().join("Cargo.lock");
    std::fs::write(&lock, "rewritten").expect("write");
    restore(&lock, Some(b"original")).expect("restore");
    assert_eq!(std::fs::read(&lock).expect("read"), b"original", "a rewritten lock file is put back");
    restore(&lock, Some(b"original")).expect("restore");
    assert_eq!(std::fs::read(&lock).expect("read"), b"original", "an unchanged lock file is left alone");
    restore(&lock, None).expect("restore");
    assert!(!lock.exists(), "a lock file Kani created where there was none is removed");
    restore(&lock, None).expect("restore");
    assert!(!lock.exists(), "no lock file before or after is a no-op");
}

// ---------------------------------------------------------------------------
// Score: listing, exclusion, fates (pure)
// ---------------------------------------------------------------------------

fn listing() -> Vec<Listed> {
    let json = std::fs::read_to_string(captures().join("mutants_list.json")).expect("captured listing");
    parse_listing(&json).expect("parses")
}

fn fixture_files() -> Vec<Checked> {
    vec![Checked::path(Path::new("src/lib.rs")).expect("checked")]
}

#[test]
fn the_captured_listing_keeps_only_mutants_outside_harness_code_and_every_diff_applies() {
    let listed = listing();
    assert!(listed.iter().all(|m| !m.diff.is_empty()), "every mutant carries its diff");
    let code = harness_code(&adapter(), &fixture(), &fixture_files()).expect("reads");
    let kept: Vec<&Listed> = listed.iter().filter(|m| !code.contains(m)).collect();
    assert!(!kept.is_empty() && kept.len() < listed.len());
    assert!(
        kept.iter().all(|m| m.function.as_deref() == Some("clamp")),
        "{:?}",
        kept.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
    assert!(listed.iter().filter(|m| m.function.as_deref() != Some("clamp")).all(|m| code.contains(m)));
    let text = std::fs::read_to_string(fixture().join("src/lib.rs")).expect("fixture");
    for m in &listed {
        let mutated = patch(&text, &m.diff).unwrap_or_else(|e| panic!("{}: {e}", m.name));
        assert_ne!(mutated, text, "{}", m.name);
        assert!(mutated.contains("~ changed by cargo-mutants ~"), "{}", m.name);
    }
}

/// A harness file reached through `#[cfg(kani)] mod proofs;`, and a harness
/// outside any `cfg(kani)` module, in a crate of their own.
#[test]
fn harness_files_and_harness_functions_are_harness_code() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", "[package]\nname = \"h\"\nversion = \"0.1.0\"\nedition = \"2024\"\n");
    write(
        root,
        "src/lib.rs",
        "#[cfg(kani)]\nmod proofs;\npub fn f() -> i32 { 1 }\n\n// proob:01K8V5NQ4A0F3R7Y2M9D6P1XWC\n#[cfg_attr(kani, kani::proof)]\nfn bare() { assert!(f() == 1); }\n",
    );
    write(
        root,
        "src/proofs.rs",
        "// proob:01K8V5NQ4A0F3R7Y2M9D6P1XWD\n#[kani::proof]\nfn holds() { assert!(super::f() == 1); }\n",
    );
    let files = ["src/lib.rs", "src/proofs.rs"].map(|f| Checked::path(Path::new(f)).expect("checked"));
    let code = harness_code(&adapter(), root, &files).expect("reads");
    let mutant = |file: &str, line: usize, function: Option<&str>| Listed {
        name: format!("{file}:{line}"),
        file: file.to_string(),
        line,
        function: function.map(str::to_string),
        diff: String::new(),
    };
    assert!(code.contains(&mutant("src/proofs.rs", 3, Some("holds"))), "a file under cfg(kani)");
    assert!(code.contains(&mutant("src/lib.rs", 7, Some("bare"))), "a harness function");
    assert!(!code.contains(&mutant("src/lib.rs", 3, Some("f"))), "the code under proof");
}

#[test]
fn the_listing_argv_names_the_package_and_each_scope_file_and_asks_for_diffs() {
    let r = parse_check_ref("kani-crate verification::clamp_stays_within_bounds").expect("parses");
    let files = [
        Checked::path(Path::new("src/lib.rs")).expect("checked"),
        Checked::path(Path::new("src/a.rs")).expect("checked"),
    ];
    let command = list_plan(Path::new("cargo"), &fixture(), &r.package, &Features::default(), &files);
    assert_eq!(
        strs(command.args()),
        [
            "mutants",
            "--package",
            "kani-crate",
            "--file",
            "src/lib.rs",
            "--file",
            "src/a.rs",
            "--list",
            "--json",
            "--diff"
        ]
    );
}

fn fate_of_capture(case: &str) -> Fate {
    let (out, sarif) = captured(case);
    let harness = match case {
        "harness_not_found" => "verification::nope".to_string(),
        "preprocessor_failed" => "verification::pass".to_string(),
        "compile_error" => "verification::broken".to_string(),
        _ => format!("verification::{case}"),
    };
    fate(Ok(&out), sarif.as_deref(), &harness).expect("a fate")
}

#[test]
fn each_captured_run_lands_on_its_row_of_the_mutant_fate_table() {
    let rows = [
        ("fail_assert", Fate::Killed),
        ("fail_overflow", Fate::Safety),
        ("pass", Fate::Survived),
        ("unwind_short", Fate::Timeout),
        ("unsupported", Fate::Unviable),
        ("cover_unsat", Fate::Unviable),
        ("harness_not_found", Fate::Unviable),
        ("preprocessor_failed", Fate::Unviable),
        ("compile_error", Fate::Unviable),
    ];
    for (case, expected) in rows {
        assert_eq!(fate_of_capture(case), expected, "{case}");
    }
}

#[test]
fn every_class_has_a_fate() {
    let failed = |failure| Class::Failed { diagnostics: String::new(), failure };
    let rows = [
        (Class::Verified { checks: 3 }, Some(0), Fate::Survived),
        (Class::Verified { checks: 0 }, Some(0), Fate::Survived),
        (Class::Verified { checks: 3 }, Some(1), Fate::Unviable),
        (failed(Failure::Assertion), Some(1), Fate::Killed),
        (failed(Failure::Safety), Some(1), Fate::Safety),
        (Class::Unwinding { at: String::new() }, Some(1), Fate::Timeout),
        (Class::Undetermined { checks: vec![] }, Some(1), Fate::Timeout),
        (Class::Timeout, Some(1), Fate::Timeout),
        (Class::OutOfMemory, Some(1), Fate::Timeout),
        (Class::Unsupported { construct: String::new() }, Some(1), Fate::Unviable),
        (Class::VacuousCover { cover: String::new() }, Some(0), Fate::Unviable),
        (Class::Vacuous { assertion: String::new() }, Some(0), Fate::Survived),
        (Class::PreprocessorFailed { detail: String::new() }, Some(1), Fate::Unviable),
        (Class::CompileError { detail: String::new() }, Some(1), Fate::Unviable),
        (Class::HarnessNotFound, Some(1), Fate::Unviable),
        (Class::Unrecognised { detail: String::new() }, Some(1), Fate::Unviable),
    ];
    for (class, code, expected) in rows {
        assert_eq!(fate_of(&class, code), expected, "{class:?}");
    }
    let timed_out = SpawnError::TimedOut { program: PathBuf::from("cargo-kani"), after: Duration::from_secs(1) };
    assert_eq!(fate(Err(&timed_out), None, "h"), Ok(Fate::Timeout), "the per-mutant limit");
    let oom =
        SpawnError::OutOfMemory { program: PathBuf::from("cargo-kani"), limit_bytes: 1 << 30, peak_bytes: 2 << 30 };
    assert_eq!(fate(Err(&oom), None, "h"), Ok(Fate::Timeout), "the memory budget: no verdict, never killed");
    let start = SpawnError::Start { program: PathBuf::from("cargo-kani"), message: "gone".into() };
    assert!(fate(Err(&start), None, "h").is_err(), "a toolchain that cannot start aborts the score");
}

#[test]
fn a_missing_toolchain_scores_errored_with_the_stable_hint() {
    let home = tempfile::tempdir().expect("tempdir");
    let env = vec![
        (OsString::from("PATH"), home.path().join("nothing").into_os_string()),
        (OsString::from("HOME"), home.path().as_os_str().to_os_string()),
        (OsString::from("KANI_HOME"), OsString::new()),
    ];
    let none = KaniAdapter::at(".").with_tool_env(env);
    let out = none.score(&fixture(), &binding(PASSING), &paths(&["src/lib.rs"])).expect("score returns a result");
    let ScoreOutcome::Errored { reason } = out.outcome else { panic!("{:?}", out.outcome) };
    assert!(reason.starts_with("the Kani toolchain was not found"), "{reason}");
}

// ---------------------------------------------------------------------------
// Score: real runs
// ---------------------------------------------------------------------------

fn score(text: &str) -> Score {
    let _one = REAL_RUN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let lock = fixture().join("Cargo.lock");
    let before = std::fs::read(&lock).expect("lock");
    let started = std::time::Instant::now();
    let b = binding(text);
    let out = adapter().score(&fixture(), &b, &paths(&["src/lib.rs"])).expect("score runs");
    eprintln!("score({}) = {:?} in {:.0?}", b.check_ref, out.outcome, started.elapsed());
    assert_eq!(std::fs::read(&lock).expect("lock"), before, "Cargo.lock is left as it was");
    out
}

fn counts(s: &Score) -> (u32, u32, u32, u32, u32) {
    match s.outcome {
        ScoreOutcome::Scored { killed, survived, timeout, unviable, unsampled } => {
            (killed, survived, timeout, unviable, unsampled)
        }
        ref other => panic!("{other:?}"),
    }
}

#[test]
fn a_meaningful_harness_kills_mutants_and_records_its_tools() {
    if !kani_present() {
        return;
    }
    let s = score(PASSING);
    let (killed, ..) = counts(&s);
    assert!(killed >= 1, "{:?}", s.outcome);
    let version = |k: &str| s.tool_versions.iter().find(|(name, _)| name == k).map(|(_, v)| v.as_str());
    assert_eq!(version("mutants-under-kani"), Some(OPERATOR_SET));
    assert_eq!(version("kani"), Some("0.68.0"));
    assert!(version("cargo-mutants").is_some(), "{:?}", s.tool_versions);
}

#[test]
fn weak_harnesses_kill_nothing() {
    if !kani_present() {
        return;
    }
    let (killed, survived, ..) = counts(&score(NO_ASSERTION));
    assert_eq!(killed, 0, "an assertion of what was assumed catches no mutant");
    assert!(survived > 0);
    // A score never leaves a mutant's build where a check, or the next score's baseline, would reuse it.
    assert!(matches!(check(&binding(NO_ASSERTION)).outcome, CheckOutcome::Verified { .. }));
    // Only Rust's bounds check looks at the result: the harness states nothing, so it never verifies.
    let CheckOutcome::Errored { reason } = check(&binding(TABLE_INDEX)).outcome else {
        panic!("verified a harness with no assertion")
    };
    assert!(reason.starts_with("vacuous: "), "{reason}");
    let ScoreOutcome::Errored { reason } = score(TABLE_INDEX).outcome else { panic!("scored a vacuous baseline") };
    assert!(reason.starts_with("baseline failed: "), "{reason}");
}

#[test]
fn a_baseline_that_does_not_verify_is_errored_naming_the_failure() {
    if !kani_present() {
        return;
    }
    let ScoreOutcome::Errored { reason } = score(PLANTED).outcome else { panic!("scored a failing baseline") };
    assert!(reason.starts_with("baseline failed: the harness does not verify before any mutation"), "{reason}");
    assert!(reason.contains("assertion failed: clamp(x, lo, hi) == x"), "{reason}");
}
