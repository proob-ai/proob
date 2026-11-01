//! The Rust adapter against `tests/fixtures/rust-repo`: discovery and binding
//! sets (pure), the argv it builds (pure), and real runs through nextest and
//! cargo-mutants. Runs that build use the fixture in place and share its
//! `target/`; tests that edit files work on a copy.
use proob_adapters::argv::Checked;
use proob_adapters::mutation::Limits;
use proob_adapters::rust::{Features, RustAdapter, check, parse_check_ref, score};
use proob_adapters::{Adapter, AdapterError, Binding, Budget, CheckOutcome, ScoreOutcome, Seed, TokenProblem, Witness};
use proob_facts::ulid;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Instant;

const A: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHA";
const B: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHB";
const C: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHC";
const D: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHD";

fn id(text: &str) -> u128 {
    ulid::decode(text).expect("ulid")
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rust-repo").canonicalize().expect("fixture exists")
}

fn adapter() -> RustAdapter {
    RustAdapter::at(".")
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

/// A copy of the fixture without its build output.
fn copy_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    copy_tree(&fixture(), dir.path());
    dir
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("read_dir").filter_map(Result::ok) {
        let name = entry.file_name();
        if name == "target" {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            std::fs::create_dir_all(&dst).expect("mkdir");
            copy_tree(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).expect("copy");
        }
    }
}

fn edit(root: &Path, rel: &str, f: impl FnOnce(String) -> String) {
    let path = root.join(rel);
    let text = std::fs::read_to_string(&path).expect("read");
    std::fs::write(&path, f(text)).expect("write");
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

#[test]
fn discover_finds_the_bound_tests_and_their_binding_sets() {
    let found = discover(&fixture());
    let ids: Vec<u128> = found.iter().map(|b| b.id).collect();
    assert_eq!(ids, vec![id(A), id(B), id(D)], "sorted by token location; C has no token");
    let a = binding(&fixture(), A);
    assert_eq!(a.check_ref, "rust-repo::props sorted_preserves_length");
    assert_eq!(a.files, paths(&["tests/common/mod.rs", "tests/props.rs"]));
    assert_eq!(a.token.to_string(), "tests/props.rs:12");
    let b = binding(&fixture(), B);
    assert_eq!(b.check_ref, "rust-repo::props clamp_stays_within_bounds");
    assert_eq!(b.files, a.files, "two tests in one crate share the crate's binding set");
    assert_eq!(b.binding_hash, a.binding_hash);
    let d = binding(&fixture(), D);
    assert_eq!(d.check_ref, "rust-repo::vacuous evens_are_counted_vacuously");
    assert_eq!(d.files, paths(&["tests/common/mod.rs", "tests/vacuous.rs"]));
    assert_ne!(d.binding_hash, a.binding_hash);
    assert!(found.iter().all(|b| b.kind == proob_core::types::Kind::Test));
}

#[test]
fn changing_the_shared_helper_changes_the_binding_hash_and_an_unrelated_test_does_not() {
    let copy = copy_fixture();
    let before_a = binding(copy.path(), A).binding_hash;
    let before_d = binding(copy.path(), D).binding_hash;

    edit(copy.path(), "tests/unrelated.rs", |t| format!("{t}\n// an unrelated edit\n"));
    assert_eq!(binding(copy.path(), A).binding_hash, before_a, "unrelated.rs is in no binding set");
    assert_eq!(binding(copy.path(), D).binding_hash, before_d);

    edit(copy.path(), "tests/common/mod.rs", |t| t.replace("0u32..100", "0u32..101"));
    let after_a = binding(copy.path(), A).binding_hash;
    assert_ne!(after_a, before_a, "the shared strategy helper is in A's binding set");
    assert_ne!(binding(copy.path(), D).binding_hash, before_d, "and in D's");

    // Trailing whitespace is normalised away: no hash change.
    edit(copy.path(), "tests/props.rs", |t| t.replace("mod common;", "mod common;   "));
    assert_eq!(binding(copy.path(), A).binding_hash, after_a);
}

#[test]
fn a_nextest_config_file_joins_every_binding_set() {
    let copy = copy_fixture();
    std::fs::create_dir_all(copy.path().join(".config")).expect("mkdir");
    std::fs::write(copy.path().join(".config/nextest.toml"), "[profile.default]\nretries = 0\n").expect("write");
    let a = binding(copy.path(), A);
    assert_eq!(a.files, paths(&[".config/nextest.toml", "tests/common/mod.rs", "tests/props.rs"]));
}

#[test]
fn a_versioned_token_is_rejected_with_file_and_line() {
    let copy = copy_fixture();
    edit(copy.path(), "tests/props.rs", |t| t.replace(&format!("// proob:{A}"), &format!("// proob:{A}:v2")));
    match adapter().discover(copy.path()) {
        Err(AdapterError::Token { at, problem: TokenProblem::NotUlid { text, .. } }) => {
            assert_eq!(at.to_string(), "tests/props.rs:12");
            assert_eq!(text, format!("{A}:v2"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_dangling_token_a_non_test_and_a_duplicate_are_rejected() {
    let dangling = copy_fixture();
    edit(dangling.path(), "tests/vacuous.rs", |t| t.replace("proptest! {", &format!("// proob:{C}\nproptest! {{")));
    assert!(matches!(
        adapter().discover(dangling.path()),
        Err(AdapterError::Token { problem: TokenProblem::NoFunction, .. })
    ));

    let non_test = copy_fixture();
    edit(non_test.path(), "tests/unrelated.rs", |t| {
        t.replace(
            "#[test]\nfn count_evens_is_exact",
            &format!("// proob:{C}\nfn helper() {{}}\n#[test]\nfn count_evens_is_exact"),
        )
    });
    match adapter().discover(non_test.path()) {
        Err(AdapterError::Token { problem: TokenProblem::NotATest { function }, .. }) => assert_eq!(function, "helper"),
        other => panic!("{other:?}"),
    }

    let duplicate = copy_fixture();
    edit(duplicate.path(), "tests/vacuous.rs", |t| t.replace(&format!("// proob:{D}"), &format!("// proob:{A}")));
    match adapter().discover(duplicate.path()) {
        Err(AdapterError::Duplicate { id: dup, first, second }) => {
            assert_eq!(dup, A);
            assert_eq!(first.to_string(), "tests/props.rs:12");
            assert_eq!(second.to_string(), "tests/vacuous.rs:9");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_unit_test_in_src_binds_its_file_and_its_cfg_test_modules() {
    let copy = copy_fixture();
    std::fs::create_dir_all(copy.path().join("src/parity")).expect("mkdir");
    std::fs::write(copy.path().join("src/parity/tests.rs"), "use super::count_evens;\n\n// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHC\n#[test]\nfn unit_bound() { assert_eq!(count_evens(&[2]), 1); }\n").expect("write");
    edit(copy.path(), "src/parity.rs", |t| format!("{t}\n#[cfg(test)]\nmod tests;\n"));
    let c = binding(copy.path(), C);
    assert_eq!(c.check_ref, "rust-repo parity::tests::unit_bound");
    // The parent module is the code under test (the scope), not the check.
    assert_eq!(c.files, paths(&["src/parity/tests.rs"]));
}

fn kani_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/kani-crate")
        .canonicalize()
        .expect("fixture exists")
}

const KANI_REASON: &str = "a Kani harness; add a `kani` adapter (`proob init --kani`)";

#[test]
fn kani_harness_tokens_are_skipped_and_reported_uncovered_with_the_fix() {
    let repo = kani_fixture();
    let found = discover(&repo);
    assert_eq!(found.iter().map(|b| b.id).collect::<Vec<_>>(), vec![id("01K8V5NQ4A0F3R7Y2M9D6P1XKA")]);
    assert_eq!(found[0].check_ref, "kani-crate::props clamp_stays_within_bounds");
    let uncovered: Vec<(String, String, &str)> = adapter()
        .uncovered(&repo)
        .expect("uncovered succeeds")
        .iter()
        .map(|u| (ulid::encode(u.id), u.token.to_string(), if u.reason == KANI_REASON { "kani" } else { "other" }))
        .collect();
    assert_eq!(
        uncovered,
        vec![
            ("01K8V5NQ4A0F3R7Y2M9D6P1XKB".to_string(), "src/lib.rs:19".to_string(), "kani"),
            ("01K8V5NQ4A0F3R7Y2M9D6P1XKC".to_string(), "src/lib.rs:28".to_string(), "kani"),
            // `#[test]` and `cfg_attr(kani, kani::proof)`: Kani's, not bound here.
            ("01K8V5NQ4A0F3R7Y2M9D6P1XKD".to_string(), "src/lib.rs:36".to_string(), "kani"),
            ("01K8V5NQ4A0F3R7Y2M9D6P1XKE".to_string(), "src/lib.rs:45".to_string(), "other"),
            ("01K8V5NQ4A0F3R7Y2M9D6P1XKF".to_string(), "src/lib.rs:57".to_string(), "kani"),
            ("01K8V5NQ4A0F3R7Y2M9D6P1XKG".to_string(), "src/lib.rs:64".to_string(), "kani"),
            ("01K8V5NQ4A0F3R7Y2M9D6P1XKH".to_string(), "src/lib.rs:74".to_string(), "kani"),
            ("01K8V5NQ4A0F3R7Y2M9D6P1XKJ".to_string(), "src/lib.rs:87".to_string(), "kani"),
            ("01K8V5NQ4A0F3R7Y2M9D6P1XKK".to_string(), "src/lib.rs:100".to_string(), "kani"),
            ("01K8V5NQ4A0F3R7Y2M9D6P1XKM".to_string(), "src/lib.rs:110".to_string(), "kani"),
        ]
    );
    let contract = adapter().uncovered(&repo).expect("uncovered succeeds").swap_remove(3);
    assert!(contract.reason.contains("Kani contracts are not supported yet"), "{}", contract.reason);
}

#[test]
fn the_rust_fixture_has_nothing_uncovered() {
    assert!(adapter().uncovered(&fixture()).expect("uncovered succeeds").is_empty());
}

// ---------------------------------------------------------------------------
// Argv: pure plans
// ---------------------------------------------------------------------------

fn strs(args: &[OsString]) -> Vec<&str> {
    args.iter().filter_map(|a| a.to_str()).collect()
}

#[test]
fn the_check_argv_selects_exactly_the_bound_test_and_sets_proptests_environment() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let r = parse_check_ref("rust-repo::props sorted_preserves_length").expect("check ref");
    let plan = check::plan(Path::new("cargo"), &fixture(), &r, &Features::default(), tmp.path(), Budget(40), Seed(9))
        .expect("plan");
    let args = strs(plan.command.args());
    assert_eq!(args[..2], ["nextest", "run"]);
    assert!(args.windows(2).any(|w| w == ["--package", "rust-repo"]));
    assert!(args.windows(2).any(|w| w == ["--profile", "@tool:proob:check"]));
    assert!(
        args.windows(2).any(|w| w == ["--filterset", "binary_id(=rust-repo::props) & test(=sorted_preserves_length)"])
    );
    assert!(args.contains(&"--no-fail-fast"));
    let env: Vec<(String, String)> = plan
        .command
        .environment()
        .iter()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .collect();
    assert!(env.contains(&("PROPTEST_RNG_SEED".into(), "9".into())));
    assert!(env.contains(&("PROPTEST_CASES".into(), "40".into())));
    assert!(env.contains(&("PROPTEST_DISABLE_FAILURE_PERSISTENCE".into(), "1".into())));
    let config = std::fs::read_to_string(tmp.path().join("nextest.toml")).expect("config written");
    assert!(config.contains("junit.xml"), "{config}");
}

// proob:01M622RQ7D1NN8MNXJA62ABZNB
#[test]
fn the_score_argv_puts_the_separator_before_the_forwarded_test_filter() {
    let r = parse_check_ref("rust-repo::vacuous evens_are_counted_vacuously").expect("check ref");
    let files = vec![Checked::path(Path::new("src/parity.rs")).expect("path")];
    let chosen = [Checked::exact_regex("src/parity.rs:3:5: replace count_evens -> usize with 0")];
    let command = score::plan(
        Path::new("cargo"),
        &fixture(),
        &r,
        &Features::default(),
        Path::new("/tmp/out"),
        &files,
        &chosen,
        3,
        Seed(1),
        Budget(256),
    );
    let args = strs(command.args());
    let sep = command.separator_index().expect("a separator");
    let filter = args.iter().position(|a| *a == "-E").expect("filter");
    assert!(sep < filter, "property-derived nextest arguments follow `--`: {args:?}");
    assert_eq!(args[filter + 1], "binary_id(=rust-repo::vacuous) & test(=evens_are_counted_vacuously)");
    assert!(args.windows(2).any(|w| w == ["--test-package", "rust-repo"]));
    assert!(args.windows(2).any(|w| w == ["--file", "src/parity.rs"]));
    assert!(args.windows(2).any(|w| w == ["--test-tool", "nextest"]));
    assert!(args.windows(2).any(|w| w == ["--jobs", "3"]));
    let re = args.iter().position(|a| *a == "--re").expect("a sampled mutant");
    assert!(re < sep && args[re + 1] == "^src/parity\\.rs:3:5: replace count_evens \\-> usize with 0$", "{args:?}");
    assert_eq!(args.iter().filter(|a| **a == "--").count(), 1);
    let listing = strs(
        score::list_plan(Path::new("cargo"), &fixture(), &r, &Features::default(), &files).args().to_vec().as_slice(),
    )
    .iter()
    .map(|a| a.to_string())
    .collect::<Vec<_>>();
    assert_eq!(
        listing,
        ["mutants", "--workspace", "--test-package", "rust-repo", "--file", "src/parity.rs", "--list", "--json"],
        "the listing selects as the run does"
    );
}

/// The argv after `cargo` up to the separator, as strings.
fn before_separator(command: &proob_adapters::argv::Command) -> Vec<String> {
    let args = strs(command.args());
    args[..command.separator_index().unwrap_or(args.len())].iter().map(|a| a.to_string()).collect()
}

#[test]
fn cargo_features_go_identically_on_every_cargo_invocation_each_name_after_its_flag() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let r = parse_check_ref("rust-repo::props sorted_preserves_length").expect("check ref");
    let files = vec![Checked::path(Path::new("src/parity.rs")).expect("path")];
    let names = ["sqlite", "serde/std"].map(|n| Checked::feature(n).expect("feature")).to_vec();
    let features = Features { names, all: true, no_default: true };
    let flags = ["--all-features", "--no-default-features", "--features", "sqlite", "--features", "serde/std"]
        .map(String::from);
    let check = check::plan(Path::new("cargo"), &fixture(), &r, &features, tmp.path(), Budget(40), Seed(9))
        .expect("plan")
        .command;
    let listing = score::list_plan(Path::new("cargo"), &fixture(), &r, &features, &files);
    let run = score::plan(
        Path::new("cargo"),
        &fixture(),
        &r,
        &features,
        Path::new("/tmp/out"),
        &files,
        &[],
        2,
        Seed(1),
        Budget(256),
    );
    for (what, command) in [("check", &check), ("listing", &listing), ("run", &run)] {
        let args = before_separator(command);
        let at = args.windows(flags.len()).position(|w| w == flags).unwrap_or_else(|| panic!("{what}: {args:?}"));
        assert_eq!(
            args.iter().filter(|a| a.contains("feature")).count(),
            4,
            "{what}: once each, nowhere else: {args:?}"
        );
        assert!(at >= 2, "{what}: after the subcommand: {args:?}");
    }
    let none = score::list_plan(Path::new("cargo"), &fixture(), &r, &Features::default(), &files);
    assert!(!strs(none.args()).iter().any(|a| a.contains("feature")), "no options, no flags");
}

#[test]
fn cargo_feature_options_are_read_from_config_and_refused_outside_rust() {
    use proob_facts::config::{Config, parse_config};
    let config = |language: &str, runner: &str, tool: &str, options: &str| -> Config {
        let text = format!(
            "schema_version = 1\n[[adapters]]\nroot = \".\"\nlanguage = \"{language}\"\nrunner = \"{runner}\"\nmutation_tool = \"{tool}\"\n{options}\n[policy]\nholds = \"pass\"\nchecked = \"pass\"\nproposed = \"pass\"\nwaived = \"pass\"\nnot_checked_within_grace = \"pass\"\nnot_checked_after_grace = \"warn\"\nre_approve = \"warn\"\nbroken_affected = \"block\"\nbroken_unaffected = \"pass\"\n"
        );
        parse_config(&text).expect("parses")
    };
    let rust = config("rust", "nextest", "cargo-mutants", "features = [\"sqlite\"]\nall_features = true");
    let configured = RustAdapter::configured(
        &rust.adapters[0],
        Checked::path(Path::new(".")).expect("root"),
        proob_adapters::WitnessPolicy::Full,
    )
    .expect("rust takes them");
    assert_eq!(
        configured.features(),
        &Features { names: vec![Checked::feature("sqlite").expect("feature")], all: true, no_default: false }
    );
    for (language, runner, tool) in [("python", "pytest", "mutmut"), ("verus", "cargo-verus", "mutants-under-proof")] {
        for options in ["features = [\"x\"]", "all_features = false", "no_default_features = true"] {
            let err = proob_adapters::adapters_for(&config(language, runner, tool, options))
                .err()
                .unwrap_or_else(|| panic!("{language} {options}: accepted"));
            assert!(
                matches!(err, proob_adapters::RegistryError::UnsupportedOption { option: "features", .. }),
                "{language} {options}: {err}"
            );
        }
    }
}

#[test]
fn a_copy_is_measured_as_the_tree_less_what_cargo_mutants_skips_plus_target_debug() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    assert_eq!(score::copy_bytes(root), None, "no build to measure by");
    let write = |rel: &str, n: usize| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("dir");
        std::fs::write(p, vec![0u8; n]).expect("write");
    };
    write("src/lib.rs", 100);
    write("Cargo.toml", 10);
    write(".git/objects/x", 5000);
    write("mutants.out/log", 7000);
    write("target/release/big", 9000);
    write("target/debug/deps/libx.rlib", 1000);
    assert_eq!(score::copy_bytes(root), Some(100 + 10 + 1000));
}

#[test]
fn a_leading_dash_glob_never_reaches_spawn() {
    assert!(Checked::path(Path::new("-x.rs")).is_err());
    assert!(Checked::path(Path::new("src/[a].rs")).is_err());
    assert!(matches!(score::mutable_files(&adapter(), &paths(&["-x.rs"])), Err(score::Outside::Grammar(..))));
    assert!(matches!(
        score::mutable_files(&RustAdapter::at("backend"), &paths(&["src/a.rs"])),
        Err(score::Outside::Root(_))
    ));
    let only_rust = score::mutable_files(&adapter(), &paths(&["src/a.rs", "README.md"])).expect("ok");
    assert_eq!(only_rust.len(), 1);
    assert!(parse_check_ref("rust-repo::props --all").is_err());
    assert!(parse_check_ref("pkg a b").is_err());
}

// ---------------------------------------------------------------------------
// Real runs
// ---------------------------------------------------------------------------

#[test]
fn passing_property_yields_no_counterexample_with_the_adapters_seed_and_budget() {
    let started = Instant::now();
    let out = adapter().check(&fixture(), &binding(&fixture(), A), Budget(37), Seed(11)).expect("check runs");
    assert_eq!(out.outcome, CheckOutcome::NoCounterexample { budget: Budget(37), replay: Seed(11) });
    let names: Vec<&str> = out.tool_versions.iter().map(|(k, _)| k.as_str()).collect();
    assert!(names.contains(&"cargo") && names.contains(&"cargo-nextest"), "{names:?}");
    assert!(out.tool_versions.iter().any(|(k, v)| k == "proptest" && v.starts_with("1.12")), "{:?}", out.tool_versions);
    eprintln!("check(passing) took {:?}", started.elapsed());
}

#[test]
fn failing_property_yields_a_shrunk_counterexample_that_the_seed_reproduces() {
    let b = binding(&fixture(), B);
    let first = adapter().check(&fixture(), &b, Budget(256), Seed(5)).expect("check runs");
    let CheckOutcome::Counterexample { witness: Witness::Full(text), replay } = &first.outcome else {
        panic!("{:?}", first.outcome)
    };
    assert_eq!(*replay, Seed(5));
    assert!(text.contains("(lo, hi) = (0, 0)"), "shrunk to the smallest bounds: {text}");
    assert!(text.contains("clamp_to("), "carries the reason: {text}");
    let again = adapter().check(&fixture(), &b, Budget(256), Seed(5)).expect("check runs");
    assert_eq!(again.outcome, first.outcome, "the seed replays the same counterexample");
    let other = adapter().check(&fixture(), &b, Budget(256), Seed(6)).expect("check runs");
    assert!(matches!(other.outcome, CheckOutcome::Counterexample { replay: Seed(6), .. }));
}

#[test]
fn a_seed_only_policy_withholds_the_witness() {
    let a = adapter().with_witness(proob_adapters::WitnessPolicy::SeedOnly);
    let out = a.check(&fixture(), &binding(&fixture(), B), Budget(64), Seed(5)).expect("check runs");
    assert!(
        matches!(out.outcome, CheckOutcome::Counterexample { witness: Witness::SeedOnly, replay: Seed(5) }),
        "{:?}",
        out.outcome
    );
}

#[test]
fn a_renamed_bound_test_is_errored_as_not_found() {
    let mut b = binding(&fixture(), A);
    b.check_ref = "rust-repo::props sorted_preserves_length_renamed".to_string();
    let out = adapter().check(&fixture(), &b, Budget(8), Seed(1)).expect("check runs");
    match out.outcome {
        CheckOutcome::Errored { reason } => assert!(reason.contains("bound test not found"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

/// The one mutation run of this suite: around twenty seconds on a laptop
/// (a cold build of the fixture copy, then five mutants of src/parity.rs).
#[test]
fn the_vacuous_test_passes_and_kills_nothing_even_though_an_unrelated_test_would() {
    let d = binding(&fixture(), D);
    let checked = adapter().check(&fixture(), &d, Budget(64), Seed(3)).expect("check runs");
    assert_eq!(checked.outcome, CheckOutcome::NoCounterexample { budget: Budget(64), replay: Seed(3) });
    let started = Instant::now();
    let scored = adapter().score(&fixture(), &d, &paths(&["src/parity.rs"])).expect("score runs");
    eprintln!("score(vacuous) took {:?}", started.elapsed());
    match scored.outcome {
        ScoreOutcome::Scored { killed, survived, timeout, unviable, unsampled: 0 } => {
            assert_eq!(killed, 0, "only the bound test ran, and it cannot fail");
            assert!(survived >= 3, "src/parity.rs has mutants: {survived}");
            assert_eq!((timeout, unviable), (0, 0));
        }
        other => panic!("{other:?}"),
    }
    assert!(scored.tool_versions.iter().any(|(k, _)| k == "cargo-mutants"), "{:?}", scored.tool_versions);
}

/// A second mutation run: two of src/parity.rs's five mutants, by name.
#[test]
fn a_capped_score_runs_exactly_the_sample_and_reports_the_rest_unsampled() {
    let capped = adapter().with_limits(Limits { max_mutants: 2, jobs: Some(1) });
    let scored = capped.score(&fixture(), &binding(&fixture(), D), &paths(&["src/parity.rs"])).expect("score runs");
    match scored.outcome {
        ScoreOutcome::Scored { killed, survived, timeout, unviable, unsampled } => {
            assert_eq!(killed + survived + timeout + unviable, 2, "exactly the sample ran");
            assert_eq!(unsampled, 3, "the rest of the five listed");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_scope_without_rust_sources_has_no_mutants() {
    let out = adapter().score(&fixture(), &binding(&fixture(), D), &paths(&["proob/config.toml"])).expect("score runs");
    assert_eq!(out.outcome, ScoreOutcome::NoMutants);
}

#[test]
fn a_missing_runner_yields_errored_with_the_install_hint() {
    // A PATH holding only cargo, and a CARGO_HOME whose bin/ is empty, so
    // `cargo nextest` resolves to nothing; the fixture needs no build for this.
    let sandbox = tempfile::tempdir().expect("tempdir");
    let bin = sandbox.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir");
    std::fs::create_dir_all(sandbox.path().join("home/bin")).expect("mkdir");
    let real_cargo = std::env::var_os("PATH")
        .and_then(|p| std::env::split_paths(&p).map(|d| d.join("cargo")).find(|c| c.is_file()))
        .expect("cargo on PATH");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_cargo, bin.join("cargo")).expect("symlink");
    #[cfg(not(unix))]
    std::fs::copy(&real_cargo, bin.join("cargo.exe")).expect("copy");
    let env = vec![
        (OsString::from("PATH"), bin.clone().into_os_string()),
        (OsString::from("CARGO_HOME"), sandbox.path().join("home").into_os_string()),
    ];
    let sandboxed = adapter().with_tool_env(env);
    let out = sandboxed.check(&fixture(), &binding(&fixture(), A), Budget(8), Seed(1)).expect("check returns a result");
    match out.outcome {
        CheckOutcome::Errored { reason } => {
            assert!(reason.contains("cargo-nextest is not installed") && reason.contains("cargo install"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
    assert!(out.tool_versions.iter().any(|(k, _)| k == "cargo"), "cargo itself was found: {:?}", out.tool_versions);
    let none = adapter().with_tool_env(vec![(OsString::from("PATH"), sandbox.path().join("empty").into_os_string())]);
    let out = none.check(&fixture(), &binding(&fixture(), A), Budget(8), Seed(1)).expect("check returns a result");
    assert!(matches!(out.outcome, CheckOutcome::Errored { reason } if reason.contains("cargo was not found")));
}
