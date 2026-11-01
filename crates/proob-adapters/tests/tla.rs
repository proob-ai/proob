//! The TLA+ adapter against `tests/fixtures/tla-spec`: discovery and binding
//! sets, the argv, config refusals, the outcome table over the captured TLC
//! 2.19 outputs in `tests/fixtures/tla`, and real runs of TLC (check and
//! score). Real runs need `java` on `PATH` and `PROOB_TLA2TOOLS` naming a
//! `tla2tools.jar`; without them they say so and return, and fail instead
//! under `PROOB_REQUIRE_TLC=1`.
use proob_adapters::tla::TlaAdapter;
use proob_adapters::tla::check::{outcome, plan, stopped};
use proob_adapters::tla::tools::{JAR_ENV, Resolution, Tools, resolve};
use proob_adapters::{
    Adapter, AdapterError, Binding, Budget, CheckOutcome, RegistryError, ScoreOutcome, Seed, TokenProblem,
};
use proob_core::types::Kind;
use proob_facts::parsers::tlc::classify;
use proob_facts::ulid;
use proob_spawn::SpawnError;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MUTUAL_EXCLUSION: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XTA";
const COUNT_BELOW_TWO: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XTB";
const SOMEONE_ENTERS: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XTC";

fn id(text: &str) -> u128 {
    ulid::decode(text).expect("ulid")
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tla-spec").canonicalize().expect("fixture exists")
}

fn captured(case: &str) -> (String, String, Option<i32>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tla");
    let read = |ext: &str| std::fs::read_to_string(dir.join(format!("{case}.{ext}"))).unwrap_or_default();
    (read("stdout.txt"), read("stderr.txt"), read("exit").trim().parse().ok())
}

fn binding(text: &str) -> Binding {
    TlaAdapter::at(".")
        .discover(&fixture())
        .expect("discovery succeeds")
        .into_iter()
        .find(|b| b.id == id(text))
        .unwrap_or_else(|| panic!("{text} is bound"))
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::write(path, text).expect("write");
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

#[test]
fn discovery_binds_each_tokened_definition_as_a_model_with_its_modules_and_cfg() {
    let found = TlaAdapter::at(".").discover(&fixture()).expect("discovery succeeds");
    let ids: Vec<u128> = found.iter().map(|b| b.id).collect();
    assert_eq!(ids, [MUTUAL_EXCLUSION, COUNT_BELOW_TWO, SOMEONE_ENTERS].map(id).to_vec());
    let b = &found[0];
    assert_eq!(b.kind, Kind::Model);
    assert_eq!(b.check_ref, "Mutex.tla MutualExclusion");
    assert_eq!(b.token.to_string(), "Mutex.tla:30");
    let files: Vec<PathBuf> = ["Bounds.tla", "Mutex.cfg", "Mutex.tla"].iter().map(PathBuf::from).collect();
    assert_eq!(b.files, files, "the module, its cfg, and the local module it extends; not Naturals");
}

#[test]
fn a_root_below_the_repository_and_its_problems() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let spec = "---- MODULE S ----\nEXTENDS Naturals, Helper\nVARIABLE x\nInit == x = 0\nNext == x' = x\n\\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XWA\nInv == x = 0\n====\n";
    write(root, "specs/S.tla", spec);
    write(root, "specs/Helper.tla", "---- MODULE Helper ----\nINSTANCE Deep\n====\n");
    write(root, "specs/Deep.tla", "---- MODULE Deep ----\n====\n");
    write(root, "specs/Other.tla", "---- MODULE Other ----\n====\n");
    let adapter = TlaAdapter::at("specs");
    let missing_cfg = adapter.discover(root).expect_err("no cfg");
    assert!(
        matches!(&missing_cfg, AdapterError::Token { problem: TokenProblem::NoTlaConfig { config }, .. } if config == "S.cfg"),
        "{missing_cfg}"
    );
    write(root, "specs/S.cfg", "INIT Init\nNEXT Next\nINVARIANT Inv\n");
    let found = adapter.discover(root).expect("discovery succeeds");
    assert_eq!(found[0].check_ref, "S.tla Inv");
    assert_eq!(found[0].token.to_string(), "specs/S.tla:6");
    let files: Vec<PathBuf> =
        ["specs/Deep.tla", "specs/Helper.tla", "specs/S.cfg", "specs/S.tla"].iter().map(PathBuf::from).collect();
    assert_eq!(found[0].files, files, "transitive through EXTENDS and INSTANCE; Other is not reached");
    let before = found[0].binding_hash;
    write(root, "specs/Deep.tla", "---- MODULE Deep ----\nX == 1\n====\n");
    assert_ne!(adapter.discover(root).expect("discovers")[0].binding_hash, before);
    write(root, "specs/T.tla", "---- MODULE T ----\n\\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XWB\nVARIABLE y\n====\n");
    let err = adapter.discover(root).expect_err("binds nothing");
    assert!(matches!(&err, AdapterError::Token { problem: TokenProblem::NoTlaDefinition, .. }), "{err}");
    write(root, "specs/T.tla", "---- MODULE T ----\n\\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XWA\nJ == TRUE\n====\n");
    write(root, "specs/T.cfg", "INVARIANT J\n");
    assert!(matches!(adapter.discover(root), Err(AdapterError::Duplicate { .. })));
}

// ---------------------------------------------------------------------------
// Argv and config
// ---------------------------------------------------------------------------

#[test]
fn the_argv_runs_one_worker_with_a_fixed_fingerprint_and_seed_in_its_own_metadir() {
    let tools = Tools { java: "/j/java".into(), jar: "/t/tla2tools.jar".into(), versions: Vec::new() };
    let c = plan(
        &tools,
        Path::new("/lab/model/Mutex.tla"),
        Path::new("/lab/c.cfg"),
        Path::new("/lab/meta-1"),
        Duration::ZERO,
    );
    let args: Vec<String> = c.args().iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(
        args,
        [
            "-XX:+UseParallelGC",
            "-cp",
            "/t/tla2tools.jar",
            "tlc2.TLC",
            "-config",
            "/lab/c.cfg",
            "-workers",
            "1",
            "-fp",
            "0",
            "-seed",
            "0",
            "-metadir",
            "/lab/meta-1",
            "/lab/model/Mutex.tla"
        ]
    );
    assert!(!args.iter().any(|a| a == "-deadlock"), "deadlock is the cfg's to turn off");
}

#[test]
fn config_refuses_what_the_tla_adapter_does_not_take() {
    use proob_facts::config::parse_config;
    let config = |runner: &str, tool: &str, options: &str| {
        let text = format!(
            "schema_version = 1\n[[adapters]]\nroot = \".\"\nlanguage = \"tla\"\nrunner = \"{runner}\"\nmutation_tool = \"{tool}\"\n{options}\n[policy]\nholds = \"pass\"\nchecked = \"pass\"\nproposed = \"pass\"\nwaived = \"pass\"\nnot_checked_within_grace = \"pass\"\nnot_checked_after_grace = \"warn\"\nre_approve = \"warn\"\nbroken_affected = \"block\"\nbroken_unaffected = \"pass\"\n"
        );
        proob_adapters::adapters_for(&parse_config(&text).expect("parses"))
    };
    let ok = config("tlc", "spec-mutants", "max_mutants = 5").expect("accepted");
    assert_eq!(ok[0].language(), proob_adapters::Language::Tla);
    let refused = |runner: &str, tool: &str, options: &str| config(runner, tool, options).err().expect("refused");
    assert!(matches!(refused("apalache", "spec-mutants", ""), RegistryError::UnsupportedRunner { .. }));
    assert!(matches!(refused("tlc", "mutmut", ""), RegistryError::UnsupportedMutationTool { .. }));
    assert!(matches!(
        refused("tlc", "spec-mutants", "jobs = 2"),
        RegistryError::UnsupportedOption { option: "jobs", .. }
    ));
    assert!(matches!(
        refused("tlc", "spec-mutants", "max_mutants = 0"),
        RegistryError::UnsupportedOption { option: "max_mutants", .. }
    ));
    assert!(matches!(
        refused("tlc", "spec-mutants", "features = [\"x\"]"),
        RegistryError::UnsupportedOption { option: "features", .. }
    ));
}

// ---------------------------------------------------------------------------
// The outcome table over captured TLC output
// ---------------------------------------------------------------------------

fn judged(case: &str, name: &str) -> CheckOutcome {
    let (stdout, stderr, code) = captured(case);
    outcome(classify(&stdout, &stderr), code, name)
}

fn reason(o: &CheckOutcome) -> &str {
    match o {
        CheckOutcome::Errored { reason } => reason,
        other => panic!("expected errored, got {other:?}"),
    }
}

#[test]
fn each_row_of_the_outcome_table() {
    assert_eq!(judged("no_error", "MutualExclusion"), CheckOutcome::NoViolation);
    let CheckOutcome::Violation { trace } = judged("invariant_violated", "CountBelowTwo") else { panic!() };
    assert!(trace.starts_with("Invariant CountBelowTwo is violated.\nState 1: <Initial predicate>"), "{trace}");
    assert!(trace.contains("/\\ count = 2"), "the last state: {trace}");
    assert!(reason(&judged("invariant_violated", "MutualExclusion")).contains("only `MutualExclusion` was checked"));
    let CheckOutcome::Violation { trace } = judged("temporal_violated", "SomeoneEnters") else { panic!() };
    assert!(trace.contains("Stuttering"), "{trace}");
    assert!(reason(&judged("deadlock", "MutualExclusion")).starts_with("deadlock reached"));
    assert!(reason(&judged("deadlock", "MutualExclusion")).contains("CHECK_DEADLOCK FALSE"));
    assert!(reason(&judged("parse_error", "MutualExclusion")).starts_with("TLC could not parse the spec"));
    assert!(reason(&judged("semantic_error", "MutualExclusion")).contains("Unknown operator"));
    assert!(reason(&judged("unrecognised", "MutualExclusion")).starts_with("unrecognised TLC output"));
    let (stdout, _, _) = captured("no_error");
    assert!(
        reason(&outcome(classify(&stdout, ""), Some(1), "MutualExclusion")).contains("exit code 1"),
        "the exit code must agree"
    );
    assert!(reason(&outcome(classify(&stdout, ""), None, "M")).contains("signal"));
}

#[test]
fn a_run_that_never_finished_is_errored() {
    let t = SpawnError::TimedOut { program: "java".into(), after: Duration::from_secs(1800) };
    assert!(reason(&stopped(&t)).contains("timed out after 30 minutes"));
    let m = SpawnError::OutOfMemory { program: "java".into(), limit_bytes: 1 << 30, peak_bytes: 2 << 30 };
    assert!(reason(&stopped(&m)).contains("1024 MB") && reason(&stopped(&m)).contains("out of memory"));
}

#[test]
fn without_the_jar_check_and_score_are_errored_with_how_to_set_it() {
    let adapter = TlaAdapter::at(".").with_tool_env(vec![(OsString::from(JAR_ENV), OsString::new())]);
    let b = binding(MUTUAL_EXCLUSION);
    let check = adapter.check(&fixture(), &b, Budget::DEFAULT, Seed(0)).expect("runs");
    assert!(reason(&check.outcome).contains("set PROOB_TLA2TOOLS"), "{:?}", check.outcome);
    let score = adapter.score(&fixture(), &b, &[]).expect("runs");
    assert!(matches!(&score.outcome, ScoreOutcome::Errored { reason } if reason.starts_with("TLC was not found")));
}

// ---------------------------------------------------------------------------
// Real TLC
// ---------------------------------------------------------------------------

/// Whether TLC resolves here; when it does not, says why, and panics under `PROOB_REQUIRE_TLC=1`.
fn tlc_here() -> bool {
    match resolve(&[]) {
        Resolution::Ready(_) => true,
        Resolution::Missing { reason, .. } if std::env::var("PROOB_REQUIRE_TLC").as_deref() == Ok("1") => {
            panic!("PROOB_REQUIRE_TLC=1 but TLC does not resolve: {reason}")
        }
        Resolution::Missing { reason, .. } => {
            eprintln!("skipped: {reason}");
            false
        }
    }
}

fn listing(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out: Vec<(PathBuf, Vec<u8>)> = std::fs::read_dir(dir)
        .expect("dir")
        .map(|e| e.expect("entry").path())
        .map(|p| (p.clone(), std::fs::read(&p).unwrap_or_default()))
        .collect();
    out.sort();
    out
}

#[test]
fn tlc_finds_no_violation_of_the_holding_invariant_and_a_trace_for_the_others() {
    if !tlc_here() {
        return;
    }
    let before = listing(&fixture());
    let adapter = TlaAdapter::at(".");
    let holds = adapter.check(&fixture(), &binding(MUTUAL_EXCLUSION), Budget::DEFAULT, Seed(0)).expect("runs");
    assert_eq!(holds.outcome, CheckOutcome::NoViolation);
    let names: Vec<&str> = holds.tool_versions.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, ["java", "tlc"]);
    let fails = adapter.check(&fixture(), &binding(COUNT_BELOW_TWO), Budget::DEFAULT, Seed(0)).expect("runs");
    let CheckOutcome::Violation { trace } = &fails.outcome else { panic!("{:?}", fails.outcome) };
    assert!(trace.starts_with("Invariant CountBelowTwo is violated.\nState 1:"), "{trace}");
    let temporal = adapter.check(&fixture(), &binding(SOMEONE_ENTERS), Budget::DEFAULT, Seed(0)).expect("runs");
    assert!(
        matches!(&temporal.outcome, CheckOutcome::Violation { trace } if trace.starts_with("Temporal property SomeoneEnters"))
    );
    assert_eq!(listing(&fixture()), before, "nothing is written into the repository");
}

#[test]
fn the_score_of_the_holding_invariant_kills_a_spec_mutant() {
    if !tlc_here() {
        return;
    }
    let score = TlaAdapter::at(".").score(&fixture(), &binding(MUTUAL_EXCLUSION), &[]).expect("runs");
    let ScoreOutcome::Scored { killed, survived, unviable, unsampled, .. } = score.outcome else {
        panic!("{:?}", score.outcome)
    };
    eprintln!("killed {killed}, survived {survived}, unviable {unviable}");
    assert!(killed >= 1, "dropping `lock = FALSE` from Acquire breaks mutual exclusion");
    assert_eq!(unsampled, 0);
    assert!(score.tool_versions.contains(&("spec-mutants".to_string(), "tla-spec-mutants-v1".to_string())));
    assert!(score.tool_versions.iter().any(|(k, _)| k == "tlc"));
    let errored = TlaAdapter::at(".").score(&fixture(), &binding(COUNT_BELOW_TWO), &[]).expect("runs");
    assert!(
        matches!(&errored.outcome, ScoreOutcome::Errored { reason } if reason.starts_with("baseline failed")),
        "{:?}",
        errored.outcome
    );
}
