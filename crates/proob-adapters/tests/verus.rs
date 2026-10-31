//! The Verus adapter against `tests/fixtures/verus-crate`: discovery and
//! binding sets (pure), the argv it builds (pure), the parser on the captured
//! verifier outputs, and real runs through `cargo verus`. Real runs need the
//! toolchain (`PROOB_VERUS_DIR`, `PATH`, or tools/get-verus.sh's cache); when
//! none is found they say so and return, since the plain-cargo CI job has no
//! Verus. Tests that edit files work on a copy; the copy shares the fixture's
//! build directory through `CARGO_TARGET_DIR`, so `vstd` is built once.
use proob_adapters::argv::Checked;
use proob_adapters::rust::RustAdapter;
use proob_adapters::verus::check::{Judged, judge, partial_dir, plan_clean, plan_crate, plan_item};
use proob_adapters::verus::discover::verified_packages;
use proob_adapters::verus::mutate::{OPERATOR_SET, Operator, mutants};
use proob_adapters::verus::scan::{Bound, scan};
use proob_adapters::verus::score::{Fate, fate};
use proob_adapters::verus::tools::{Resolution, Toolchain, resolve};
use proob_adapters::verus::{VerusAdapter, parse_check_ref};
use proob_adapters::{
    Adapter, AdapterError, Binding, Budget, CheckOutcome, ProofMeasure, ScoreOutcome, Seed, TokenProblem,
};
use proob_core::types::Kind;
use proob_facts::ulid;
use proob_spawn::Output;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const A: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XVA";
const B: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XVB";
/// A fresh id the edit tests put where they need a token.
const C: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XVC";
/// `classify`: a body that is one `match` of constructors.
const E: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XVE";
/// `ordered`: braces inside the `ensures`.
const F: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XVF";

fn id(text: &str) -> u128 {
    ulid::decode(text).expect("ulid")
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/verus-crate")
        .canonicalize()
        .expect("fixture exists")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

fn captures() -> PathBuf {
    fixture().join("captures")
}

/// Every copy builds in the fixture's own target directory.
fn adapter() -> VerusAdapter {
    VerusAdapter::at(".")
        .with_tool_env(vec![(OsString::from("CARGO_TARGET_DIR"), fixture().join("target").into_os_string())])
}

fn discover(repo: &Path) -> Vec<Binding> {
    adapter().discover(repo).expect("discovery succeeds")
}

fn binding(repo: &Path, id_text: &str) -> Binding {
    discover(repo).into_iter().find(|b| b.id == id(id_text)).unwrap_or_else(|| panic!("{id_text} is bound"))
}

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

fn copy_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    proob_adapters::verus::score::copy_tree(&fixture(), dir.path()).expect("copy");
    dir
}

fn edit(root: &Path, rel: &str, f: impl FnOnce(String) -> String) {
    let path = root.join(rel);
    let text = std::fs::read_to_string(&path).expect("read");
    std::fs::write(&path, f(text)).expect("write");
}

/// The toolchain, or a note and `None` when this machine has none.
fn toolchain() -> Option<Toolchain> {
    match resolve(adapter().tool_env()) {
        Resolution::Ready(tc) => Some(tc),
        Resolution::Missing { reason, .. } => {
            eprintln!("skipped: {reason}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

#[test]
fn discover_finds_the_annotated_items_and_the_crates_binding_set() {
    let found = discover(&fixture());
    let ids: Vec<u128> = found.iter().map(|b| b.id).collect();
    assert_eq!(ids, vec![id(A), id(B), id(E), id(F)], "sorted by token location");
    let a = binding(&fixture(), A);
    assert_eq!(a.check_ref, "clamp clamp");
    assert_eq!(a.kind, Kind::Proof);
    assert_eq!(
        a.files,
        paths(&["Cargo.toml", "clamp/Cargo.toml", "clamp/src/lib.rs", "rust-toolchain.toml"]),
        "the whole crate plus its pins"
    );
    assert_eq!(a.token.to_string(), "clamp/src/lib.rs:10");
    let b = binding(&fixture(), B);
    assert_eq!(b.check_ref, "clamp smaller");
    assert_eq!(b.files, a.files, "two items in one verified crate share its binding set");
    assert_eq!(b.binding_hash, a.binding_hash);
    assert_eq!(b.token.to_string(), "clamp/src/lib.rs:30");
    assert_eq!(binding(&fixture(), E).check_ref, "clamp classify");
    assert_eq!(binding(&fixture(), F).check_ref, "clamp ordered");
}

#[test]
fn changing_the_toolchain_pin_or_the_vstd_pin_changes_the_binding_hash() {
    let copy = copy_fixture();
    let before = binding(copy.path(), A).binding_hash;
    edit(copy.path(), "rust-toolchain.toml", |t| t.replace("1.99.1", "1.99.2"));
    let after_toolchain = binding(copy.path(), A).binding_hash;
    assert_ne!(after_toolchain, before, "rust-toolchain.toml is a member");
    edit(copy.path(), "Cargo.toml", |t| t.replace("2026-10-25-0158", "2026-10-18-0147"));
    assert_ne!(binding(copy.path(), A).binding_hash, after_toolchain, "the workspace root pins vstd");
    // A comment in the source is content too: the crate is the unit of approval.
    edit(copy.path(), "clamp/src/lib.rs", |t| format!("{t}// a trailing comment\n"));
    let after_source = binding(copy.path(), A).binding_hash;
    assert_ne!(after_source, after_toolchain);
    assert_eq!(binding(copy.path(), B).binding_hash, after_source, "and B shares it");
}

#[test]
fn a_crate_without_verify_true_yields_no_binding_and_an_uncovered_token() {
    let copy = copy_fixture();
    edit(copy.path(), "clamp/Cargo.toml", |t| t.replace("verify = true", "verify = false"));
    assert!(discover(copy.path()).is_empty(), "nothing is verified, so nothing binds");
    assert!(verified_packages(&adapter(), copy.path()).expect("packages").is_empty());
    let uncovered = adapter().uncovered(copy.path()).expect("uncovered");
    let ids: Vec<u128> = uncovered.iter().map(|u| u.id).collect();
    assert_eq!(ids, vec![id(A), id(B), id(E), id(F)]);
    assert_eq!(uncovered[0].token.to_string(), "clamp/src/lib.rs:10");
    assert!(uncovered[0].reason.contains("verify = true"), "{}", uncovered[0].reason);
    assert!(adapter().uncovered(&fixture()).expect("uncovered").is_empty(), "the real fixture covers both");
}

#[test]
fn malformed_dangling_outside_and_duplicate_tokens_are_rejected_with_file_and_line() {
    let versioned = copy_fixture();
    edit(versioned.path(), "clamp/src/lib.rs", |t| t.replace(&format!("// proob:{A}"), &format!("// proob:{A}:v2")));
    match adapter().discover(versioned.path()) {
        Err(AdapterError::Token { at, problem: TokenProblem::NotUlid { text, .. } }) => {
            assert_eq!(at.to_string(), "clamp/src/lib.rs:10");
            assert_eq!(text, format!("{A}:v2"));
        }
        other => panic!("{other:?}"),
    }

    let dangling = copy_fixture();
    edit(dangling.path(), "clamp/src/lib.rs", |t| {
        t.replace("} // verus!", &format!("// proob:{C}\npub struct Nothing;\n}} // verus!"))
    });
    assert!(matches!(
        adapter().discover(dangling.path()),
        Err(AdapterError::Token { problem: TokenProblem::NoVerusItem, .. })
    ));

    let outside = copy_fixture();
    edit(outside.path(), "clamp/src/lib.rs", |t| format!("{t}\n// proob:{C}\npub fn plain() {{}}\n"));
    match adapter().discover(outside.path()) {
        Err(AdapterError::Token { problem: TokenProblem::OutsideVerus { function }, .. }) => {
            assert_eq!(function, "plain")
        }
        other => panic!("{other:?}"),
    }

    let duplicate = copy_fixture();
    edit(duplicate.path(), "clamp/src/lib.rs", |t| t.replace(&format!("// proob:{B}"), &format!("// proob:{A}")));
    match adapter().discover(duplicate.path()) {
        Err(AdapterError::Duplicate { id: dup, first, second }) => {
            assert_eq!(dup, A);
            assert_eq!(first.to_string(), "clamp/src/lib.rs:10");
            assert_eq!(second.to_string(), "clamp/src/lib.rs:30");
        }
        other => panic!("{other:?}"),
    }
}

/// proob's own declaration (U14): every proof-backed property binds an exec
/// item of `proob-core`, and the binding set is the crate plus its trusted
/// base and the toolchain pins; the test-backed properties bind integration
/// tests of the unverified crates through the Rust adapter.
#[test]
fn proob_core_is_a_verified_crate_whose_items_carry_proobs_own_bindings() {
    let root = repo_root();
    let names: Vec<String> =
        verified_packages(&VerusAdapter::at("."), &root).expect("packages").into_iter().map(|p| p.name).collect();
    assert!(names.contains(&"proob-core".to_string()) && names.contains(&"proob-trusted".to_string()), "{names:?}");
    let proofs = VerusAdapter::at(".").discover(&root).expect("discovery");
    let refs: Vec<&str> = proofs.iter().map(|b| b.check_ref.as_str()).collect();
    for expected in [
        "proob-core verdict::verdict",
        "proob-core derive::derive",
        "proob-core eligibility::eligibility",
        "proob-core scope_hash::scope_hash",
        "proob-core policy::policy",
        "proob-core evidence_accept::accept_run",
    ] {
        assert!(refs.contains(&expected), "{expected} is bound: {refs:?}");
    }
    assert!(proofs.iter().all(|b| b.kind == Kind::Proof && b.check_ref.starts_with("proob-core ")), "{refs:?}");
    let files = &proofs[0].files;
    for member in [
        "Cargo.toml",
        "rust-toolchain.toml",
        "crates/proob-core/Cargo.toml",
        "crates/proob-core/src/verdict.rs",
        "crates/proob-trusted/Cargo.toml",
        "crates/proob-trusted/src/lib.rs",
    ] {
        assert!(files.contains(&PathBuf::from(member)), "{member} is in the binding set: {files:?}");
    }
    assert!(proofs.iter().all(|b| b.binding_hash == proofs[0].binding_hash), "one verified crate, one binding set");
    // The Rust adapter leaves the verified crates' lib roots alone and binds
    // the test-backed properties in the other crates' integration tests. The
    // Verus adapter reports those same tokens as uncovered (a crate it does
    // not verify), never as errors; the CLI keeps the Rust binding.
    let rust = RustAdapter::at(".").discover(&root).expect("rust discovery");
    let rust_refs: Vec<&str> = rust.iter().map(|b| b.check_ref.as_str()).collect();
    let uncovered = VerusAdapter::at(".").uncovered(&root).expect("uncovered");
    assert!(rust.iter().all(|b| uncovered.iter().any(|u| u.id == b.id && u.token == b.token)), "{uncovered:?}");
    assert!(rust_refs.iter().any(|r| r.starts_with("proob-facts::roundtrip ")), "{rust_refs:?}");
    assert!(rust_refs.iter().any(|r| r.starts_with("proob-effects::cache ")), "{rust_refs:?}");
    assert!(
        rust.iter().all(|b| b.kind == Kind::Test && !b.token.path.starts_with("crates/proob-core/src")),
        "{rust_refs:?}"
    );
}

// ---------------------------------------------------------------------------
// Argv: pure plans
// ---------------------------------------------------------------------------

fn strs(args: &[OsString]) -> Vec<&str> {
    args.iter().filter_map(|a| a.to_str()).collect()
}

fn tc() -> Toolchain {
    Toolchain { dir: PathBuf::from("/opt/verus"), versions: Vec::new() }
}

#[test]
fn the_verify_argv_forwards_no_cheating_after_the_separator() {
    let r = parse_check_ref("clamp clamp").expect("check ref");
    let command = plan_crate(&tc(), &fixture(), Path::new("/t/proob-verus"), &r, 10);
    let args = strs(command.args());
    assert_eq!(command.program(), Path::new("/opt/verus/cargo-verus"));
    assert_eq!(args[..4], ["verus", "verify", "--locked", "-p"]);
    assert_eq!(args[4], "clamp");
    assert!(args.windows(2).any(|w| w == ["--fwd-verus-args-to", "roots"]));
    assert!(args.windows(2).any(|w| w == ["--target-dir", "/t/proob-verus"]));
    let sep = command.separator_index().expect("a separator");
    assert_eq!(&args[sep..], ["--", "--no-cheating", "--rlimit", "10", "--output-json"]);
}

#[test]
fn the_attribution_argv_names_the_module_and_the_function_after_the_separator() {
    let nested = parse_check_ref("proob-core scope_hash::combine_sorted").expect("check ref");
    let command = plan_item(&tc(), &fixture(), Path::new("/t/proob-verus"), &nested, 10, Duration::from_secs(1));
    let args = strs(command.args());
    assert_eq!(args[..2], ["verus", "focus"]);
    let sep = command.separator_index().expect("a separator");
    assert_eq!(
        &args[sep..],
        [
            "--",
            "--no-cheating",
            "--rlimit",
            "10",
            "--output-json",
            "--verify-only-module",
            "scope_hash",
            "--verify-function",
            "combine_sorted"
        ]
    );
    let root = parse_check_ref("clamp clamp").expect("check ref");
    let command = plan_item(&tc(), &fixture(), Path::new("/t/proob-verus"), &root, 10, Duration::from_secs(1));
    let args = strs(command.args());
    let sep = command.separator_index().expect("a separator");
    assert_eq!(
        &args[sep..],
        ["--", "--no-cheating", "--rlimit", "10", "--output-json", "--verify-root", "--verify-function", "clamp"]
    );
    let clean = plan_clean(Path::new("cargo"), &fixture(), &partial_dir(Path::new("/t/proob-verus")), &root);
    assert_eq!(
        strs(clean.args()),
        ["clean", "--locked", "-p", "clamp", "--target-dir", "/t/proob-verus/verus-partial"]
    );
}

#[test]
fn property_derived_values_pass_the_grammars_first() {
    assert!(parse_check_ref("clamp clamp*").is_err());
    assert!(parse_check_ref("clamp --verify-root").is_err());
    assert!(parse_check_ref("-p clamp").is_err());
    assert!(Checked::identifier("clamp").is_ok());
    let mut b = binding(&fixture(), A);
    b.check_ref = "clamp clamp*".to_string();
    assert!(matches!(adapter().check(&fixture(), &b, Budget(1), Seed(1)), Err(AdapterError::Binding { .. })));
}

// ---------------------------------------------------------------------------
// Mutants (pure) and the captured verifier outputs
// ---------------------------------------------------------------------------

fn described(text: &str, line: usize) -> Vec<String> {
    mutants(text, line).iter().map(|m| m.describe()).collect()
}

#[test]
fn the_fixture_items_have_the_expected_mutants() {
    let text = std::fs::read_to_string(fixture().join("clamp/src/lib.rs")).expect("read");
    assert_eq!(
        described(&text, 11),
        vec![
            "20:10: `<` -> `>`",
            "20:10: `<` -> `<=`",
            "22:17: `>` -> `<`",
            "22:17: `>` -> `>=`",
            "23:9: then-branch and else-branch swapped"
        ],
        "clamp: the inner `if … else` swaps; the outer one's else is an `if`"
    );
    assert_eq!(
        described(&text, 31),
        vec!["35:10: `<` -> `>`", "35:10: `<` -> `<=`", "36:9: then-branch and else-branch swapped"]
    );
    assert_eq!(
        described(&text, 55),
        vec!["62:26: arm 1 and arm 2 swapped", "63:14: arm 2 and arm 3 swapped"],
        "classify: v1 found nothing in a match of constructors"
    );
    assert_eq!(
        described(&text, 80),
        vec!["86:10: `<` -> `>`", "86:10: `<` -> `<=`", "87:9: then-branch and else-branch swapped"],
        "ordered: v1 took the `ensures`' struct pattern for the body"
    );
    assert_eq!(OPERATOR_SET, "mutants-under-proof-v2");
}

/// U15's first operator set found no mutant for six of the nine bound items
/// of proob-core: spec clauses whose braces were read as the body (`derive`,
/// `eligibility`, `verdict`), and dispatch bodies without an operator
/// (`verdict`, `verdict_scoped`, `policy`, `scope_hash`). Under v2 every
/// item has at least one, so every proof-backed property can reach the kill
/// floor, and the flagship `verdict` has exactly the one-way arm copy its
/// differently-bound arms allow. Pure: the real sources, no toolchain.
#[test]
fn every_bound_item_of_proob_core_has_mutants_under_v2() {
    let root = repo_root();
    let proofs = VerusAdapter::at(".").discover(&root).expect("discovery");
    assert_eq!(proofs.len(), 9, "{proofs:?}");
    for b in &proofs {
        let text = std::fs::read_to_string(root.join(&b.token.path)).expect("source");
        let token = scan(&text).tokens.into_iter().find(|t| t.line == b.token.line).expect("the token");
        let Bound::Item { item, .. } = token.bound else { panic!("{:?}", token.bound) };
        let all = mutants(&text, item.line);
        eprintln!("{}: {} mutants", b.check_ref, all.len());
        for m in &all {
            eprintln!("    {:?} {}", m.operator, m.describe());
        }
        assert!(!all.is_empty(), "{} still has no mutant", b.check_ref);
        if b.check_ref == "proob-core verdict::verdict" {
            let kinds: Vec<(Operator, String)> = all.iter().map(|m| (m.operator, m.change.clone())).collect();
            assert_eq!(kinds, vec![(Operator::ArmSwap, "arm 2 takes arm 1's result".to_string())]);
        }
    }
}

fn captured(stem: &str) -> Output {
    let read = |suffix: &str| std::fs::read(captures().join(format!("{stem}.{suffix}"))).unwrap_or_default();
    let code =
        std::fs::read_to_string(captures().join(format!("{stem}.exit"))).ok().and_then(|t| t.trim().parse().ok());
    Output { code, stdout: read("stdout.txt"), stderr: read("stderr.txt") }
}

#[test]
fn captured_outputs_judge_as_pass_proof_failure_and_error() {
    assert_eq!(judge(&captured("pass")), Judged::Verified);
    match judge(&captured("proof-failed")) {
        Judged::Rejected(diagnostics) => assert!(
            diagnostics.contains("postcondition not satisfied") && diagnostics.contains("clamp/src/lib.rs:"),
            "{diagnostics}"
        ),
        other => panic!("{other:?}"),
    }
    match judge(&captured("error")) {
        Judged::Aborted(text) => {
            assert!(text.contains("mismatched types"), "a rustc error still comes with a JSON object: {text}")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(judge(&captured("item")), Judged::Verified, "the attribution run verifies the one item");
    assert_eq!(fate(Ok(&captured("pass"))), Ok(Fate::Survived));
    assert_eq!(fate(Ok(&captured("proof-failed"))), Ok(Fate::Killed));
    assert_eq!(fate(Ok(&captured("error"))), Ok(Fate::Unviable), "a mutant rustc rejects is unviable, not killed");
}

// ---------------------------------------------------------------------------
// Real runs
// ---------------------------------------------------------------------------

#[test]
fn the_annotated_item_verifies_and_the_versions_are_recorded() {
    if toolchain().is_none() {
        return;
    }
    let started = Instant::now();
    let out = adapter().check(&fixture(), &binding(&fixture(), A), Budget(1), Seed(1)).expect("check runs");
    eprintln!("check(clamp) took {:?}", started.elapsed());
    assert_eq!(
        out.outcome,
        CheckOutcome::Verified { measure: ProofMeasure::Verus { rlimit: 10 } },
        "{:?}",
        out.outcome
    );
    let names: Vec<&str> = out.tool_versions.iter().map(|(k, _)| k.as_str()).collect();
    assert!(names.contains(&"verus") && names.contains(&"z3") && names.contains(&"cargo"), "{names:?}");
    assert!(out.tool_versions.iter().any(|(k, v)| k == "verus" && v.starts_with("0.20")), "{:?}", out.tool_versions);
    assert!(out.tool_versions.iter().any(|(k, v)| k == "verus-commit" && v.len() == 40), "{:?}", out.tool_versions);
}

/// Nothing changes between two checks, so cargo would replay the first build
/// and the verifier would print no JSON; the adapter's clean before each run
/// is what makes the second verdict real. Cargo refuses that clean in a
/// build directory it did not create unless the directory carries its
/// `CACHEDIR.TAG`, which the adapter writes when it makes the directory.
#[test]
fn a_second_check_of_an_unchanged_crate_still_reports_a_verdict() {
    if toolchain().is_none() {
        return;
    }
    let copy = copy_fixture();
    let target = tempfile::tempdir().expect("tempdir");
    let adapter = VerusAdapter::at(".")
        .with_tool_env(vec![(OsString::from("CARGO_TARGET_DIR"), target.path().as_os_str().to_os_string())]);
    let b = adapter.discover(copy.path()).expect("discovery").into_iter().find(|b| b.id == id(A)).expect("bound");
    let first = adapter.check(copy.path(), &b, Budget(1), Seed(1)).expect("check runs");
    assert_eq!(
        first.outcome,
        CheckOutcome::Verified { measure: ProofMeasure::Verus { rlimit: 10 } },
        "{:?}",
        first.outcome
    );
    assert!(
        target
            .path()
            .join(proob_adapters::verus::check::TARGET_SUBDIR)
            .join(proob_adapters::verus::check::CACHEDIR_TAG)
            .is_file()
    );
    let second = adapter.check(copy.path(), &b, Budget(1), Seed(1)).expect("check runs");
    assert_eq!(
        second.outcome,
        CheckOutcome::Verified { measure: ProofMeasure::Verus { rlimit: 10 } },
        "a replayed build reports no verdict: {:?}",
        second.outcome
    );
}

#[test]
fn a_bug_in_the_implementation_fails_the_proof_with_diagnostics() {
    if toolchain().is_none() {
        return;
    }
    let copy = copy_fixture();
    edit(copy.path(), "clamp/src/lib.rs", |t| {
        t.replace("    if x < lo {\n        lo\n", "    if x < lo {\n        hi\n")
    });
    let out = adapter().check(copy.path(), &binding(copy.path(), A), Budget(1), Seed(1)).expect("check runs");
    match out.outcome {
        CheckOutcome::ProofFailed { diagnostics } => {
            assert!(!diagnostics.is_empty());
            assert!(diagnostics.contains("postcondition not satisfied"), "{diagnostics}");
            assert!(diagnostics.contains("clamp/src/lib.rs:"), "names the item's file: {diagnostics}");
        }
        other => panic!("{other:?}"),
    }
    // The weak item is still fine: a failed crate fails every property in it,
    // because any proof in the crate may rest on the broken one.
    let weak = adapter().check(copy.path(), &binding(copy.path(), B), Budget(1), Seed(1)).expect("check runs");
    assert!(matches!(weak.outcome, CheckOutcome::ProofFailed { .. }), "{:?}", weak.outcome);
}

#[test]
fn a_type_error_is_errored_not_a_failed_proof() {
    if toolchain().is_none() {
        return;
    }
    let copy = copy_fixture();
    edit(copy.path(), "clamp/src/lib.rs", |t| {
        t.replace("    if a < b {\n        a\n", "    let _bad: u8 = a;\n    if a < b {\n        a\n")
    });
    let out = adapter().check(copy.path(), &binding(copy.path(), B), Budget(1), Seed(1)).expect("check runs");
    match out.outcome {
        CheckOutcome::Errored { reason } => assert!(reason.contains("mismatched types"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

/// Mutation-under-proof on both items: the weak specification kills nothing,
/// the meaningful one kills what returns the wrong bound. Around a minute on
/// a laptop with a warm build directory (two baselines and eight mutants).
#[test]
fn the_weak_spec_scores_zero_and_the_meaningful_one_kills() {
    if toolchain().is_none() {
        return;
    }
    let started = Instant::now();
    let weak = adapter().score(&fixture(), &binding(&fixture(), B), &paths(&["clamp/src/lib.rs"])).expect("score runs");
    eprintln!("score(smaller) took {:?}", started.elapsed());
    assert_eq!(
        weak.outcome,
        ScoreOutcome::Scored { killed: 0, survived: 3, timeout: 0, unviable: 0, unsampled: 0 },
        "{:?}",
        weak.outcome
    );
    assert!(
        weak.tool_versions.iter().any(|(k, v)| k == "mutants-under-proof" && v == OPERATOR_SET),
        "{:?}",
        weak.tool_versions
    );

    let started = Instant::now();
    let strong =
        adapter().score(&fixture(), &binding(&fixture(), A), &paths(&["clamp/src/lib.rs"])).expect("score runs");
    eprintln!("score(clamp) took {:?}", started.elapsed());
    match strong.outcome {
        ScoreOutcome::Scored { killed, survived, timeout, unviable, unsampled } => {
            assert!(killed >= 1, "swapping a comparison returns the wrong bound: {killed} killed, {survived} survived");
            assert_eq!(killed + survived, 5, "five mutants of clamp");
            assert_eq!((timeout, unviable, unsampled), (0, 0, 0));
        }
        other => panic!("{other:?}"),
    }
}

/// The two shapes v1 scored `NoMutants`: the `match` of constructors is
/// killed by both arm swaps, and the item whose `ensures` braces hid its body
/// is killed by the comparison flip and the branch swap (the `<` → `<=`
/// mutant is equivalent when the inputs are equal and survives). Two
/// baselines and five mutants.
#[test]
fn the_dispatch_item_and_the_braced_spec_item_score_under_v2() {
    if toolchain().is_none() {
        return;
    }
    let started = Instant::now();
    let classify =
        adapter().score(&fixture(), &binding(&fixture(), E), &paths(&["clamp/src/lib.rs"])).expect("score runs");
    eprintln!("score(classify) took {:?}", started.elapsed());
    assert_eq!(
        classify.outcome,
        ScoreOutcome::Scored { killed: 2, survived: 0, timeout: 0, unviable: 0, unsampled: 0 },
        "{:?}",
        classify.outcome
    );

    let started = Instant::now();
    let ordered =
        adapter().score(&fixture(), &binding(&fixture(), F), &paths(&["clamp/src/lib.rs"])).expect("score runs");
    eprintln!("score(ordered) took {:?}", started.elapsed());
    match ordered.outcome {
        ScoreOutcome::Scored { killed, survived, timeout, unviable, unsampled } => {
            assert!(killed >= 1, "{killed} killed, {survived} survived");
            assert_eq!(killed + survived, 3, "three mutants of ordered");
            assert_eq!((timeout, unviable, unsampled), (0, 0, 0));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_mutant_cap_leaves_the_rest_unsampled() {
    if toolchain().is_none() {
        return;
    }
    let capped = adapter().with_max_mutants(1);
    let out = capped.score(&fixture(), &binding(&fixture(), A), &paths(&["clamp/src/lib.rs"])).expect("score runs");
    match out.outcome {
        ScoreOutcome::Scored { killed, survived, unsampled, .. } => {
            assert_eq!(killed + survived, 1);
            assert_eq!(unsampled, 4);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_missing_toolchain_yields_errored_with_the_hint() {
    let home = tempfile::tempdir().expect("tempdir");
    let env = vec![
        (OsString::from("PROOB_VERUS_DIR"), OsString::new()),
        (OsString::from("PATH"), home.path().join("nothing").into_os_string()),
        (OsString::from("HOME"), home.path().as_os_str().to_os_string()),
    ];
    let none = VerusAdapter::at(".").with_tool_env(env);
    let out = none.check(&fixture(), &binding(&fixture(), A), Budget(1), Seed(1)).expect("check returns a result");
    assert!(
        matches!(&out.outcome, CheckOutcome::Errored { reason } if reason.contains("Verus toolchain was not found")),
        "{:?}",
        out.outcome
    );
    let out = none.score(&fixture(), &binding(&fixture(), A), &[]).expect("score returns a result");
    assert!(
        matches!(&out.outcome, ScoreOutcome::Errored { reason } if reason.contains("Verus toolchain was not found")),
        "{:?}",
        out.outcome
    );
}
