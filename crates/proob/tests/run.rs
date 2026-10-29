//! `proob run`, and discovery through the CLI, on a committed copy of the
//! Rust fixture: status shows Bound and Unbound from real discovery, validate
//! rejects a versioned token, and `run check|score` print and record
//! `RunResultRecord`s as local, unverified results.
mod common;

use common::*;
use serde_json::Value;
use std::path::{Path, PathBuf};

const A: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHA";
const B: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHB";
const C: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHC";
const D: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHD";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rust-repo").canonicalize().expect("fixture")
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("read_dir").filter_map(Result::ok) {
        let name = entry.file_name();
        if name == "target" {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        if src.is_dir() {
            std::fs::create_dir_all(&dst).expect("mkdir");
            copy_tree(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).expect("copy");
        }
    }
}

struct RustRepo {
    repo: TempRepo,
    fixture: Fixture,
}

impl RustRepo {
    /// The fixture, committed to main by a direct push, with a GitHub that
    /// knows no pull requests: every property is Proposed, and the binding
    /// axis comes from discovery alone.
    fn new() -> RustRepo {
        let repo = TempRepo::new();
        copy_tree(&fixture_dir(), repo.root());
        repo.commit("the rust fixture", NOW - 10 * DAY);
        let scenario = ScenarioBuilder::new().governance(true, true, true, true).build();
        RustRepo { repo, fixture: Fixture::new(&scenario, None) }
    }

    fn proob(&self) -> std::process::Command {
        let mut cmd = self.repo.proob();
        // Tool processes need the real PATH; the sandbox PATH holds only git.
        cmd.env("PATH", std::env::var_os("PATH").expect("PATH"));
        cmd.env("PROOB_FIXTURE_DIR", self.fixture.path());
        // Share the fixture's build directory across copies, so each run
        // compiles proptest at most once per machine.
        cmd.env("CARGO_TARGET_DIR", fixture_dir().join("target"));
        cmd
    }
}

fn rows(report: &Value) -> std::collections::BTreeMap<String, Value> {
    report["properties"]
        .as_array()
        .expect("properties")
        .iter()
        .map(|r| (r["id"].as_str().expect("id").to_string(), r.clone()))
        .collect()
}

#[test]
fn status_shows_bound_and_unbound_from_real_discovery() {
    let r = RustRepo::new();
    let out = run(r.proob().args(["status", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let rows = rows(&out.json());
    for id in [A, B, D] {
        let binding = &rows[id]["status"]["binding"];
        assert_eq!(binding["state"], "bound", "{id}: {binding}");
        assert_eq!(binding["binding_hash"].as_str().map(str::len), Some(64));
        assert_ne!(binding["primary"]["state"], "fresh", "no evidence run exists");
    }
    assert_eq!(rows[C]["status"]["binding"]["state"], "unbound");
    assert_eq!(rows[C]["status"]["binding"]["why"], "no_check");
    assert!(rows.values().all(|r| r["word"] == "Proposed"), "a direct push is never approved: {rows:?}");
    let hash = run(r.proob().args(["hash", A, "--json"])).json();
    assert_eq!(hash["binding_files"], serde_json::json!(["tests/common/mod.rs", "tests/props.rs"]));
    assert_eq!(hash["scope_files"], serde_json::json!(["src/ordering.rs"]));
}

#[test]
fn validate_rejects_a_versioned_token_and_a_token_without_a_property() {
    let r = RustRepo::new();
    let out = run(r.proob().arg("validate"));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    r.repo.write(
        "tests/props.rs",
        &r.repo.read("tests/props.rs").replace(&format!("// proob:{A}"), &format!("// proob:{A}:v2")),
    );
    let out = run(r.proob().arg("validate"));
    assert_eq!(out.code, 1, "{}", out.stdout);
    assert!(
        out.stdout.contains(&format!("tests/props.rs:12: binding token `proob:{A}:v2` is not `proob:<ULID>`")),
        "{}",
        out.stdout
    );
    r.repo.write(
        "tests/props.rs",
        &r.repo.read("tests/props.rs").replace(&format!("// proob:{A}:v2"), "// proob:01K8V5NQ4A0F3R7Y2M9D6P1XZZ"),
    );
    let out = run(r.proob().arg("validate"));
    assert_eq!(out.code, 1, "{}", out.stdout);
    assert!(
        out.stdout.contains("tests/props.rs:12: binding token `proob:01K8V5NQ4A0F3R7Y2M9D6P1XZZ` names no property"),
        "{}",
        out.stdout
    );
    // The amended tree still derives: status refuses nothing it can explain.
    let out = run(r.proob().args(["status", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(rows(&out.json())[A]["status"]["binding"]["state"], "unbound");
}

#[test]
fn run_check_and_score_print_and_record_local_unverified_results() {
    let r = RustRepo::new();
    let results = r.repo.root().join("proob/.cache/results/results.jsonl");
    let before = rows(&run(r.proob().args(["status", "--json"])).json());

    let out = run(r.proob().args(["run", "check", A, "--json", "--budget", "40", "--seed", "9", "--record"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let record = out.json();
    assert_eq!(record["kind"], "check");
    assert_eq!(record["outcome"], serde_json::json!({ "result": "no_counterexample", "budget": 40 }));
    assert_eq!(record["replay"], serde_json::json!({ "seed": "9", "cases": 40 }));
    assert_eq!(record["binding_hash"].as_str().map(str::len), Some(64));
    assert_eq!(record["head_sha"].as_str().map(str::len), Some(40));
    assert!(record["tool_versions"]["cargo-nextest"].is_string(), "{record}");
    assert!(record.get("run_id").is_none(), "a run result is not evidence");
    let recorded = std::fs::read_to_string(&results).expect("results.jsonl");
    assert_eq!(recorded.trim(), out.stdout.trim());

    let out = run(r.proob().args(["run", "check", B, "--seed", "9"]));
    assert_eq!(out.code, 1, "a counterexample exits 1: {}", out.stderr);
    assert!(out.stdout.starts_with("proob run check: local run, unverified"), "{}", out.stdout);
    assert!(out.stdout.contains("counterexample: ") && out.stdout.contains("replay with seed 9"), "{}", out.stdout);
    assert!(out.stdout.contains("tests/props.rs:18"), "{}", out.stdout);

    let out = run(r.proob().args(["run", "check", C]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains(&format!("no check carries `proob:{C}`")), "{}", out.stderr);

    let out = run(r.proob().args(["run", "score", D, "--json", "--record"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let record = out.json();
    assert_eq!(record["kind"], "score");
    assert_eq!(record["outcome"]["result"], "scored");
    assert_eq!(record["outcome"]["killed"], 0, "{record}");
    assert!(record["outcome"]["survived"].as_u64().unwrap_or(0) >= 3, "{record}");
    assert!(record["tool_versions"]["cargo-mutants"].is_string(), "{record}");
    let recorded = std::fs::read_to_string(&results).expect("results.jsonl");
    assert_eq!(recorded.lines().count(), 2);

    // Local results never become evidence: status is byte-for-byte what it was.
    let after = rows(&run(r.proob().args(["status", "--json"])).json());
    assert_eq!(after, before);
    assert_ne!(after[A]["status"]["binding"]["primary"]["state"], "fresh");
}

// ---------------------------------------------------------------------------
// Proof-backed properties: the Verus fixture through the CLI
// ---------------------------------------------------------------------------

const VA: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XVA";
const VB: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XVB";

fn verus_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/verus-crate").canonicalize().expect("fixture")
}

struct VerusRepo {
    repo: TempRepo,
    fixture: Fixture,
}

impl VerusRepo {
    /// The Verus fixture, committed to main by a direct push, with a GitHub
    /// that knows no pull requests.
    fn new() -> VerusRepo {
        let repo = TempRepo::new();
        copy_tree(&verus_fixture_dir(), repo.root());
        repo.commit("the verus fixture", NOW - 10 * DAY);
        let scenario = ScenarioBuilder::new().governance(true, true, true, true).build();
        VerusRepo { repo, fixture: Fixture::new(&scenario, None) }
    }

    fn proob(&self) -> std::process::Command {
        let mut cmd = self.repo.proob();
        // The toolchain is found through PATH, PROOB_VERUS_DIR, or the cache
        // under HOME; the sandbox PATH holds only git, so hand the real one over.
        cmd.env("PATH", std::env::var_os("PATH").expect("PATH"));
        cmd.env("PROOB_FIXTURE_DIR", self.fixture.path());
        // Share the fixture's build directory across copies, so each run
        // builds vstd under Verus at most once per machine.
        cmd.env("CARGO_TARGET_DIR", verus_fixture_dir().join("target"));
        cmd
    }
}

/// `None` when this machine has no Verus toolchain: the run's record says so.
fn verified_or_skipped(out: &Run) -> Option<Value> {
    let record = out.json();
    let reason = record["outcome"]["reason"].as_str().unwrap_or("");
    if record["outcome"]["result"] == "errored" && reason.contains("Verus toolchain was not found") {
        eprintln!("skipped: {reason}");
        return None;
    }
    Some(record)
}

#[test]
fn status_and_hash_show_proof_bindings_from_verus_discovery() {
    let r = VerusRepo::new();
    let out = run(r.proob().args(["status", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let rows = rows(&out.json());
    for id in [VA, VB] {
        assert_eq!(rows[id]["kind"], "proof", "{id}");
        let binding = &rows[id]["status"]["binding"];
        assert_eq!(binding["state"], "bound", "{id}: {binding}");
        assert_eq!(binding["binding_hash"].as_str().map(str::len), Some(64));
        assert_ne!(binding["primary"]["state"], "fresh", "no evidence run exists");
    }
    assert_eq!(
        rows[VA]["status"]["binding"]["binding_hash"], rows[VB]["status"]["binding"]["binding_hash"],
        "one verified crate, one binding set"
    );
    let hash = run(r.proob().args(["hash", VA, "--json"])).json();
    assert_eq!(
        hash["binding_files"],
        serde_json::json!(["Cargo.toml", "clamp/Cargo.toml", "clamp/src/lib.rs", "rust-toolchain.toml"])
    );
    assert_eq!(hash["scope_files"], serde_json::json!(["clamp/src/lib.rs"]));
    let out = run(r.proob().arg("validate"));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
}

#[test]
fn validate_wants_kind_proof_on_a_proof_binding_and_rejects_a_token_outside_verus() {
    let r = VerusRepo::new();
    r.repo.write(&property_path(VA), &r.repo.read(&property_path(VA)).replace("kind = \"proof\"", "kind = \"test\""));
    let out = run(r.proob().arg("validate"));
    assert_eq!(out.code, 1, "{}", out.stdout);
    assert!(
        out.stdout.contains(&format!("{}: bound to a verus! item at clamp/src/lib.rs:10", property_path(VA))),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("add `kind = \"proof\"`"), "{}", out.stdout);
    // And `run check` refuses the mismatch before running anything.
    let out = run(r.proob().args(["run", "check", VA]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("kind = test but the bound check at clamp/src/lib.rs:10 is a proof"), "{}", out.stderr);

    let r = VerusRepo::new();
    r.repo.write(
        "clamp/src/lib.rs",
        &format!(
            "{}\n// proob:{VB}\npub fn plain() {{}}\n",
            r.repo.read("clamp/src/lib.rs").replace(&format!("// proob:{VB}\n"), "")
        ),
    );
    let out = run(r.proob().arg("validate"));
    assert_eq!(out.code, 1, "{}", out.stdout);
    assert!(out.stdout.contains("outside every `verus!` block"), "{}", out.stdout);
}

#[test]
fn a_crate_without_verify_true_renders_not_checked_no_adapter() {
    let r = VerusRepo::new();
    r.repo.write("clamp/Cargo.toml", &r.repo.read("clamp/Cargo.toml").replace("verify = true", "verify = false"));
    let out = run(r.proob().args(["status", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let rows = rows(&out.json());
    for id in [VA, VB] {
        assert_eq!(rows[id]["status"]["binding"]["state"], "unbound", "{id}");
        assert_eq!(rows[id]["status"]["binding"]["why"], "no_adapter", "{id}");
    }
    let out = run(r.proob().args(["run", "check", VA]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("Not checked (no adapter)") && out.stderr.contains("verify = true"), "{}", out.stderr);
}

/// Two verifier runs on a warm build directory, a few seconds each; the
/// first on a machine builds vstd under Verus (around a minute).
#[test]
fn run_check_on_a_proof_records_verified_then_a_bug_fails_it() {
    let r = VerusRepo::new();
    let out = run(r.proob().args(["run", "check", VA, "--json", "--record"]));
    let Some(record) = verified_or_skipped(&out) else { return };
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(record["kind"], "proof");
    assert_eq!(record["outcome"], serde_json::json!({ "result": "verified", "rlimit": 10 }), "{record}");
    assert!(record.get("replay").is_none(), "a proof has no seed");
    assert!(record["tool_versions"]["verus"].is_string() && record["tool_versions"]["z3"].is_string(), "{record}");
    assert!(record.get("run_id").is_none(), "a run result is not evidence");
    let recorded =
        std::fs::read_to_string(r.repo.root().join("proob/.cache/results/results.jsonl")).expect("results.jsonl");
    assert_eq!(recorded.trim(), out.stdout.trim());

    r.repo.write(
        "clamp/src/lib.rs",
        &r.repo.read("clamp/src/lib.rs").replace("    if x < lo {\n        lo\n", "    if x < lo {\n        hi\n"),
    );
    let out = run(r.proob().args(["run", "check", VA]));
    assert_eq!(out.code, 1, "a failed proof exits 1: {}", out.stderr);
    assert!(out.stdout.starts_with("proob run proof: local run, unverified"), "{}", out.stdout);
    assert!(
        out.stdout.contains("proof failed: ") && out.stdout.contains("postcondition not satisfied"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("clamp/src/lib.rs:"), "{}", out.stdout);
}

/// Mutation-under-proof on the weak item: a baseline and three mutants (two
/// comparison flips and the branch swap), none of which `ensures true` kills.
#[test]
fn run_score_on_the_weak_spec_records_zero_kills() {
    let r = VerusRepo::new();
    let out = run(r.proob().args(["run", "score", VB, "--json"]));
    let Some(record) = verified_or_skipped(&out) else { return };
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(record["kind"], "score");
    assert_eq!(
        record["outcome"],
        serde_json::json!({ "result": "scored", "killed": 0, "survived": 3, "timeout": 0, "unviable": 0 }),
        "{record}"
    );
    assert_eq!(record["tool_versions"]["mutants-under-proof"], "mutants-under-proof-v2", "{record}");
}
