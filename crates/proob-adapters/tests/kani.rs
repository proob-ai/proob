//! The Kani adapter against `tests/fixtures/kani-crate`: discovery and
//! binding sets (pure), the argv it builds (pure), config refusals, the
//! outcome table over the captured Kani 0.68.0 outputs in
//! `tests/fixtures/kani`, and real runs through `cargo kani`. Real runs need
//! the pinned toolchain (`cargo-kani` on `PATH` and its bundle); when it does
//! not resolve they say so and return.
use proob_adapters::kani::check::{judge, outcome, plan};
use proob_adapters::kani::tools::{Resolution, resolve};
use proob_adapters::kani::{KaniAdapter, parse_check_ref};
use proob_adapters::{Adapter, Binding, Budget, Check, CheckOutcome, Location, ProofMeasure, RegistryError, Seed};
use proob_core::types::Kind;
use proob_facts::parsers::kani::Class;
use proob_facts::ulid;
use proob_spawn::Output;
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
    let expected = [PASSING, GATED, TEST_AND_HARNESS, PLANTED, UNSUPPORTED, SHORT_UNWIND, UNREACHABLE_COVER];
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
    let args = strs(plan(Path::new("cargo-kani"), root, Path::new("/t"), Path::new("/s"), &r, Duration::ZERO).args());
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
    for options in ["features = [\"x\"]", "all_features = true", "no_default_features = true"] {
        let err = refused("kani", options);
        assert!(matches!(err, RegistryError::UnsupportedOption { option: "features", .. }), "{err}");
    }
    let err = refused("kani", "jobs = 2");
    assert!(matches!(err, RegistryError::UnsupportedOption { option: "jobs", .. }), "{err}");
    assert!(err.to_string().contains("one at a time"), "says why: {err}");
    assert!(matches!(
        refused("kani", "max_mutants = 0"),
        RegistryError::UnsupportedOption { option: "max_mutants", .. }
    ));
    assert!(matches!(refused("nextest", ""), RegistryError::UnsupportedRunner { expected: "kani", .. }));
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
    assert!(reason.contains("cargo install --locked kani-verifier --version 0.68.0"), "{reason}");
    assert!(reason.contains("cargo kani setup"), "{reason}");
}

// ---------------------------------------------------------------------------
// Real runs
// ---------------------------------------------------------------------------

/// Whether the pinned toolchain resolves here; a note when it does not.
fn kani_present() -> bool {
    match resolve(adapter().tool_env()) {
        Resolution::Ready(_) => true,
        Resolution::Missing { reason, .. } => {
            eprintln!("skipped: {reason}");
            false
        }
    }
}

fn check(b: &Binding) -> Check {
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
