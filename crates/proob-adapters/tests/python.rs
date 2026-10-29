//! The Python adapter against `tests/fixtures/py-repo`: discovery and binding
//! sets (pure), the argv and mutmut table it builds (pure), the captured
//! mutmut outputs under `tests/fixtures/mutmut-output`, and real runs through
//! pytest and mutmut. Runs use the fixture in place (they write nothing into
//! it); tests that edit files work on a copy.
//!
//! Real runs need `python3` on PATH with the pins in `tools/python-tools.txt`
//! installed, and fail (not skip) without them, as the Rust adapter's tests
//! need cargo-nextest and cargo-mutants. Set up a virtual environment outside
//! the repository and put its `bin` first on PATH:
//!
//! ```text
//! uv venv /somewhere/pyvenv
//! uv pip install --python /somewhere/pyvenv/bin/python -r tools/python-tools.txt
//! PATH=/somewhere/pyvenv/bin:$PATH cargo nextest run -p proob-adapters --test python
//! ```
//!
//! (`python3 -m venv` and `pip install -r tools/python-tools.txt` do the same.)
//! CI installs the pins with actions/setup-python in the `test` job.
use proob_adapters::argv::Checked;
use proob_adapters::mutation::Limits;
use proob_adapters::python::tools::{MUTMUT, Need, Resolution, pinned, resolve};
use proob_adapters::python::{PythonAdapter, check, parse_check_ref, score};
use proob_adapters::rust::score::Outside;
use proob_adapters::{Adapter, AdapterError, Binding, Budget, CheckOutcome, ScoreOutcome, Seed, TokenProblem, Witness};
use proob_facts::parsers::mutmut::{counts, parse_meta};
use proob_facts::ulid;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Instant;

const A: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XPA";
const B: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XPB";
const C: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XPC";
const D: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XPD";
const A_REF: &str = "tests/test_props.py::test_merge_yields_disjoint_sorted_intervals_covering_the_input";

fn id(text: &str) -> u128 {
    ulid::decode(text).expect("ulid")
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/py-repo").canonicalize().expect("fixture exists")
}

fn captures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/mutmut-output")
        .canonicalize()
        .expect("captures exist")
}

fn adapter() -> PythonAdapter {
    PythonAdapter::at(".")
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

/// A copy of the fixture (it has no build output to leave out).
fn copy_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    score::copy_tree(&fixture(), dir.path()).expect("copy");
    dir
}

fn edit(root: &Path, rel: &str, f: impl FnOnce(String) -> String) {
    let path = root.join(rel);
    let text = std::fs::read_to_string(&path).expect("read");
    std::fs::write(&path, f(text)).expect("write");
}

fn strs(args: &[OsString]) -> Vec<&str> {
    args.iter().filter_map(|a| a.to_str()).collect()
}

/// The fixture after a run: nothing written into it.
fn assert_fixture_untouched() {
    for junk in ["mutants", ".hypothesis", ".pytest_cache", "__pycache__", "interval/__pycache__", "tests/__pycache__"]
    {
        assert!(!fixture().join(junk).exists(), "a run wrote {junk} into the fixture");
    }
}

// ---------------------------------------------------------------------------
// The toolchain
// ---------------------------------------------------------------------------

#[test]
fn python_tools_are_installed_at_their_pins() {
    match resolve(&fixture(), &[Need::Pytest, Need::Mutmut], &[]) {
        Resolution::Ready(tools) => {
            let names: Vec<&str> = tools.versions.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(names, vec!["python", "pytest", "hypothesis", "mutmut"], "{:?}", tools.versions);
            assert!(
                tools.versions.iter().any(|(k, v)| k == MUTMUT && Some(v.as_str()) == pinned(MUTMUT)),
                "{:?}",
                tools.versions
            );
        }
        Resolution::Missing { reason, .. } => panic!(
            "the Python adapter's tools are not available: {reason}\n\
             These tests need python3 with the pins in tools/python-tools.txt; see the doc comment at the top of this file for the venv setup."
        ),
    }
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
    assert_eq!(a.check_ref, A_REF);
    assert_eq!(a.files, paths(&["pyproject.toml", "tests/conftest.py", "tests/test_props.py"]));
    assert_eq!(a.token.to_string(), "tests/test_props.py:13");
    let b = binding(&fixture(), B);
    assert_eq!(b.check_ref, "tests/test_props.py::test_bucket_never_goes_negative");
    assert_eq!(b.files, a.files, "two tests in one file share the file's binding set");
    assert_eq!(b.binding_hash, a.binding_hash);
    let d = binding(&fixture(), D);
    assert_eq!(d.check_ref, "tests/test_vacuous.py::test_lists_are_short");
    assert_eq!(d.files, paths(&["pyproject.toml", "tests/conftest.py", "tests/test_vacuous.py"]));
    assert_eq!(d.token.to_string(), "tests/test_vacuous.py:9");
    assert_ne!(d.binding_hash, a.binding_hash);
    assert!(found.iter().all(|b| b.kind == proob_core::types::Kind::Test));
    assert!(adapter().uncovered(&fixture()).expect("uncovered").is_empty());
}

#[test]
fn changing_the_shared_conftest_changes_the_binding_hash_and_an_unrelated_test_does_not() {
    let copy = copy_fixture();
    let before_a = binding(copy.path(), A).binding_hash;
    let before_d = binding(copy.path(), D).binding_hash;

    edit(copy.path(), "tests/test_unrelated.py", |t| format!("{t}\n# an unrelated edit\n"));
    assert_eq!(binding(copy.path(), A).binding_hash, before_a, "test_unrelated.py is in no binding set");
    assert_eq!(binding(copy.path(), D).binding_hash, before_d);

    edit(copy.path(), "tests/conftest.py", |t| t.replace("BOUND = 20", "BOUND = 21"));
    let after_a = binding(copy.path(), A).binding_hash;
    assert_ne!(after_a, before_a, "the shared strategies live in conftest.py, a member of A's binding set");
    assert_ne!(binding(copy.path(), D).binding_hash, before_d, "and of D's");

    // Trailing whitespace is normalised away: no hash change.
    edit(copy.path(), "tests/test_props.py", |t| {
        t.replace("from hypothesis import given", "from hypothesis import given   ")
    });
    assert_eq!(binding(copy.path(), A).binding_hash, after_a);
}

#[test]
fn a_pytest_ini_wins_over_pyproject_in_the_binding_set() {
    let copy = copy_fixture();
    std::fs::write(copy.path().join("pytest.ini"), "[pytest]\ntestpaths = tests\n").expect("write");
    assert_eq!(binding(copy.path(), A).files, paths(&["pytest.ini", "tests/conftest.py", "tests/test_props.py"]));
    std::fs::write(copy.path().join("tests/conftest.py"), "").expect("write");
    std::fs::write(copy.path().join("conftest.py"), "").expect("write");
    assert_eq!(
        binding(copy.path(), A).files,
        paths(&["conftest.py", "pytest.ini", "tests/conftest.py", "tests/test_props.py"]),
        "every conftest on the path"
    );
}

#[test]
fn a_versioned_token_is_rejected_with_file_and_line() {
    let copy = copy_fixture();
    edit(copy.path(), "tests/test_props.py", |t| t.replace(&format!("# proob:{A}"), &format!("# proob:{A}:v2")));
    match adapter().discover(copy.path()) {
        Err(AdapterError::Token { at, problem: TokenProblem::NotUlid { text, .. } }) => {
            assert_eq!(at.to_string(), "tests/test_props.py:13");
            assert_eq!(text, format!("{A}:v2"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_dangling_token_a_helper_a_method_a_source_file_and_a_duplicate_are_rejected() {
    let dangling = copy_fixture();
    edit(dangling.path(), "tests/test_vacuous.py", |t| {
        t.replace("from hypothesis import given\n", &format!("# proob:{C}\nfrom hypothesis import given\n"))
    });
    assert!(matches!(
        adapter().discover(dangling.path()),
        Err(AdapterError::Token { problem: TokenProblem::NoDef, .. })
    ));

    let helper = copy_fixture();
    edit(helper.path(), "tests/test_unrelated.py", |t| {
        t.replace(
            "def test_count_covering_is_exact",
            &format!("# proob:{C}\ndef helper():\n    pass\n\n\ndef test_count_covering_is_exact"),
        )
    });
    match adapter().discover(helper.path()) {
        Err(AdapterError::Token { problem: TokenProblem::NotATestDef { function }, .. }) => {
            assert_eq!(function, "helper")
        }
        other => panic!("{other:?}"),
    }

    let method = copy_fixture();
    edit(method.path(), "tests/test_unrelated.py", |t| {
        format!("{t}\n\nclass TestGroup:\n    # proob:{C}\n    def test_method(self):\n        pass\n")
    });
    match adapter().discover(method.path()) {
        Err(AdapterError::Token { at, problem: TokenProblem::NestedDef { function } }) => {
            assert_eq!(function, "test_method");
            assert_eq!(at.path, PathBuf::from("tests/test_unrelated.py"));
        }
        other => panic!("{other:?}"),
    }

    let source = copy_fixture();
    std::fs::write(
        source.path().join("interval/extra.py"),
        format!("# proob:{C}\ndef test_in_a_source_file():\n    pass\n"),
    )
    .expect("write");
    match adapter().discover(source.path()) {
        Err(AdapterError::Token { at, problem: TokenProblem::NotATestFile }) => {
            assert_eq!(at.to_string(), "interval/extra.py:1")
        }
        other => panic!("{other:?}"),
    }

    let duplicate = copy_fixture();
    edit(duplicate.path(), "tests/test_vacuous.py", |t| t.replace(&format!("# proob:{D}"), &format!("# proob:{A}")));
    match adapter().discover(duplicate.path()) {
        Err(AdapterError::Duplicate { id: dup, first, second }) => {
            assert_eq!(dup, A);
            assert_eq!(first.to_string(), "tests/test_props.py:13");
            assert_eq!(second.to_string(), "tests/test_vacuous.py:9");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_virtual_environment_and_caches_inside_the_root_are_not_scanned() {
    let copy = copy_fixture();
    for dir in [".venv/lib", "venv-here", "__pycache__"] {
        std::fs::create_dir_all(copy.path().join(dir)).expect("mkdir");
        std::fs::write(
            copy.path().join(dir).join("test_planted.py"),
            format!("# proob:{C}\ndef test_planted():\n    pass\n"),
        )
        .expect("write");
    }
    std::fs::write(copy.path().join("venv-here/pyvenv.cfg"), "home = /usr/bin\n").expect("write");
    let ids: Vec<u128> = discover(copy.path()).into_iter().map(|b| b.id).collect();
    assert_eq!(ids, vec![id(A), id(B), id(D)]);
}

// ---------------------------------------------------------------------------
// Argv and configuration: pure plans
// ---------------------------------------------------------------------------

#[test]
fn the_check_argv_selects_exactly_the_node_id_after_the_separator_and_sets_hypothesis_up() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let r = parse_check_ref(A_REF).expect("check ref");
    let plan = check::plan(Path::new("python3"), &fixture(), &r, tmp.path(), Budget(40), Seed(9), &[]).expect("plan");
    let args = strs(plan.command.args());
    assert_eq!(args[..2], ["-m", "pytest"]);
    assert!(args.windows(2).any(|w| w == ["--hypothesis-seed", "9"]), "{args:?}");
    assert!(args.windows(2).any(|w| w == ["--hypothesis-profile", "proob"]), "{args:?}");
    assert!(
        args.windows(2).any(|w| w == ["-p", "no:cacheprovider"])
            && args.windows(2).any(|w| w == ["-p", "proob_hypothesis"]),
        "{args:?}"
    );
    assert!(args.windows(2).any(|w| w[0] == "--rootdir" && Path::new(w[1]) == fixture()), "{args:?}");
    let sep = plan.command.separator_index().expect("a separator");
    assert_eq!(args[sep + 1], A_REF, "the property-derived node id follows `--`: {args:?}");
    assert_eq!(args.len(), sep + 2, "and is the last argument");
    let env: Vec<(String, String)> = plan
        .command
        .environment()
        .iter()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .collect();
    let pythonpath = env.iter().find(|(k, _)| k == "PYTHONPATH").map(|(_, v)| v.clone()).expect("PYTHONPATH set");
    assert!(pythonpath.starts_with(&tmp.path().join("plugin").to_string_lossy().into_owned()), "{pythonpath}");
    assert!(env.contains(&("PYTHONDONTWRITEBYTECODE".into(), "1".into())));
    assert!(
        env.iter()
            .any(|(k, v)| k == "HYPOTHESIS_STORAGE_DIRECTORY"
                && v.starts_with(&tmp.path().to_string_lossy().into_owned()))
    );
    let plugin = std::fs::read_to_string(&plan.plugin).expect("plugin written");
    assert!(plugin.contains("max_examples=40,") && plugin.contains("database=None"), "{plugin}");
}

#[test]
fn the_score_table_restricts_mutation_to_the_scope_and_selection_to_the_bound_test() {
    assert_eq!(
        score::source_paths(&fixture()).expect("sources"),
        vec!["interval".to_string(), "tests".to_string()],
        "proob/ holds no Python"
    );
    assert!(score::also_copy(&fixture()).is_empty());
    let files = score::mutable_files(&adapter(), &paths(&["interval/merge.py", "README.md"])).expect("ok");
    assert_eq!(files.len(), 1, "only .py files are mutated");
    let r = parse_check_ref(A_REF).expect("check ref");
    let table = score::mutmut_table(
        &r,
        &files,
        &score::source_paths(&fixture()).expect("sources"),
        &[],
        Seed::for_scoring(id(A)),
    );
    let text =
        score::merged_pyproject(Some(&std::fs::read_to_string(fixture().join("pyproject.toml")).expect("read")), table)
            .expect("merges");
    assert!(text.contains("only_mutate = [\"interval/merge.py\"]"), "{text}");
    assert!(text.contains(&format!("pytest_add_cli_args_test_selection = [\"{A_REF}\"]")), "{text}");
    assert!(text.contains("source_paths = [\"interval\", \"tests\"]"), "{text}");
    assert!(text.contains("testpaths = [\"tests\"]"), "the repository's pytest table is kept: {text}");
    let command = score::plan(Path::new("python3"), Path::new("/tmp/copy"), Path::new("/tmp/t"), &[], &[], 2);
    assert_eq!(strs(command.args()), vec!["-m", "mutmut", "run", "--max-children", "2"]);
    let named = [Checked::mutant_name("interval.merge.x_merge__mutmut_3").expect("name")];
    let command = score::plan(Path::new("python3"), Path::new("/tmp/copy"), Path::new("/tmp/t"), &[], &named, 2);
    assert_eq!(
        strs(command.args()),
        vec!["-m", "mutmut", "run", "--max-children", "2", "--", "interval.merge.x_merge__mutmut_3"]
    );
    let listing = score::list_plan(Path::new("python3"), Path::new("/tmp/copy"), Path::new("/tmp/t"), &[], 2);
    assert_eq!(strs(listing.args()), vec!["-m", "mutmut", "run", "--max-children", "2", score::LIST_ONLY]);
}

#[test]
fn a_leading_dash_glob_never_reaches_spawn() {
    assert!(Checked::path(Path::new("-x.py")).is_err());
    assert!(matches!(score::mutable_files(&adapter(), &paths(&["-x.py"])), Err(Outside::Grammar(..))));
    assert!(matches!(
        score::mutable_files(&PythonAdapter::at("backend"), &paths(&["interval/merge.py"])),
        Err(Outside::Root(_))
    ));
    assert!(parse_check_ref("tests/test_props.py::--collect-only").is_err());
    assert!(parse_check_ref("--help::test_x").is_err());
}

// ---------------------------------------------------------------------------
// Captured mutmut outputs
// ---------------------------------------------------------------------------

#[test]
fn captured_meta_files_count_as_the_adapter_scores_them() {
    let meta = |name: &str| {
        counts(&parse_meta(&std::fs::read_to_string(captures().join(name)).expect("capture")).expect("parses"))
    };
    let merge = meta("merge.py.meta");
    assert_eq!((merge.killed, merge.total()), (21, 21));
    assert_eq!(
        score::scored(merge),
        ScoreOutcome::Scored { killed: 21, survived: 0, timeout: 0, unviable: 0, unsampled: 0 }
    );
    let bucket = meta("bucket.py.meta");
    assert_eq!((bucket.no_tests, bucket.total()), (12, 12), "methods are keyed with ǁ and counted like functions");
    assert_eq!(
        score::scored(bucket),
        ScoreOutcome::Errored { reason: score::NOT_TRACED.to_string() },
        "no mutant traced to the test: not a score"
    );
    let count = meta("count.py.meta");
    assert_eq!((count.not_checked, count.total()), (4, 4));
    assert_eq!(score::unreached(count), ScoreOutcome::Errored { reason: score::NOT_TRACED.to_string() });
    let unreached = std::fs::read_to_string(captures().join("unreached.stdout.txt")).expect("capture");
    assert!(unreached.contains(score::STOP_NO_COVERAGE));
    let baseline = std::fs::read_to_string(captures().join("baseline-failed.stdout.txt")).expect("capture");
    assert!(baseline.contains(score::STATS_FAILED));
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
    assert_eq!(names, vec!["python", "pytest", "hypothesis"], "{names:?}");
    assert!(
        out.tool_versions.iter().any(|(k, v)| k == "pytest" && Some(v.as_str()) == pinned("pytest")),
        "{:?}",
        out.tool_versions
    );
    eprintln!("check(passing) took {:?}", started.elapsed());
    assert_fixture_untouched();
}

#[test]
fn failing_property_yields_a_shrunk_counterexample_that_the_seed_reproduces() {
    let b = binding(&fixture(), B);
    let first = adapter().check(&fixture(), &b, Budget(256), Seed(5)).expect("check runs");
    let CheckOutcome::Counterexample { witness: Witness::Full(text), replay } = &first.outcome else {
        panic!("{:?}", first.outcome)
    };
    assert_eq!(*replay, Seed(5));
    assert!(text.contains("takes=[6]"), "shrunk to one take over the capacity: {text}");
    assert!(text.contains("AssertionError: took 6 with 5 left"), "carries the reason: {text}");
    assert!(!text.contains(" at 0x"), "no object addresses: {text}");
    let again = adapter().check(&fixture(), &b, Budget(256), Seed(5)).expect("check runs");
    assert_eq!(again.outcome, first.outcome, "the seed replays the same counterexample");
    let other = adapter().check(&fixture(), &b, Budget(256), Seed(6)).expect("check runs");
    assert!(matches!(other.outcome, CheckOutcome::Counterexample { replay: Seed(6), .. }));
    assert_fixture_untouched();
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
    b.check_ref = "tests/test_props.py::test_merge_renamed".to_string();
    let out = adapter().check(&fixture(), &b, Budget(8), Seed(1)).expect("check runs");
    match out.outcome {
        CheckOutcome::Errored { reason } => assert!(reason.contains("bound test not found"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_test_module_that_does_not_import_is_a_runner_error_not_a_pass() {
    let copy = copy_fixture();
    edit(copy.path(), "tests/test_props.py", |t| {
        t.replace("from interval.bucket import TokenBucket", "from interval.nowhere import TokenBucket")
    });
    let out = adapter().check(copy.path(), &binding(copy.path(), A), Budget(8), Seed(1)).expect("check runs");
    match out.outcome {
        CheckOutcome::Errored { reason } => {
            assert!(reason.contains("could not collect") && reason.contains("interval.nowhere"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
}

/// Around five seconds: mutmut copies the fixture, maps the test to the
/// functions it runs, then runs the twenty-odd mutants of merge.py.
#[test]
fn the_killing_property_scores_at_least_one_kill_with_the_pinned_mutmut() {
    let started = Instant::now();
    let scored =
        adapter().score(&fixture(), &binding(&fixture(), A), &paths(&["interval/merge.py"])).expect("score runs");
    eprintln!("score(merge) took {:?}", started.elapsed());
    match scored.outcome {
        ScoreOutcome::Scored { killed, survived, timeout, unviable, unsampled: 0 } => {
            assert!(killed >= 1, "the property kills mutants of merge: {killed}");
            assert!(killed + survived >= 10, "merge.py has mutants: {killed} + {survived}");
            assert_eq!((timeout, unviable), (0, 0));
        }
        other => panic!("{other:?}"),
    }
    assert!(
        scored.tool_versions.iter().any(|(k, v)| k == MUTMUT && Some(v.as_str()) == pinned(MUTMUT)),
        "{:?}",
        scored.tool_versions
    );
    assert_fixture_untouched();
}

/// The capped score: merge.py's twenty-odd mutants under a cap of five.
#[test]
fn a_capped_score_runs_exactly_the_sample_and_reports_the_rest_unsampled() {
    let capped = adapter().with_limits(Limits { max_mutants: 5, jobs: Some(2) });
    let b = binding(&fixture(), A);
    let scored = capped.score(&fixture(), &b, &paths(&["interval/merge.py"])).expect("score runs");
    match scored.outcome {
        ScoreOutcome::Scored { killed, survived, timeout, unviable, unsampled } => {
            assert_eq!(killed + survived + timeout + unviable, 5, "exactly the sample ran and was counted");
            assert!(unsampled >= 5, "merge.py has more mutants than the cap: {unsampled}");
        }
        other => panic!("{other:?}"),
    }
    let again = capped.score(&fixture(), &b, &paths(&["interval/merge.py"])).expect("score runs");
    assert_eq!(again.outcome, scored.outcome, "the same tree and property score the same sample");
    assert_fixture_untouched();
}

#[test]
fn the_vacuous_test_passes_and_kills_nothing_even_though_an_unrelated_test_would() {
    let d = binding(&fixture(), D);
    let checked = adapter().check(&fixture(), &d, Budget(64), Seed(3)).expect("check runs");
    assert_eq!(checked.outcome, CheckOutcome::NoCounterexample { budget: Budget(64), replay: Seed(3) });
    let scored = adapter().score(&fixture(), &d, &paths(&["interval/count.py"])).expect("score runs");
    // The bound test never reaches count_covering, so mutmut traces it into
    // none of the scope: proob reports no score rather than "cannot fail".
    assert_eq!(scored.outcome, ScoreOutcome::Errored { reason: score::NOT_TRACED.to_string() });
    assert_fixture_untouched();
}

#[test]
fn a_failing_baseline_scores_nothing() {
    let out =
        adapter().score(&fixture(), &binding(&fixture(), B), &paths(&["interval/bucket.py"])).expect("score runs");
    match out.outcome {
        ScoreOutcome::Errored { reason } => assert!(reason.contains("baseline failed"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_scope_without_python_sources_has_no_mutants() {
    let out = adapter().score(&fixture(), &binding(&fixture(), D), &paths(&["proob/config.toml"])).expect("score runs");
    assert_eq!(out.outcome, ScoreOutcome::NoMutants);
}

#[test]
fn a_missing_interpreter_yields_errored_with_the_install_hint() {
    let sandbox = tempfile::tempdir().expect("tempdir");
    let none = adapter().with_tool_env(vec![(OsString::from("PATH"), sandbox.path().join("empty").into_os_string())]);
    let out = none.check(&fixture(), &binding(&fixture(), A), Budget(8), Seed(1)).expect("check returns a result");
    assert!(
        matches!(&out.outcome, CheckOutcome::Errored { reason } if reason.contains("python3 was not found")),
        "{:?}",
        out.outcome
    );
    let out = none
        .score(&fixture(), &binding(&fixture(), A), &paths(&["interval/merge.py"]))
        .expect("score returns a result");
    assert!(
        matches!(&out.outcome, ScoreOutcome::Errored { reason } if reason.contains("python3 was not found")),
        "{:?}",
        out.outcome
    );
}

/// A `python3` that reports another mutmut: the version probe is the only
/// thing it answers, so the refusal is reached before any run.
#[cfg(unix)]
#[test]
fn another_mutmut_version_is_refused_naming_expected_and_found() {
    use std::os::unix::fs::PermissionsExt;
    let sandbox = tempfile::tempdir().expect("tempdir");
    let bin = sandbox.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir");
    let shim = bin.join("python3");
    std::fs::write(&shim, "#!/bin/sh\ncase \"$1\" in\n  --version) echo \"Python 3.12.0\" ;;\n  -c) printf 'pytest 9.1.1\\nhypothesis 6.169.4\\nmutmut 3.0.0\\n' ;;\n  *) exit 97 ;;\nesac\n").expect("write");
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let shimmed = adapter().with_tool_env(vec![(OsString::from("PATH"), bin.into_os_string())]);
    let out = shimmed
        .score(&fixture(), &binding(&fixture(), A), &paths(&["interval/merge.py"]))
        .expect("score returns a result");
    match out.outcome {
        ScoreOutcome::Errored { reason } => {
            assert!(reason.contains("mutmut 3.0.0 is installed"), "{reason}");
            assert!(reason.contains(&format!("mutmut {} exactly", pinned(MUTMUT).expect("pin"))), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        out.tool_versions.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        vec!["python", "pytest", "hypothesis"],
        "what was learnt before the refusal"
    );
}
