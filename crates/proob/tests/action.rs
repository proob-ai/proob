//! `proob action`: the execute and judge jobs of both modes, driven by a
//! fixture Actions environment (event payload, run id, commit, summary and
//! output files) over the same fixture repositories the CLI tests use.
//!
//! The judge never runs repository code: every judge here runs with a PATH
//! that holds only `git`, including the one over the real Rust fixture whose
//! bindings come from the adapter's source scan.
mod common;

use common::scenarios::*;
use common::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const REPOSITORY: &str = "acme/widgets";
const RUN_ID: u64 = 4242;
const GATE_WORKFLOW: &str = ".github/workflows/proob-gate.yml";
const EVIDENCE_WORKFLOW: &str = ".github/workflows/proob-evidence.yml";

// ---------------------------------------------------------------------------
// A fixture Actions environment
// ---------------------------------------------------------------------------

struct Actions {
    dir: TempDir,
    vars: Vec<(&'static str, String)>,
}

impl Actions {
    fn new(event: &str, payload: Value, sha: &str, git_ref: &str, run_id: u64, workflow: &str) -> Actions {
        let dir = tempfile::tempdir().expect("tempdir");
        let event_path = dir.path().join("event.json");
        std::fs::write(&event_path, payload.to_string()).expect("event payload");
        let s = |p: &Path| p.display().to_string();
        let vars = vec![
            ("GITHUB_ACTIONS", "true".to_string()),
            ("GITHUB_EVENT_NAME", event.to_string()),
            ("GITHUB_EVENT_PATH", s(&event_path)),
            ("GITHUB_REPOSITORY", REPOSITORY.to_string()),
            ("GITHUB_SHA", sha.to_string()),
            ("GITHUB_REF", git_ref.to_string()),
            ("GITHUB_RUN_ID", run_id.to_string()),
            ("GITHUB_RUN_ATTEMPT", "1".to_string()),
            ("GITHUB_WORKFLOW_REF", format!("{REPOSITORY}/{workflow}@{git_ref}")),
            ("GITHUB_STEP_SUMMARY", s(&dir.path().join("summary.md"))),
            ("GITHUB_OUTPUT", s(&dir.path().join("output.txt"))),
            ("RUNNER_TEMP", s(dir.path())),
        ];
        Actions { dir, vars }
    }

    /// A `pull_request` event: PR `number` from `pr` into `main`, the checkout at `head_sha`.
    fn pull_request(number: u64, base_sha: &str, head_sha: &str, run_id: u64) -> Actions {
        let payload = json!({
            "action": "synchronize", "number": number,
            "pull_request": { "number": number, "base": { "ref": "main", "sha": base_sha }, "head": { "ref": "pr", "sha": head_sha } },
            "repository": { "full_name": REPOSITORY, "default_branch": "main" }
        });
        let mut a = Actions::new(
            "pull_request",
            payload,
            head_sha,
            &format!("refs/pull/{number}/merge"),
            run_id,
            GATE_WORKFLOW,
        );
        a.vars.push(("GITHUB_BASE_REF", "main".to_string()));
        a.vars.push(("GITHUB_HEAD_REF", "pr".to_string()));
        a
    }

    fn default_branch_event(event: &str, sha: &str, run_id: u64) -> Actions {
        let payload = json!({ "ref": "refs/heads/main", "after": sha, "repository": { "full_name": REPOSITORY, "default_branch": "main" } });
        Actions::new(event, payload, sha, "refs/heads/main", run_id, EVIDENCE_WORKFLOW)
    }

    fn push(sha: &str, run_id: u64) -> Actions {
        Actions::default_branch_event("push", sha, run_id)
    }

    fn schedule(sha: &str, run_id: u64) -> Actions {
        Actions::default_branch_event("schedule", sha, run_id)
    }

    fn var(mut self, name: &'static str, value: &str) -> Actions {
        self.vars.retain(|(k, _)| *k != name);
        self.vars.push((name, value.to_string()));
        self
    }

    fn without(mut self, name: &str) -> Actions {
        self.vars.retain(|(k, _)| *k != name);
        self
    }

    fn apply<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        for (k, v) in &self.vars {
            cmd.env(k, v);
        }
        cmd
    }

    fn summary(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("summary.md")).unwrap_or_default()
    }

    /// `name=value` lines of GITHUB_OUTPUT (every value written here is one line).
    fn outputs(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(self.dir.path().join("output.txt"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect()
    }
}

fn action(cmd: &mut Command, mode: &str, job: &str) -> Run {
    run(cmd.args(["action", "--mode", mode, "--job", job]))
}

fn rows_by_id(report: &Value) -> BTreeMap<String, Value> {
    report["properties"]
        .as_array()
        .expect("properties")
        .iter()
        .map(|r| (r["id"].as_str().expect("id").to_string(), r.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// The raw results artifact, as the execute job leaves it
// ---------------------------------------------------------------------------

fn manifest(run_id: u64, head_sha: &str, mode: &str, event: &str, attempted: &[(&str, &str, &str)]) -> Value {
    let attempted: Vec<Value> =
        attempted.iter().map(|(id, what, outcome)| json!({ "id": id, "what": what, "outcome": outcome })).collect();
    json!({
        "schema_version": 1, "mode": mode, "event": event, "repository": REPOSITORY,
        "run_id": run_id, "run_attempt": 1, "head_sha": head_sha, "created_at": NOW,
        "attempted": attempted, "skipped": [], "notes": []
    })
}

fn write_artifact(repo: &TempRepo, manifest: &Value, lines: &str) {
    repo.write("proob/.cache/results/manifest.json", &manifest.to_string());
    repo.write("proob/.cache/results/results.jsonl", lines);
}

fn results_dir(repo: &TempRepo) -> PathBuf {
    repo.root().join("proob/.cache/results")
}

/// `with_pr` plus this PR's result for A, under a manifest naming `run_id` at `head`.
fn pr_with_result(outcome: Value, run_id: u64, head_override: Option<&str>) -> (Checked, Actions) {
    let k = with_pr();
    let line = k.results(outcome.clone());
    let head = head_override.map_or(k.c4.clone(), str::to_string);
    write_artifact(
        &k.c.repo,
        &manifest(
            run_id,
            &head,
            "check",
            "pull_request",
            &[(PROP_A, "check", outcome["result"].as_str().expect("result"))],
        ),
        &line,
    );
    let actions = Actions::pull_request(8, &k.c.c2, &k.c4, RUN_ID);
    (k, actions)
}

// ---------------------------------------------------------------------------
// check / judge
// ---------------------------------------------------------------------------

#[test]
fn check_judge_blocks_an_affected_counterexample_with_summary_annotations_and_outputs() {
    let (k, actions) = pr_with_result(json!({ "result": "counterexample", "witness": "f(0)" }), RUN_ID, None);
    let out = action(actions.apply(&mut k.c.proob()), "check", "judge");
    assert_eq!(out.code, 1, "stdout: {}\nstderr: {}", out.stdout, out.stderr);
    let row = format!("| A holds `{PROP_A}` | Broken **blocks** | _counterexample on this PR_ | counterexample |");
    assert!(out.stdout.contains(&row), "{}", out.stdout);
    assert!(out.stdout.contains("::notice title=proob::proob gate: block (4 affected of 8)"), "{}", out.stdout);
    assert!(
        out.stdout.contains(&format!(
            "::error file={},title=proob gate::A holds: Broken (counterexample on this PR); this PR: counterexample",
            property_path(PROP_A)
        )),
        "{}",
        out.stdout
    );
    let summary = actions.summary();
    assert!(summary.starts_with("## proob gate: block (4 affected of 8)"), "{summary}");
    assert!(summary.contains(&row), "{summary}");
    let outputs = actions.outputs();
    assert_eq!(outputs["verdict"], "block");
    assert_eq!(outputs["blocked"], "true");
    assert_eq!(outputs["mode"], "check");
    assert_eq!(outputs["affected"], "4");
    assert_eq!(outputs["broken"], "2");
    assert!(outputs["headline"].starts_with("proob gate: block"), "{outputs:?}");

    // JSON mode: the same report; the head relaxed the gate, the approved base config still blocks.
    let json = action(actions.apply(k.c.proob().arg("--json")), "check", "judge");
    assert_eq!(json.code, 1, "{}", json.stderr);
    let report = json.json();
    assert_eq!(report["config"]["in_force"], "approved");
    let rows = rows_by_id(&report);
    assert_eq!(rows[PROP_A]["verdict"]["why"]["source"], "this_pr");
    assert_eq!(rows[PROP_A]["outcome"], "block");
    assert!(!json.stdout.contains("::error"), "no annotations in JSON mode");
}

#[test]
fn check_judge_blocks_a_runner_error_on_an_affected_property() {
    let (k, actions) = pr_with_result(json!({ "result": "errored", "reason": "nextest did not run" }), RUN_ID, None);
    let out = action(actions.apply(k.c.proob().arg("--json")), "check", "judge");
    assert_eq!(out.code, 1, "{}", out.stderr);
    let rows = rows_by_id(&out.json());
    assert_eq!(rows[PROP_A]["verdict"]["why"]["kind"], "runner_errored");
    assert_eq!(rows[PROP_A]["reason"], "runner error on this PR");
    assert_eq!(rows[PROP_A]["this_pr"], "runner error");
    assert_eq!(actions.outputs()["verdict"], "block");
}

#[test]
fn check_judge_passes_an_unaffected_default_branch_failure_and_counts_it_in_the_headline() {
    let mut k = with_pr();
    // A newer push run on main found a counterexample for B, whose scope this PR does not touch.
    let b = hashes_of(&k.c.repo, &k.c.fixture, PROP_B);
    let blob = k.c.repo.blob("main", EVIDENCE_WORKFLOW);
    let c2 = k.c.c2.clone();
    let meta = RunMeta { run_id: 102, head_sha: &c2, workflow_blob: &blob, at: NOW - DAY / 2 };
    let records = vec![record(PROP_B, "check", &b, json!({ "result": "counterexample", "witness": "g(7)" }), &meta)];
    k.c.scenario.evidence_run(102, &c2, "push", "2026-10-27T12:00:00Z", records);
    k.c.fixture.write_scenario(&k.c.scenario.build());
    let actions = Actions::pull_request(8, &k.c.c2, &k.c4, RUN_ID);
    let out = action(actions.apply(k.c.proob().arg("--json")), "check", "judge");
    assert_eq!(out.code, 0, "{}", out.stderr);
    let report = out.json();
    let rows = rows_by_id(&report);
    assert_eq!(rows[PROP_B]["verdict"]["word"], "broken");
    assert_eq!(rows[PROP_B]["verdict"]["why"]["source"], "default_branch");
    assert_eq!(rows[PROP_B]["affected"], false);
    assert_eq!(rows[PROP_B]["outcome"], "pass");
    assert_eq!(rows[PROP_F]["verdict"]["why"]["kind"], "scope_empty");
    assert_eq!(report["counts"]["broken"], 2);
    assert!(report["headline"].as_str().expect("headline").contains("2 broken"), "{}", report["headline"]);
    assert_eq!(actions.outputs()["verdict"], "warn", "re-approve and stale evidence warn; nothing blocks");
    let notes = report["notes"].as_array().expect("notes");
    assert!(
        notes.iter().any(|n| n.as_str().expect("note").contains("no raw results under proob/.cache/results")),
        "{notes:?}"
    );
}

#[test]
fn check_judge_passes_drift_re_approve_and_unbound_after_grace_alone() {
    let k = with_pr();
    let actions = Actions::pull_request(8, &k.c.c2, &k.c4, RUN_ID);
    let out = action(actions.apply(k.c.proob().arg("--json")), "check", "judge");
    assert_eq!(out.code, 0, "{}", out.stderr);
    let rows = rows_by_id(&out.json());
    assert_eq!(rows[PROP_E]["verdict"]["word"], "re_approve");
    assert_eq!(rows[PROP_E]["outcome"], "warn");
    assert_eq!(rows[PROP_C]["verdict"]["reason"], "no_check");
    assert_eq!(rows[PROP_C]["grace"], "after");
    assert_eq!(rows[PROP_A]["status"]["decl"]["drift"]["commits"], 1, "drift is counted, never gated");
    assert_eq!(rows[PROP_A]["verdict"]["reason"], "evidence_stale");
    let outputs = actions.outputs();
    assert_eq!(outputs["verdict"], "warn");
    assert_eq!(outputs["blocked"], "false");
}

#[test]
fn check_judge_is_silent_about_an_unbound_property_within_its_grace_window() {
    // main: PR #7 (approved by bob two days ago) introduced proob and P, which binds no check.
    let repo = TempRepo::new();
    repo.write("src/lib.rs", "pub fn f() {}\n");
    repo.write("proob/config.toml", CONFIG_TOML);
    repo.write(GATE_WORKFLOW, GATE_YML);
    repo.write(EVIDENCE_WORKFLOW, EVIDENCE_YML);
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "P has no check yet", &["src/lib.rs"], "test"));
    let c1 = repo.commit("introduce proob (PR #7)", NOW - 2 * DAY);
    repo.branch("pr");
    repo.write("src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let c2 = repo.commit("touch lib", NOW - DAY);
    let governed = ["proob/config.toml", GATE_WORKFLOW, EVIDENCE_WORKFLOW, &property_path(PROP_A)];
    let mut scenario = ScenarioBuilder::new();
    scenario
        .merged_pr(7, ALICE, &c1, &governed)
        .review(7, 501, BOB, "APPROVED", &c1)
        .governance(true, true, true, true);
    for path in governed {
        scenario.blob(&c1, path, Some(&repo.blob(&c1, path)));
    }
    scenario.open_pr(8, ALICE, &c2).pr_commit(8, ALICE, &c2, &["src/lib.rs"], &[&c1]);
    // An adapter that found no binding: the property is unbound, not adapterless.
    let fixture = Fixture::new(&scenario.build(), Some(&serde_json::Map::new()));
    let actions = Actions::pull_request(8, &c1, &c2, RUN_ID);
    let out =
        action(actions.apply(repo.proob().env("PROOB_FIXTURE_DIR", fixture.path()).arg("--json")), "check", "judge");
    assert_eq!(out.code, 0, "{}", out.stderr);
    let rows = rows_by_id(&out.json());
    assert_eq!(rows[PROP_A]["affected"], true);
    assert_eq!(rows[PROP_A]["verdict"]["reason"], "no_check");
    assert_eq!(rows[PROP_A]["grace"], "within");
    assert_eq!(rows[PROP_A]["outcome"], "pass");
    assert_eq!(actions.outputs()["verdict"], "pass");
}

#[test]
fn check_judge_applies_defaults_and_says_re_approve_when_the_base_config_is_unapproved() {
    let (mut k, actions) = pr_with_result(json!({ "result": "counterexample", "witness": "f(0)" }), RUN_ID, None);
    // bob's review was on a config blob that later changed: the base config is not approved.
    k.c.scenario.blob(&k.c.c1.clone(), "proob/config.toml", Some(&"9".repeat(40)));
    k.c.fixture.write_scenario(&k.c.scenario.build());
    let out = action(actions.apply(k.c.proob().arg("--json")), "check", "judge");
    assert_eq!(out.code, 1, "the defaults still block an affected counterexample: {}", out.stderr);
    let report = out.json();
    assert_eq!(report["config"]["in_force"], "defaults");
    assert_eq!(report["config"]["why"], "unapproved");
    assert!(
        report["headline"].as_str().expect("headline").contains("Re-approve (config amended)"),
        "{}",
        report["headline"]
    );
    assert!(actions.summary().contains("Re-approve (config amended)"));
}

#[test]
fn check_judge_renders_the_bootstrap_summary_and_exits_zero() {
    let repo = TempRepo::new();
    repo.write("src/lib.rs", "pub fn f() {}\n");
    let base = repo.commit("bare", NOW - 3 * DAY);
    repo.branch("pr");
    repo.write("proob/config.toml", CONFIG_TOML);
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "A is proposed", &["src/lib.rs"], "test"));
    let head = repo.commit("introduce proob", NOW - DAY);
    let actions = Actions::pull_request(8, &base, &head, RUN_ID);
    // The execute job of a bootstrap PR runs nothing and says why.
    let exec = action(actions.apply(&mut repo.proob()), "check", "execute");
    assert_eq!(exec.code, 0, "{}", exec.stderr);
    let m: Value = serde_json::from_str(&repo.read("proob/.cache/results/manifest.json")).expect("manifest");
    assert_eq!(m["attempted"], json!([]));
    assert!(m["notes"][0].as_str().expect("note").starts_with("bootstrap:"), "{m}");
    let out = action(actions.apply(&mut repo.proob()), "check", "judge");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout.contains("::notice title=proob::proob gate takes effect after this PR merges"),
        "{}",
        out.stdout
    );
    // The summary file is appended to: execute's block, then the judge's.
    assert!(actions.summary().contains("\n## proob gate takes effect after this PR merges"), "{}", actions.summary());
    let outputs = actions.outputs();
    assert_eq!(outputs["mode"], "bootstrap");
    assert_eq!(outputs["verdict"], "pass");
    assert_eq!(outputs["proposed"], "1");
}

#[test]
fn check_judge_if_merged_now_rows_name_reason_and_remedy_including_an_edited_evidence_workflow() {
    let mut k = with_pr();
    // A third commit edits the evidence workflow; bob's approval at c3 predates it.
    k.c.repo.write(EVIDENCE_WORKFLOW, &format!("{EVIDENCE_YML}# edited\n"));
    let c5 = k.c.repo.commit("edit the evidence workflow", NOW - DAY / 2);
    let h = property_path(PROP_H);
    let c3 = k.c3.clone();
    k.c.scenario.open_pr(8, ALICE, &c5).pr_commit(8, ALICE, &c5, &[EVIDENCE_WORKFLOW], &[&k.c4.clone()]);
    for path in [h.as_str(), "proob/config.toml", EVIDENCE_WORKFLOW] {
        k.c.scenario.blob(&c5, path, Some(&k.c.repo.blob(&c5, path)));
    }
    k.c.scenario.blob(&c3, EVIDENCE_WORKFLOW, Some(&k.c.repo.blob(&c3, EVIDENCE_WORKFLOW)));
    k.c.fixture.write_scenario(&k.c.scenario.build());
    let actions = Actions::pull_request(8, &k.c.c2, &c5, RUN_ID);
    let out = action(actions.apply(&mut k.c.proob()), "check", "judge");
    assert_eq!(out.code, 0, "{}", out.stderr);
    let summary = actions.summary();
    assert!(summary.contains("**If merged now** (governed files in this change)"), "{summary}");
    for path in [h.as_str(), "proob/config.toml", EVIDENCE_WORKFLOW] {
        let row = format!(
            "| `{path}` | unreviewed: the approval predates the file's last change | re-approve the latest commit |"
        );
        assert!(summary.contains(&row), "missing row for {path}:\n{summary}");
    }
    let report = action(actions.apply(k.c.proob().arg("--json")), "check", "judge").json();
    let rows = rows_by_id(&report);
    // The PR's own workflow edit changes nothing about evidence: it is read from main's approved blob.
    assert_ne!(rows[PROP_A]["status"]["binding"]["primary"]["state"], "fresh");
    assert_eq!(report["governance"]["evidence_workflow"]["state"], "approved", "main's workflow stays approved");
}

#[test]
fn check_judge_rejects_a_results_artifact_from_another_run() {
    // Another run id.
    let (k, actions) = pr_with_result(json!({ "result": "counterexample", "witness": "f(0)" }), 999, None);
    let out = action(actions.apply(&mut k.c.proob()), "check", "judge");
    assert_eq!(out.code, 0, "the foreign counterexample never reaches derive: {}", out.stderr);
    assert!(out.stdout.contains("raw results from another run were ignored"), "{}", out.stdout);
    assert!(
        out.stdout.contains(&format!("| A holds `{PROP_A}` | Not checked (warn) | _evidence stale_ | not run |")),
        "{}",
        out.stdout
    );
    // The right run id but another commit.
    let other = "a".repeat(40);
    let (k, actions) = pr_with_result(json!({ "result": "counterexample", "witness": "f(0)" }), RUN_ID, Some(&other));
    let report = action(actions.apply(k.c.proob().arg("--json")), "check", "judge").json();
    assert_eq!(rows_by_id(&report)[PROP_A]["this_pr"], "not run");
    assert!(
        report["notes"].as_array().expect("notes").iter().any(|n| n.as_str().expect("note").contains("another run")),
        "{}",
        report["notes"]
    );
    // Results without a manifest are not this run's either.
    let k = with_pr();
    k.c.repo.write(
        "proob/.cache/results/results.jsonl",
        &k.results(json!({ "result": "counterexample", "witness": "f(0)" })),
    );
    let actions = Actions::pull_request(8, &k.c.c2, &k.c4, RUN_ID);
    let report = action(actions.apply(k.c.proob().arg("--json")), "check", "judge").json();
    assert_eq!(rows_by_id(&report)[PROP_A]["this_pr"], "not run");
    assert!(
        report["notes"]
            .as_array()
            .expect("notes")
            .iter()
            .any(|n| n.as_str().expect("note").contains("without a manifest.json")),
        "{}",
        report["notes"]
    );
}

// ---------------------------------------------------------------------------
// parity
// ---------------------------------------------------------------------------

#[test]
fn parity_the_judge_and_the_cli_print_the_same_json_from_the_same_inputs() {
    // Evidence judge against `status --json` on the composite fixture.
    let c = composite();
    let status = run(c.proob().args(["status", "--json"])).json();
    let actions = Actions::push(&c.c2, RUN_ID);
    let exec = action(actions.apply(&mut c.proob()), "evidence", "execute");
    assert_eq!(exec.code, 0, "{}", exec.stderr);
    let judge = action(actions.apply(c.proob().arg("--json")), "evidence", "judge");
    assert_eq!(judge.code, 0, "{}", judge.stderr);
    assert_eq!(judge.json(), status, "the session view and the CI view are one function");

    // Check judge against `check --json` with the same base, head, PR, and results.
    let (k, actions) = pr_with_result(json!({ "result": "counterexample", "witness": "f(0)" }), RUN_ID, None);
    let results = results_dir(&k.c.repo).join("results.jsonl");
    let mut cli = k.c.proob();
    cli.args(["check", "--base", &k.c.c2, "--head", &k.c4, "--pr", "8", "--results"]).arg(&results).arg("--json");
    let check = run(&mut cli).json();
    let judge = action(actions.apply(k.c.proob().arg("--json")), "check", "judge").json();
    assert_eq!(judge, check);
}

// ---------------------------------------------------------------------------
// The Rust fixture: real adapter runs in execute, none in judge
// ---------------------------------------------------------------------------

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

const RA: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHA";
const RB: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHB";
const RC: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHC";
const RD: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHD";

impl RustRepo {
    /// The fixture plus both workflow files, committed to main; a GitHub that
    /// knows no pull requests, so every property is Proposed.
    fn new(keep: &[&str]) -> RustRepo {
        let repo = TempRepo::new();
        copy_tree(&fixture_dir(), repo.root());
        for id in [RA, RB, RC, RD] {
            if !keep.contains(&id) {
                repo.remove(&property_path(id));
            }
        }
        repo.write(GATE_WORKFLOW, GATE_YML);
        repo.write(EVIDENCE_WORKFLOW, EVIDENCE_YML);
        repo.commit("the rust fixture", NOW - 10 * DAY);
        let scenario = ScenarioBuilder::new().governance(true, true, true, true).build();
        RustRepo { repo, fixture: Fixture::new(&scenario, None) }
    }

    /// The binary with the sandbox PATH (git only): what a judge gets.
    fn proob(&self) -> Command {
        let mut cmd = self.repo.proob();
        cmd.env("PROOB_FIXTURE_DIR", self.fixture.path());
        cmd
    }

    /// The binary with the real PATH and a shared build directory: what an execute job gets.
    fn proob_with_tools(&self) -> Command {
        let mut cmd = self.proob();
        cmd.env("PATH", std::env::var_os("PATH").expect("PATH"));
        cmd.env("CARGO_TARGET_DIR", fixture_dir().join("target"));
        cmd
    }

    fn results(&self) -> Vec<Value> {
        self.repo
            .read("proob/.cache/results/results.jsonl")
            .lines()
            .map(|l| serde_json::from_str(l).expect("record"))
            .collect()
    }

    fn manifest(&self) -> Value {
        serde_json::from_str(&self.repo.read("proob/.cache/results/manifest.json")).expect("manifest")
    }

    fn hashes(&self, id: &str) -> Value {
        run(self.proob().args(["hash", id, "--json"])).json()
    }
}

#[test]
fn evidence_execute_on_push_writes_records_at_the_current_hashes_and_judge_repackages_them_without_cargo() {
    let r = RustRepo::new(&[RA, RB, RC, RD]);
    let head = r.repo.rev("HEAD");
    let actions = Actions::push(&head, RUN_ID);
    let exec = action(actions.apply(&mut r.proob_with_tools()), "evidence", "execute");
    assert_eq!(exec.code, 0, "{}\n{}", exec.stdout, exec.stderr);
    let records = r.results();
    let by_id: BTreeMap<&str, &Value> = records.iter().map(|v| (v["id"].as_str().expect("id"), v)).collect();
    assert_eq!(by_id.keys().copied().collect::<Vec<_>>(), vec![RA, RB, RD], "every bound property, once");
    for (id, record) in &by_id {
        let h = r.hashes(id);
        assert_eq!(record["kind"], "check");
        assert_eq!(record["content_hash"], h["content_hash"], "{id}");
        assert_eq!(record["binding_hash"], h["binding_hash"], "{id}");
        assert_eq!(record["scope_hash"], h["scope_hash"], "{id}");
        assert_eq!(record["head_sha"], head, "{id}");
        assert!(record.get("run_id").is_none(), "a raw result carries no provenance");
    }
    assert_eq!(by_id[RA]["outcome"]["result"], "no_counterexample");
    assert_eq!(by_id[RB]["outcome"]["result"], "counterexample");
    assert_eq!(by_id[RA]["replay"]["cases"], 256, "the push run uses the adapter's default budget");
    let m = r.manifest();
    assert_eq!(m["run_id"], RUN_ID);
    assert_eq!(m["head_sha"], head);
    assert_eq!(m["event"], "push");
    assert_eq!(m["mode"], "evidence");
    assert_eq!(m["attempted"].as_array().expect("attempted").len(), 3);
    assert_eq!(m["skipped"], json!([]));
    assert_eq!(actions.outputs()["attempted"], "3");

    // The judge: PATH holds only git, bindings come from the adapter's source scan.
    let judge = action(actions.apply(&mut r.proob()), "evidence", "judge");
    assert_eq!(judge.code, 0, "{}\n{}", judge.stdout, judge.stderr);
    let evidence = r.repo.read("proob/.cache/evidence/evidence.jsonl");
    let lines: Vec<Value> = evidence.lines().map(|l| serde_json::from_str(l).expect("evidence line")).collect();
    assert_eq!(lines.len(), 3);
    let blob = r.repo.blob("HEAD", EVIDENCE_WORKFLOW);
    for line in &lines {
        proob_facts::evidence::parse_evidence(&line.to_string()).expect("a well-formed evidence record");
        assert_eq!(line["run_id"], RUN_ID);
        assert_eq!(line["head_sha"], head);
        assert_eq!(line["workflow_blob"], blob);
        assert_eq!(line["at"], NOW);
        assert_eq!(line["schema_version"], 1);
    }
    let a = lines.iter().find(|l| l["id"] == RA).expect("A");
    assert_eq!(a["content_hash"], by_id[RA]["content_hash"]);
    assert_eq!(a["tool_versions"], by_id[RA]["tool_versions"]);
    let summary = actions.summary();
    assert!(
        summary.contains(&format!("negative outcome on the default branch: `{RB}` check counterexample")),
        "{summary}"
    );
    assert!(summary.contains("3 records packaged for run 4242"), "{summary}");
    assert!(
        summary.contains("| Property | Status | Reason | This PR |"),
        "the status of the default branch follows:\n{summary}"
    );
    assert!(
        judge.stdout.contains("::warning title=proob evidence::negative outcome on the default branch"),
        "{}",
        judge.stdout
    );
    let outputs = actions.outputs();
    assert_eq!(outputs["records"], "3");
    assert_eq!(outputs["negative"], "1");
    assert_eq!(outputs["mode"], "status");
}

#[test]
fn evidence_execute_on_schedule_adds_scores_and_uses_the_large_budget() {
    let r = RustRepo::new(&[RD]);
    let head = r.repo.rev("HEAD");
    let actions = Actions::schedule(&head, RUN_ID);
    let exec = action(actions.apply(&mut r.proob_with_tools()), "evidence", "execute");
    assert_eq!(exec.code, 0, "{}\n{}", exec.stdout, exec.stderr);
    let records = r.results();
    assert_eq!(records.len(), 2, "{records:?}");
    assert_eq!(records[0]["kind"], "check");
    assert_eq!(records[0]["outcome"], json!({ "result": "no_counterexample", "budget": 2000 }));
    assert_eq!(records[0]["replay"]["cases"], 2000, "the scheduled run uses the large budget");
    assert_eq!(records[1]["kind"], "score");
    assert_eq!(records[1]["outcome"]["result"], "scored");
    assert!(records[1]["tool_versions"]["cargo-mutants"].is_string(), "{}", records[1]);
    let m = r.manifest();
    assert_eq!(m["event"], "schedule");
    assert_eq!(
        m["attempted"],
        json!([{ "id": RD, "what": "check", "outcome": "no_counterexample" }, { "id": RD, "what": "score", "outcome": "scored" }])
    );
}

#[test]
fn check_execute_runs_exactly_the_affected_bound_properties() {
    let r = RustRepo::new(&[RA, RB, RC, RD]);
    let base = r.repo.rev("HEAD");
    r.repo.branch("pr");
    r.repo.write("src/ordering.rs", &format!("{}\n// touched\n", r.repo.read("src/ordering.rs")));
    let head = r.repo.commit("touch ordering", NOW - DAY);
    let actions = Actions::pull_request(8, &base, &head, RUN_ID);
    let exec = action(actions.apply(&mut r.proob_with_tools()), "check", "execute");
    assert_eq!(exec.code, 0, "{}\n{}", exec.stdout, exec.stderr);
    let records = r.results();
    assert_eq!(records.len(), 1, "only A's scope changed: {records:?}");
    assert_eq!(records[0]["id"], RA);
    let m = r.manifest();
    assert_eq!(m["pull_request"], 8);
    assert_eq!(m["base_sha"], base);
    assert_eq!(m["notes"][0], "1 changed path affects 1 of 4 properties; 1 of those bind a check");
    // The judge reads it back without cargo and shows the column.
    let judge = action(actions.apply(r.proob().arg("--json")), "check", "judge");
    assert_eq!(judge.code, 0, "{}\n{}", judge.stdout, judge.stderr);
    let rows = rows_by_id(&judge.json());
    assert_eq!(rows[RA]["this_pr"], "passed 256 examples");
    assert_eq!(rows[RA]["affected"], true);
    assert_eq!(rows[RB]["affected"], false);
}

// ---------------------------------------------------------------------------
// Refusals and the closed environment
// ---------------------------------------------------------------------------

#[test]
fn an_execute_job_with_a_token_in_its_environment_fails_with_one_line() {
    let k = with_pr();
    for name in ["GITHUB_TOKEN", "GH_TOKEN"] {
        let actions = Actions::pull_request(8, &k.c.c2, &k.c4, RUN_ID).var(name, "ghp_notatoken");
        let out = action(actions.apply(&mut k.c.proob()), "check", "execute");
        assert_eq!(out.code, 2, "{name}: {}", out.stdout);
        assert_eq!(out.stderr.lines().count(), 1, "{}", out.stderr);
        assert!(out.stderr.contains(&format!("must hold no token, but {name} is set")), "{}", out.stderr);
        assert!(!out.stderr.contains("notatoken"), "the value is never printed");
        assert!(!results_dir(&k.c.repo).exists(), "nothing ran");
        // The judge takes it.
        let out = action(actions.apply(&mut k.c.proob()), "check", "judge");
        assert_eq!(out.code, 0, "{}", out.stderr);
    }
}

#[test]
fn the_environment_is_closed_and_every_gap_is_named() {
    let k = with_pr();
    let pr = || Actions::pull_request(8, &k.c.c2, &k.c4, RUN_ID);
    let out = action(pr().without("GITHUB_RUN_ID").apply(&mut k.c.proob()), "check", "judge");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("GITHUB_RUN_ID is not set"), "{}", out.stderr);
    let out = action(pr().var("GITHUB_EVENT_NAME", "pull_request_target").apply(&mut k.c.proob()), "check", "judge");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("refuses pull_request_target"), "{}", out.stderr);
    let out = action(pr().var("GITHUB_EVENT_NAME", "push").apply(&mut k.c.proob()), "check", "judge");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("mode check judges pull requests"), "{}", out.stderr);
    let out = action(pr().apply(&mut k.c.proob()), "evidence", "judge");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("mode evidence runs on push, schedule, or workflow_dispatch"), "{}", out.stderr);
    let out = action(pr().var("GITHUB_SHA", &k.c3).apply(&mut k.c.proob()), "check", "judge");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("the checkout is at"), "{}", out.stderr);
    let out = action(pr().var("GITHUB_WORKFLOW_REF", "nope").apply(&mut k.c.proob()), "check", "judge");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("GITHUB_WORKFLOW_REF"), "{}", out.stderr);
}

#[test]
fn evidence_judge_refuses_results_that_are_not_this_runs() {
    let c = composite();
    let actions = Actions::push(&c.c2, RUN_ID);
    write_artifact(&c.repo, &manifest(999, &c.c2, "evidence", "push", &[]), "");
    let out = action(actions.apply(&mut c.proob()), "evidence", "judge");
    assert_eq!(out.code, 2, "{}", out.stdout);
    assert!(out.stderr.contains("another run") && out.stderr.contains("no evidence is produced"), "{}", out.stderr);
    assert!(!c.repo.root().join("proob/.cache/evidence/evidence.jsonl").exists());
    std::fs::remove_dir_all(results_dir(&c.repo)).expect("clear");
    let out = action(actions.apply(&mut c.proob()), "evidence", "judge");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("no raw results under proob/.cache/results"), "{}", out.stderr);
}

#[test]
fn without_a_proob_directory_both_jobs_pass_and_say_so() {
    let repo = TempRepo::new();
    repo.write("src/lib.rs", "pub fn f() {}\n");
    let base = repo.commit("bare", NOW - 3 * DAY);
    repo.branch("pr");
    repo.write("src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let head = repo.commit("touch lib", NOW - DAY);
    let actions = Actions::pull_request(8, &base, &head, RUN_ID);
    let exec = action(actions.apply(&mut repo.proob()), "check", "execute");
    assert_eq!(exec.code, 0, "{}", exec.stderr);
    let m: Value = serde_json::from_str(&repo.read("proob/.cache/results/manifest.json")).expect("manifest");
    assert_eq!(m["attempted"], json!([]));
    assert!(m["notes"][0].as_str().expect("note").contains("no proob/ directory"), "{m}");
    let judge = action(actions.apply(&mut repo.proob()), "check", "judge");
    assert_eq!(judge.code, 0, "{}", judge.stderr);
    assert!(judge.stdout.contains("::notice title=proob::proob check: no proob/ directory at"), "{}", judge.stdout);
    let outputs = actions.outputs();
    assert_eq!(outputs["verdict"], "pass");
    assert_eq!(outputs["mode"], "none");
    assert!(actions.summary().contains("no proob/ directory"), "{}", actions.summary());
    // Evidence mode on the default branch: an empty evidence file, so the upload step has something to upload.
    repo.checkout("main");
    let actions = Actions::push(&base, RUN_ID + 1);
    assert_eq!(action(actions.apply(&mut repo.proob()), "evidence", "execute").code, 0);
    let judge = action(actions.apply(&mut repo.proob()), "evidence", "judge");
    assert_eq!(judge.code, 0, "{}", judge.stderr);
    assert_eq!(repo.read("proob/.cache/evidence/evidence.jsonl"), "");
    assert_eq!(actions.outputs()["records"], "0");
}

// ---------------------------------------------------------------------------
// The templates
// ---------------------------------------------------------------------------

const GATE_TEMPLATE: &str = include_str!("../../../templates/workflows/proob-gate.yml");
const EVIDENCE_TEMPLATE: &str = include_str!("../../../templates/workflows/proob-evidence.yml");

/// The lines of one job's block: from `  <name>:` up to the next job key or
/// top-level key (the `@@SETUP_STEPS@@` placeholder sits at column 0 inside
/// the execute job and belongs to it).
fn job_block(workflow: &str, name: &str) -> String {
    let header = format!("  {name}:");
    let mut lines = workflow.lines().skip_while(|l| *l != header);
    let first = lines.next().unwrap_or_else(|| panic!("no job {name}"));
    let another_key = |l: &str| {
        (l.starts_with("  ") && !l.starts_with("   "))
            || (!l.is_empty() && !l.starts_with(' ') && l != "@@SETUP_STEPS@@")
    };
    let body: Vec<&str> = lines.take_while(|l| !another_key(l)).collect();
    format!("{first}\n{}\n", body.join("\n"))
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn the_templates_hold_the_two_job_contract() {
    for (name, wf) in [("gate", GATE_TEMPLATE), ("evidence", EVIDENCE_TEMPLATE)] {
        assert!(!wf.lines().any(|l| l.trim_end().ends_with(": write")), "{name}: no write permission anywhere");
        assert!(wf.contains("\npermissions: {}\n"), "{name}: the workflow's own permissions are empty");
        let execute = job_block(wf, "execute");
        let judge = job_block(wf, "judge");
        assert!(execute.contains("    permissions: {}\n"), "{name}: execute holds no token:\n{execute}");
        assert!(!execute.contains("GITHUB_TOKEN"), "{name}: execute never receives the token");
        assert_eq!(
            count(&judge, "GITHUB_TOKEN: ${{ github.token }}"),
            1,
            "{name}: the judge step alone takes the token"
        );
        assert!(judge.contains("    needs: execute\n"));
        for job in [execute.as_str(), judge.as_str()] {
            assert_eq!(count(job, "actions/checkout@"), 1);
            assert_eq!(count(job, "\n          fetch-depth: 0\n"), 1, "{name}: full history on every checkout");
            assert_eq!(
                count(job, "\n          persist-credentials: false\n"),
                1,
                "{name}: no credential left in .git/config"
            );
            assert_eq!(
                count(job, "uses: @@ACTION_PIN@@ @@ACTION_PIN_COMMENT@@"),
                1,
                "{name}: the pinned action in each job"
            );
        }
        assert!(
            execute.contains("          name: proob-results\n          path: proob/.cache/results/\n"),
            "{name}: execute uploads the raw results"
        );
        assert!(
            judge.contains("          name: proob-results\n          path: proob/.cache/results/\n"),
            "{name}: judge downloads them to the same place"
        );
        for line in wf.lines().filter(|l| l.contains("uses: actions/") || l.contains("uses: taiki-e/")) {
            let sha = line.split('@').nth(1).and_then(|s| s.split_whitespace().next()).expect("sha");
            assert_eq!(sha.len(), 40, "{line}");
            assert!(line.contains("# v"), "{line}");
        }
    }
    assert!(GATE_TEMPLATE.contains("\non:\n  pull_request:\n"));
    assert!(
        GATE_TEMPLATE.contains("mode: check\n          job: execute\n")
            && GATE_TEMPLATE.contains("mode: check\n          job: judge\n")
    );
    assert!(job_block(GATE_TEMPLATE, "judge").contains("    name: proob gate\n"), "the required check's name");
    assert!(EVIDENCE_TEMPLATE.contains("\non:\n  push:\n    branches: [@@DEFAULT_BRANCH@@]\n  schedule:\n"));
    assert!(EVIDENCE_TEMPLATE.contains("    - cron: \"17 3 * * *\"\n"), "a nightly schedule at a fixed minute");
    assert!(EVIDENCE_TEMPLATE.contains("\n  workflow_dispatch:\n"));
    assert!(
        EVIDENCE_TEMPLATE.contains("mode: evidence\n          job: execute\n")
            && EVIDENCE_TEMPLATE.contains("mode: evidence\n          job: judge\n")
    );
    let judge = job_block(EVIDENCE_TEMPLATE, "judge");
    assert!(judge.contains("          name: proob-evidence\n          path: proob/.cache/evidence/\n          if-no-files-found: error\n          retention-days: 90\n"), "{judge}");

    // Rendered by `init`: no placeholder survives, and the action pin is a SHA whenever the build knew one.
    let repo = TempRepo::new();
    repo.write("Cargo.toml", "[package]\nname = \"x\"\n");
    assert_eq!(run(repo.proob().arg("init")).code, 0);
    let gate = repo.read(GATE_WORKFLOW);
    assert!(!gate.contains("@@"), "{gate}");
    assert_eq!(count(&gate, "persist-credentials: false"), 2);
    let pin = env!("PROOB_ACTION_PIN");
    assert_eq!(count(&gate, &format!("uses: {pin}")), 2);
    if env!("PROOB_ACTION_PINNED_BY_SHA") == "true" {
        let (_, sha) = pin.split_once('@').expect("owner/repo@sha");
        assert_eq!(sha.len(), 40, "{pin}");
    }
    assert!(repo.read("proob/config.toml").contains("scheduled_cases = 2000"), "the large budget is a config knob");
}
