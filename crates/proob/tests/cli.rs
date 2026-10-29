//! The CLI end to end: `status --json` over a fixture repository with fixture
//! GitHub facts and runs (one `Status` per verdict class), `check`'s exit
//! codes and bootstrap, the unverified views, `validate`, `hash`, `run`,
//! and the U1 promises (`--version`, `--help`, usage).
mod common;

use common::scenarios::*;
use common::*;
use serde_json::{Value, json};
use std::process::Command;

// ---------------------------------------------------------------------------
// U1 promises
// ---------------------------------------------------------------------------

#[test]
fn version_prints_the_workspace_version() {
    let out = run(Command::new(env!("CARGO_BIN_EXE_proob")).arg("--version"));
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout.trim_end(), format!("proob {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_exits_zero_and_names_the_binary() {
    let out = run(Command::new(env!("CARGO_BIN_EXE_proob")).arg("--help"));
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("Usage: proob"), "help was: {}", out.stdout);
    for sub in ["validate", "hash", "status", "check", "run", "init", "hook", "action"] {
        assert!(out.stdout.contains(sub), "help lacks {sub}");
    }
}

#[test]
fn bare_and_unknown_invocations_exit_two_with_usage() {
    let bare = run(&mut Command::new(env!("CARGO_BIN_EXE_proob")));
    assert_eq!(bare.code, 2);
    let unknown = run(Command::new(env!("CARGO_BIN_EXE_proob")).arg("frobnicate"));
    assert_eq!(unknown.code, 2);
    assert!(unknown.stderr.contains("Usage"), "{}", unknown.stderr);
}

#[test]
fn action_outside_github_actions_names_the_missing_variable() {
    let gh = empty_gh_config();
    let out =
        run(proob(gh.path()).env_remove("GITHUB_EVENT_PATH").args(["action", "--mode", "check", "--job", "judge"]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("GITHUB_EVENT_PATH is not set"), "{}", out.stderr);
    let out = run(proob(gh.path()).args(["action", "--mode", "nope", "--job", "judge"]));
    assert_eq!(out.code, 2, "a mode outside the closed set is a usage error");
}

#[test]
fn status_without_a_proob_directory_says_so() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gh = empty_gh_config();
    let out = run(proob(gh.path()).arg("--repo").arg(dir.path()).arg("status"));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("no proob/ directory"), "{}", out.stderr);
    let out = run(proob(gh.path()).arg("--repo").arg(dir.path()).args(["status", "--json"]));
    assert_eq!(out.code, 2);
    assert!(out.json()["error"].as_str().expect("error").contains("no proob/ directory"));
}

fn rows_by_id(report: &Value) -> std::collections::BTreeMap<String, Value> {
    report["properties"]
        .as_array()
        .expect("properties")
        .iter()
        .map(|r| (r["id"].as_str().expect("id").to_string(), r.clone()))
        .collect()
}

#[test]
fn status_json_yields_one_status_per_verdict_class() {
    let c = composite();
    let out = run(c.proob().args(["status", "--json"]));
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let report = out.json();
    assert_eq!(report["verification"]["state"], "verified");
    assert_eq!(report["config"]["in_force"], "approved");
    let rows = rows_by_id(&report);
    let word = |id: &str| rows[id]["verdict"]["word"].as_str().unwrap_or("<none>").to_string();
    assert_eq!(word(PROP_A), "holds");
    assert_eq!(word(PROP_B), "checked");
    assert_eq!(rows[PROP_B]["verdict"]["strength_pending"], true);
    assert_eq!(word(PROP_C), "not_checked");
    assert_eq!(rows[PROP_C]["verdict"]["reason"], "no_check");
    assert_eq!(word(PROP_D), "proposed");
    assert_eq!(word(PROP_E), "re_approve");
    assert_eq!(rows[PROP_E]["verdict"]["why"]["kind"], "statement_amended");
    assert_eq!(word(PROP_F), "broken");
    assert_eq!(rows[PROP_F]["verdict"]["why"]["kind"], "scope_empty");
    assert_eq!(word(PROP_G), "waived");
    // The full Status travels with each row.
    assert_eq!(rows[PROP_A]["status"]["shape"], "scoped");
    assert_eq!(rows[PROP_A]["status"]["binding"]["primary"]["state"], "fresh");
    assert_eq!(rows[PROP_A]["status"]["binding"]["score"]["outcome"]["killed"], 3);
    assert_eq!(rows[PROP_A]["status"]["decl"]["drift"]["commits"], 1);
    assert_eq!(rows[PROP_F]["status"]["shape"], "scope_empty");
    assert_eq!(rows[PROP_G]["status"]["decl"]["window"]["issue_open"], true);
    // Words and reasons in the plan's vocabulary.
    assert_eq!(rows[PROP_A]["word"], "Holds");
    assert_eq!(rows[PROP_B]["word"], "Checked (strength pending)");
    assert_eq!(rows[PROP_C]["word"], "Not checked");
    assert_eq!(rows[PROP_C]["reason"], "no check bound");
    assert_eq!(rows[PROP_G]["word"], "Waived until 2026-12-31");
    // Governance and the footer.
    assert_eq!(report["governance"]["config"]["state"], "approved");
    assert_eq!(report["footer"]["approvals_verified"], "yes");
    assert_eq!(report["footer"]["gate_required"], "yes");
    assert_eq!(report["footer"]["scheduled_evidence_age_secs"], DAY);
    assert_eq!(report["counts"]["total"], 7);
    assert_eq!(report["exit"], 0);
}

#[test]
fn status_summary_is_one_line() {
    let c = composite();
    let out = run(c.proob().args(["status", "--summary"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout.lines().count(), 1);
    assert_eq!(
        out.stdout.trim_end(),
        "proob: 1 holds, 1 checked, 1 not checked, 1 re-approve, 1 broken, 1 proposed, 1 waived"
    );
}

#[test]
fn an_unapproved_config_applies_defaults_and_says_re_approve() {
    let mut c = composite();
    // bob's review was on a config blob that later changed: the approval predates the change.
    c.scenario.blob(&c.c1.clone(), "proob/config.toml", Some(&"9".repeat(40)));
    c.fixture.write_scenario(&c.scenario.build());
    let out = run(c.proob().args(["status", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let report = out.json();
    assert_eq!(report["config"]["in_force"], "defaults");
    assert_eq!(report["config"]["why"], "unapproved");
    assert_eq!(report["amendments"][0]["kind"], "config_amended");
    assert!(report["headline"].as_str().expect("headline").contains("Re-approve (config amended)"));
    let rows = rows_by_id(&report);
    // A would hold; the governance rows demote it.
    assert_eq!(rows[PROP_A]["verdict"]["why"]["kind"], "config_amended");
}

#[test]
fn rows_beyond_the_call_budget_are_unverified_rate_limited() {
    let c = composite();
    c.repo.write(
        "proob/config.toml",
        &CONFIG_TOML.replace("schema_version = 1\n", "schema_version = 1\ncall_budget = 30\n"),
    );
    let out = run(c.proob().args(["status", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let report = out.json();
    let rows = rows_by_id(&report);
    let limited: Vec<&str> =
        rows.values().filter(|r| r["verification"] == "rate_limited").map(|r| r["id"].as_str().expect("id")).collect();
    assert!(!limited.is_empty(), "no row was rate limited: {report:#}");
    assert_eq!(rows[limited[0]]["word"], "Unverified (rate limited)");
    assert_eq!(rows[PROP_D]["verdict"]["word"], "proposed", "a local proposal needs no GitHub call");
    assert_eq!(report["footer"]["approvals_verified"], "rate_limited");
}

#[test]
fn locally_amended_property_reads_re_approve_statement_amended() {
    let c = composite();
    let path = property_path(PROP_A);
    c.repo.write(&path, &c.repo.read(&path).replace("always.", "always and forever."));
    let out = run(c.proob().args(["status", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let rows = rows_by_id(&out.json());
    assert_eq!(rows[PROP_A]["verdict"]["word"], "re_approve");
    assert_eq!(rows[PROP_A]["verdict"]["why"]["kind"], "statement_amended");
}

// ---------------------------------------------------------------------------
// check
// ---------------------------------------------------------------------------

#[test]
fn check_blocks_only_an_affected_broken_property_and_base_config_governs() {
    let k = with_pr();
    let out = run(k.check(Some(&k.results(json!({ "result": "counterexample", "witness": "f(0)" })))).arg("--json"));
    assert_eq!(out.code, 1, "stdout: {}\nstderr: {}", out.stdout, out.stderr);
    let report = out.json();
    let rows = rows_by_id(&report);
    assert_eq!(rows[PROP_A]["verdict"]["word"], "broken");
    assert_eq!(rows[PROP_A]["verdict"]["why"]["source"], "this_pr");
    assert_eq!(rows[PROP_A]["affected"], true);
    assert_eq!(rows[PROP_A]["outcome"], "block");
    assert_eq!(rows[PROP_A]["this_pr"], "counterexample");
    assert_eq!(rows[PROP_A]["reason"], "counterexample on this PR");
    // The head relaxed broken_affected to warn; the approved base config still blocks.
    assert_eq!(report["config"]["in_force"], "approved");
    // An unaffected Broken only counts in the headline.
    assert_eq!(rows[PROP_F]["affected"], false);
    assert_eq!(rows[PROP_F]["outcome"], "pass");
    assert_eq!(report["counts"]["broken"], 2);
    // Governed files in the diff get if-merged-now rows with reason and remedy.
    let rows_im = report["if_merged_now"].as_array().expect("if merged now");
    let paths: Vec<&str> = rows_im.iter().map(|r| r["path"].as_str().expect("path")).collect();
    assert!(paths.contains(&"proob/config.toml"), "{paths:?}");
    assert!(paths.contains(&property_path(PROP_H).as_str()), "{paths:?}");
    assert!(!paths.contains(&"src/lib.rs"));
    for r in rows_im {
        assert_eq!(r["remedy"], "re_approve_latest_commit", "{r:#}");
        assert_eq!(r["approval"]["reason"]["why"], "approval_predates_change");
    }
    assert_eq!(report["pull_request"], 8);
    assert_eq!(report["exit"], 1);
}

#[test]
fn check_passes_for_re_approve_unbound_after_grace_and_stale_evidence() {
    let k = with_pr();
    let out = run(k.check(None).arg("--json"));
    assert_eq!(out.code, 0, "stdout: {}\nstderr: {}", out.stdout, out.stderr);
    let rows = rows_by_id(&out.json());
    assert_eq!(rows[PROP_A]["verdict"]["reason"], "evidence_stale");
    assert_eq!(rows[PROP_A]["outcome"], "warn");
    assert_eq!(rows[PROP_C]["verdict"]["reason"], "no_check");
    assert_eq!(rows[PROP_C]["grace"], "after");
    assert_eq!(rows[PROP_C]["outcome"], "warn");
    assert_eq!(rows[PROP_E]["verdict"]["word"], "re_approve");
    assert_eq!(rows[PROP_E]["affected"], true);
    assert_eq!(rows[PROP_E]["outcome"], "warn");
    assert_eq!(rows[PROP_H]["verdict"]["word"], "proposed");
    assert_eq!(rows[PROP_B]["affected"], false);
}

#[test]
fn a_positive_result_from_this_pr_never_holds() {
    let k = with_pr();
    let out = run(k.check(Some(&k.results(json!({ "result": "no_counterexample", "budget": 256 })))).arg("--json"));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let rows = rows_by_id(&out.json());
    assert_eq!(rows[PROP_A]["this_pr"], "passed 256 examples");
    assert_ne!(rows[PROP_A]["verdict"]["word"], "holds");
    assert_eq!(rows[PROP_A]["status"]["binding"]["provisional"]["state"], "provisional");
}

#[test]
fn check_refuses_a_tree_that_is_not_at_head() {
    let k = with_pr();
    k.c.repo.checkout("main");
    let out = run(&mut k.check(None));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("check out pr first"), "{}", out.stderr);
}

#[test]
fn bootstrap_pr_exits_zero_with_the_bootstrap_summary() {
    let repo = TempRepo::new();
    repo.write("src/lib.rs", "pub fn f() {}\n");
    repo.commit("bare", NOW - 3 * DAY);
    repo.branch("pr");
    repo.write("proob/config.toml", CONFIG_TOML);
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "A is proposed", &["src/lib.rs"], "test"));
    repo.commit("introduce proob", NOW - DAY);
    let out = run(repo.proob().args(["check", "--base", "main", "--head", "pr", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let report = out.json();
    assert_eq!(report["mode"], "bootstrap");
    assert_eq!(report["verification"]["state"], "not_consulted");
    assert!(report["headline"].as_str().expect("headline").starts_with("proob gate takes effect after this PR merges"));
    assert_eq!(report["properties"][0]["word"], "Proposed");
    let text = run(repo.proob().args(["check", "--base", "main", "--head", "pr"]));
    assert_eq!(text.code, 0);
    assert!(text.stdout.contains("proob gate takes effect after this PR merges"));
}

#[test]
fn bootstrap_pr_with_an_invalid_property_exits_two() {
    let repo = TempRepo::new();
    repo.write("src/lib.rs", "pub fn f() {}\n");
    repo.commit("bare", NOW - 3 * DAY);
    repo.branch("pr");
    repo.write("proob/config.toml", CONFIG_TOML);
    repo.write(
        &property_path(PROP_A),
        &format!("{}status = \"holds\"\n", property_toml(PROP_A, "A", &["src/lib.rs"], "test")),
    );
    repo.commit("introduce proob badly", NOW - DAY);
    let out = run(repo.proob().args(["check", "--base", "main", "--head", "pr"]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("status"), "{}", out.stderr);
}

// ---------------------------------------------------------------------------
// Unverified views
// ---------------------------------------------------------------------------

fn offline_repo() -> TempRepo {
    let repo = TempRepo::new();
    repo.write("src/lib.rs", "pub fn f() {}\n");
    repo.write("proob/config.toml", CONFIG_TOML);
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "A is on main", &["src/lib.rs"], "test"));
    repo.commit("introduce proob", NOW - 3 * DAY);
    repo.write(&property_path(PROP_D), &property_toml(PROP_D, "D is proposed", &["src/lib.rs"], "test"));
    repo.add_origin();
    repo
}

#[test]
fn status_without_token_and_cache_is_unverified_and_still_lists_proposals() {
    let repo = offline_repo();
    let out = run(repo.proob().args(["status", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let report = out.json();
    assert_eq!(report["verification"]["state"], "unverified");
    assert_eq!(report["verification"]["reason"], "no_token");
    assert_eq!(report["verification"]["cache_age_secs"], Value::Null);
    let rows = rows_by_id(&report);
    assert_eq!(rows[PROP_A]["verification"], "no_token");
    assert_eq!(rows[PROP_A]["word"], "Not checked");
    assert_eq!(rows[PROP_A]["reason"], "no GitHub token");
    assert_eq!(rows[PROP_A]["verdict"], Value::Null, "no verdict is derived without facts");
    assert_eq!(rows[PROP_D]["verification"], "git_only");
    assert_eq!(rows[PROP_D]["word"], "Proposed");
    assert_eq!(report["footer"]["approvals_verified"], "no");
    let text = run(repo.proob().arg("status"));
    assert!(text.stdout.contains("[unverified: no GitHub token; no cached facts]"), "{}", text.stdout);
    let summary = run(repo.proob().args(["status", "--summary"]));
    assert_eq!(summary.stdout.lines().count(), 1);
    assert!(summary.stdout.contains("unverified"), "{}", summary.stdout);
}

#[test]
fn an_old_cache_prints_its_age_in_the_marker() {
    let repo = offline_repo();
    repo.write("proob/.cache/facts/last-write", &(NOW - 3 * DAY).to_string());
    let out = run(repo.proob().arg("status"));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("[unverified: no GitHub token; cached facts from 3 days ago]"), "{}", out.stdout);
    let report = run(repo.proob().args(["status", "--json"])).json();
    assert_eq!(report["verification"]["cache_age_secs"], 3 * DAY);
}

#[test]
fn a_repository_without_a_github_remote_is_unverified_too() {
    let repo = TempRepo::new();
    repo.write("proob/config.toml", CONFIG_TOML);
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "A", &["src/**"], "test"));
    repo.commit("introduce proob", NOW - DAY);
    let report = run(repo.proob().args(["status", "--json"])).json();
    assert_eq!(report["verification"]["reason"], "no_remote");
}

// ---------------------------------------------------------------------------
// validate, hash, run
// ---------------------------------------------------------------------------

#[test]
fn validate_passes_a_clean_repository() {
    let repo = offline_repo();
    let out = run(repo.proob().arg("validate"));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.contains("ok"), "{}", out.stdout);
}

#[test]
fn validate_fails_on_a_tracked_cache_and_names_it() {
    let repo = offline_repo();
    repo.write("proob/.cache/facts/pull/7.json", "{}");
    repo.commit("oops, cache tracked", NOW - DAY);
    let out = run(repo.proob().args(["validate", "--json"]));
    assert_eq!(out.code, 1);
    let problems = out.json()["problems"].as_array().expect("problems").clone();
    assert!(
        problems.iter().any(|p| p["file"] == "proob/.cache/facts/pull/7.json"
            && p["message"].as_str().expect("m").contains("must not be tracked")),
        "{problems:#?}"
    );
}

#[test]
fn validate_fails_on_a_secret_pattern_naming_file_and_line() {
    let repo = offline_repo();
    let path = property_path(PROP_A);
    repo.write(
        &path,
        &repo
            .read(&path)
            .replace("Because the team said so.", "Because ghp_abcdefghijklmnopqrstuvwxyz0123456789 said so."),
    );
    let out = run(repo.proob().arg("validate"));
    assert_eq!(out.code, 1, "{}", out.stdout);
    assert!(out.stdout.contains(&format!("{path}:6: looks like a secret (GitHub token)")), "{}", out.stdout);
    let aws = run(repo.proob().arg("validate").arg("--json")).json();
    assert_eq!(aws["ok"], false);
}

#[test]
fn validate_rejects_a_malformed_binding_token_and_a_schema_error() {
    let repo = offline_repo();
    repo.write(
        "tests/prop.rs",
        "// proob:01J9Z3W1V8Q4K7N2M5R8T1X4A (truncated)\n// /proob:sense is a skill, not a token\n",
    );
    repo.write(&property_path(PROP_B), "id = \"nope\"\ntitle = \"x\"\n");
    repo.commit("bad token", NOW - DAY);
    let out = run(repo.proob().arg("validate"));
    assert_eq!(out.code, 1);
    assert!(
        out.stdout.contains("tests/prop.rs:1: binding token `proob:01J9Z3W1V8Q4K7N2M5R8T1X4A` is not `proob:<ULID>`"),
        "{}",
        out.stdout
    );
    assert!(!out.stdout.contains("proob:sense"), "{}", out.stdout);
    assert!(out.stdout.contains(&format!("{}: ", property_path(PROP_B))), "{}", out.stdout);
}

#[test]
fn hash_prints_the_three_hashes() {
    let c = composite();
    let out = run(c.proob().args(["hash", PROP_A]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("content hash (proob-content-v1): "), "{}", out.stdout);
    assert!(out.stdout.contains("binding hash (proob-binding-v1): "), "{}", out.stdout);
    assert!(out.stdout.contains("scope hash (proob-scope-v1): "), "{}", out.stdout);
    assert!(out.stdout.contains("over 1 file"), "{}", out.stdout);
    let json = run(c.proob().args(["hash", PROP_A, "--json"])).json();
    assert_eq!(json["content_hash"].as_str().expect("content").len(), 64);
    assert_eq!(json["binding_hash"].as_str().expect("binding").len(), 64);
    assert_eq!(json["scope_files"], json!(["src/lib.rs"]));
    let empty = run(c.proob().args(["hash", PROP_F, "--json"])).json();
    assert_eq!(empty["scope_hash"], Value::Null);
    let unknown = run(c.proob().args(["hash", "01J9Z3W1V8Q4K7N2M5R8T1X4ZZ"]));
    assert_eq!(unknown.code, 2);
}

#[test]
fn run_is_a_closed_error_without_an_adapter() {
    let c = composite();
    let out = run(c.proob().args(["run", "check", PROP_A]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("no adapters"), "{}", out.stderr);
    // A language whose adapter has not landed yet names the unit that brings it.
    c.repo.write("proob/config.toml", &format!("{CONFIG_TOML}\n[[adapters]]\nroot = \".\"\nlanguage = \"tla\"\nrunner = \"tlc\"\nmutation_tool = \"spec-mutants\"\n"));
    let out = run(c.proob().args(["run", "score", PROP_A]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("no adapter for language tla yet (it arrives in U12)"), "{}", out.stderr);
    // The Python adapter over a tree without a tokened pytest test binds nothing.
    c.repo.write("proob/config.toml", &format!("{CONFIG_TOML}\n[[adapters]]\nroot = \".\"\nlanguage = \"python\"\nrunner = \"pytest\"\nmutation_tool = \"mutmut\"\n"));
    let out = run(c.proob().args(["run", "score", PROP_A]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("no check carries") && out.stderr.contains("# proob:"), "{}", out.stderr);
    // A Rust adapter over a tree with no Cargo.toml cannot discover anything.
    c.repo.write("proob/config.toml", &format!("{CONFIG_TOML}\n[[adapters]]\nroot = \".\"\nlanguage = \"rust\"\nrunner = \"nextest\"\nmutation_tool = \"cargo-mutants\"\n"));
    let out = run(c.proob().args(["run", "check", PROP_A]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("Cargo.toml"), "{}", out.stderr);
}

// ---------------------------------------------------------------------------
// New code without a property
// ---------------------------------------------------------------------------

const RUST_ADAPTER: &str =
    "\n[[adapters]]\nroot = \".\"\nlanguage = \"rust\"\nrunner = \"nextest\"\nmutation_tool = \"cargo-mutants\"\n";

/// main: an approved PR #7 adopts proob with a Rust adapter and A over
/// src/lib.rs, bound to tests/prop_a.rs and within its grace window, so the
/// gate passes; then a `feature` branch.
fn adopted() -> (TempRepo, Fixture) {
    let repo = TempRepo::new();
    let governed = [
        "proob/config.toml",
        ".github/workflows/proob-gate.yml",
        ".github/workflows/proob-evidence.yml",
        "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4A1.toml",
        "tests/prop_a.rs",
    ];
    repo.write("src/lib.rs", "pub fn f() {}\n");
    repo.write("tests/prop_a.rs", "// proob:01J9Z3W1V8Q4K7N2M5R8T1X4A1\n#[test] fn a() {}\n");
    repo.write("proob/config.toml", &format!("{CONFIG_TOML}{RUST_ADAPTER}"));
    repo.write(".github/workflows/proob-gate.yml", GATE_YML);
    repo.write(".github/workflows/proob-evidence.yml", EVIDENCE_YML);
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "A", &["src/lib.rs"], "test"));
    let c1 = repo.commit("adopt proob (PR #7)", NOW - 2 * DAY);
    let mut scenario = ScenarioBuilder::new();
    scenario
        .merged_pr(7, ALICE, &c1, &governed)
        .review(7, 501, BOB, "APPROVED", &c1)
        .governance(true, true, true, true);
    for path in governed {
        scenario.blob(&c1, path, Some(&repo.blob(&c1, path)));
    }
    let bindings = json!({ PROP_A: { "files": ["tests/prop_a.rs"] } });
    let fixture = Fixture::new(&scenario.build(), bindings.as_object());
    repo.branch("feature");
    (repo, fixture)
}

fn with_fixture(repo: &TempRepo, fixture: &Fixture) -> Command {
    let mut cmd = repo.proob();
    cmd.env("PROOB_FIXTURE_DIR", fixture.path());
    cmd
}

#[test]
fn check_lists_new_code_without_a_property_and_still_exits_zero() {
    let (repo, fixture) = adopted();
    repo.write("src/refunds/service.rs", "pub fn refund() {}\n");
    repo.write("src/refunds/limits.rs", "pub const MAX: u32 = 3;\n");
    repo.write("docs/refunds.md", "# Refunds\n");
    repo.write("examples/demo.rs", "fn main() {}\n");
    repo.commit("add refunds", NOW - DAY);
    let out = run(with_fixture(&repo, &fixture).args(["check", "--base", "main", "--head", "feature", "--json"]));
    assert_eq!(out.code, 0, "uncovered code alone never blocks: {}\n{}", out.stdout, out.stderr);
    let report = out.json();
    assert_eq!(
        report["uncovered"],
        json!(["src/refunds/limits.rs", "src/refunds/service.rs"]),
        "docs and examples are left out"
    );
    assert_eq!(report["exit"], 0);
    assert!(report["headline"].as_str().expect("headline").starts_with("proob gate: pass"), "{}", report["headline"]);
    let text = run(with_fixture(&repo, &fixture).args(["check", "--base", "main", "--head", "feature"]));
    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(
        text.stdout.contains("\nNew code without a property: src/refunds/limits.rs, src/refunds/service.rs · run /proob:declare on this branch to propose some\n"),
        "{}",
        text.stdout
    );
    assert_golden("check-uncovered.md", &text.stdout);
}

#[test]
fn check_caps_the_uncovered_line_at_ten_paths() {
    let (repo, fixture) = adopted();
    for i in 0..12 {
        repo.write(&format!("src/m{i:02}.rs"), "pub fn m() {}\n");
    }
    repo.commit("add twelve modules", NOW - DAY);
    let json =
        run(with_fixture(&repo, &fixture).args(["check", "--base", "main", "--head", "feature", "--json"])).json();
    assert_eq!(json["uncovered"].as_array().expect("uncovered").len(), 12, "the JSON lists every path");
    let text = run(with_fixture(&repo, &fixture).args(["check", "--base", "main", "--head", "feature"]));
    assert_eq!(text.code, 0);
    assert!(text.stdout.contains("src/m09.rs and 2 more · run /proob:declare"), "{}", text.stdout);
    assert!(!text.stdout.contains("src/m10.rs"), "{}", text.stdout);
}

#[test]
fn check_has_no_uncovered_line_when_a_property_covers_the_new_code() {
    let (repo, fixture) = adopted();
    repo.write("src/refunds.rs", "pub fn refund() {}\n");
    repo.write(
        &property_path(PROP_H),
        &property_toml(PROP_H, "Refunds never exceed the charge", &["src/refunds.rs"], "test"),
    );
    repo.commit("add refunds and a property for them", NOW - DAY);
    let out = run(with_fixture(&repo, &fixture).args(["check", "--base", "main", "--head", "feature", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.json()["uncovered"], json!([]));
    let text = run(with_fixture(&repo, &fixture).args(["check", "--base", "main", "--head", "feature"]));
    assert!(!text.stdout.contains("New code without a property"), "{}", text.stdout);
}

#[test]
fn check_honours_uncovered_ignore_from_the_head_config() {
    let (repo, fixture) = adopted();
    repo.write("src/generated/api.rs", "pub fn api() {}\n");
    repo.write("proob/config.toml", &format!("uncovered_ignore = [\"src/generated/**\"]\n{CONFIG_TOML}{RUST_ADAPTER}"));
    repo.commit("generated code", NOW - DAY);
    let out = run(with_fixture(&repo, &fixture).args(["check", "--base", "main", "--head", "feature", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.json()["uncovered"], json!([]));
}

#[test]
fn status_json_lists_uncovered_work_including_uncommitted_files_and_text_is_unchanged() {
    let (repo, fixture) = adopted();
    repo.write("src/refunds.rs", "pub fn refund() {}\n");
    repo.commit("add refunds", NOW - DAY);
    repo.write("src/limits.rs", "pub const MAX: u32 = 3;\n");
    repo.write("src/gone.rs", "x\n");
    let out = run(with_fixture(&repo, &fixture).args(["status", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.json()["uncovered"], json!(["src/gone.rs", "src/limits.rs", "src/refunds.rs"]));
    repo.remove("src/gone.rs");
    let text = run(with_fixture(&repo, &fixture).arg("status"));
    assert!(!text.stdout.contains("without a property"), "{}", text.stdout);
    repo.checkout("main");
    let on_main = run(with_fixture(&repo, &fixture).args(["status", "--json"])).json();
    assert_eq!(on_main["uncovered"], json!(["src/limits.rs"]), "the untracked file follows the checkout");
}
