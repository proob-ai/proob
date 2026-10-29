//! Shared test scaffolding: a deterministic temporary git repository (fixed
//! identities and dates, so every sha is the same on every machine), the
//! `proob` binary with the environment scrubbed of tokens, and a builder for
//! the fixture GitHub scenario the CLI honours through `PROOB_FIXTURE_DIR`.
#![allow(dead_code)]
pub mod scenarios;

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// 2026-10-28T00:00:00Z: the clock every test runs the binary at.
pub const NOW: u64 = 1_793_145_600;
pub const DAY: u64 = 86_400;

pub const PROP_A: &str = "01J9Z3W1V8Q4K7N2M5R8T1X4A1";
pub const PROP_B: &str = "01J9Z3W1V8Q4K7N2M5R8T1X4B2";
pub const PROP_C: &str = "01J9Z3W1V8Q4K7N2M5R8T1X4C3";
pub const PROP_D: &str = "01J9Z3W1V8Q4K7N2M5R8T1X4D4";
pub const PROP_E: &str = "01J9Z3W1V8Q4K7N2M5R8T1X4E5";
pub const PROP_F: &str = "01J9Z3W1V8Q4K7N2M5R8T1X4F6";
pub const PROP_G: &str = "01J9Z3W1V8Q4K7N2M5R8T1X4G7";
pub const PROP_H: &str = "01J9Z3W1V8Q4K7N2M5R8T1X4H8";

pub fn property_path(id: &str) -> String {
    format!("proob/properties/{id}.toml")
}

pub fn property_toml(id: &str, title: &str, scope: &[&str], kind: &str) -> String {
    let scope: Vec<String> = scope.iter().map(|s| format!("\"{s}\"")).collect();
    format!(
        "id = \"{id}\"\ntitle = \"{title}\"\nstatement = \"{title}, always.\"\nscope = [{}]\nseverity = \"high\"\nrationale = \"Because the team said so.\"\nkind = \"{kind}\"\n",
        scope.join(", ")
    )
}

pub const CONFIG_TOML: &str = "schema_version = 1\nallow_self_signed = false\n\n[policy]\nholds = \"pass\"\nchecked = \"pass\"\nproposed = \"pass\"\nwaived = \"pass\"\nnot_checked_within_grace = \"pass\"\nnot_checked_after_grace = \"warn\"\nre_approve = \"warn\"\nbroken_affected = \"block\"\nbroken_unaffected = \"pass\"\n";

pub const GATE_YML: &str = "name: proob gate\non: pull_request\njobs: {}\n";
pub const EVIDENCE_YML: &str = "name: proob evidence\non: push\njobs: {}\n";

// ---------------------------------------------------------------------------
// The binary
// ---------------------------------------------------------------------------

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{}\n--- stderr ---\n{}", self.stdout, self.stderr))
    }
}

/// A sandbox for the binary's token discovery: `gh` reads its (absent)
/// credentials from here, and `bin/` holds the only program on the child's
/// `PATH`, `git`, so `gh auth token` cannot run and no keychain token leaks
/// into a test that must be offline.
pub fn empty_gh_config() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir");
    let git = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join("git")).find(|g| g.is_file()).expect("git on PATH"))
        .expect("PATH");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&git, bin.join("git")).expect("symlink git");
    #[cfg(not(unix))]
    std::fs::copy(&git, bin.join("git.exe")).expect("copy git");
    dir
}

/// The binary with a fixed clock and no way to a token. Every `GITHUB_*` and
/// `RUNNER_*` variable of the host is removed, so a test behaves the same on
/// a laptop and inside GitHub Actions (where the runner sets dozens of them).
pub fn proob(gh_config: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_proob"));
    std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| k.starts_with("GITHUB_") || k.starts_with("RUNNER_"))
        .for_each(|k| {
            cmd.env_remove(k);
        });
    cmd.env_remove("GH_TOKEN")
        .env_remove("GH_ENTERPRISE_TOKEN")
        .env_remove("PROOB_FIXTURE_DIR")
        .env("GH_CONFIG_DIR", gh_config)
        .env("PATH", gh_config.join("bin"))
        .env("PROOB_NOW", NOW.to_string());
    cmd
}

pub fn run(cmd: &mut Command) -> Run {
    let out = cmd.output().expect("proob binary runs");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

// ---------------------------------------------------------------------------
// The repository
// ---------------------------------------------------------------------------

pub struct TempRepo {
    pub dir: TempDir,
    pub gh_config: TempDir,
}

impl TempRepo {
    pub fn new() -> TempRepo {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = TempRepo { dir, gh_config: empty_gh_config() };
        repo.git(&["init", "-q", "-b", "main", "."]);
        repo.git(&["config", "user.name", "Alice"]);
        repo.git(&["config", "user.email", "alice@example.com"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.git(&["config", "core.autocrlf", "false"]);
        repo
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.root())
            .env("GIT_AUTHOR_DATE", format!("{NOW} +0000"))
            .env("GIT_COMMITTER_DATE", format!("{NOW} +0000"))
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    pub fn write(&self, path: &str, text: &str) {
        let full = self.root().join(path);
        if let Some(dir) = full.parent() {
            std::fs::create_dir_all(dir).expect("mkdir");
        }
        std::fs::write(full, text).expect("write");
    }

    pub fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    pub fn remove(&self, path: &str) {
        std::fs::remove_file(self.root().join(path)).expect("remove");
    }

    /// Commits everything at `at` seconds since the epoch; returns the sha.
    pub fn commit(&self, message: &str, at: u64) -> String {
        self.git(&["add", "-A"]);
        let out = Command::new("git")
            .args(["commit", "-q", "--allow-empty", "-m", message])
            .current_dir(self.root())
            .env("GIT_AUTHOR_DATE", format!("{at} +0000"))
            .env("GIT_COMMITTER_DATE", format!("{at} +0000"))
            .output()
            .expect("git commit");
        assert!(out.status.success(), "commit failed: {}", String::from_utf8_lossy(&out.stderr));
        self.rev("HEAD")
    }

    pub fn rev(&self, spec: &str) -> String {
        self.git(&["rev-parse", spec])
    }

    pub fn blob(&self, rev: &str, path: &str) -> String {
        self.git(&["rev-parse", &format!("{rev}:{path}")])
    }

    pub fn branch(&self, name: &str) {
        self.git(&["checkout", "-q", "-b", name]);
    }

    pub fn checkout(&self, name: &str) {
        self.git(&["checkout", "-q", name]);
    }

    pub fn add_origin(&self) {
        self.git(&["remote", "add", "origin", "https://github.com/acme/widgets.git"]);
    }

    pub fn proob(&self) -> Command {
        let mut cmd = proob(self.gh_config.path());
        cmd.arg("--repo").arg(self.root());
        cmd
    }
}

// ---------------------------------------------------------------------------
// The fixture GitHub
// ---------------------------------------------------------------------------

pub fn user(login: &str, id: u64) -> Value {
    json!({ "login": login, "id": id, "type": "User" })
}

pub const ALICE: (&str, u64) = ("alice", 1001);
pub const BOB: (&str, u64) = ("bob", 1002);

/// A scenario in the shape `proob_effects::fixture::Scenario` reads.
pub struct ScenarioBuilder {
    pulls_for_commit: BTreeMap<String, Vec<Value>>,
    pulls: BTreeMap<String, Value>,
    reviews: BTreeMap<String, Vec<Value>>,
    permissions: BTreeMap<String, Value>,
    pull_commits: BTreeMap<String, Vec<Value>>,
    commits: BTreeMap<String, Value>,
    blobs: BTreeMap<String, Value>,
    issues: BTreeMap<String, Value>,
    workflow_runs: Vec<Value>,
    artifacts: BTreeMap<String, Vec<Value>>,
    artifact_lines: BTreeMap<String, Vec<Value>>,
    branch: Option<Value>,
    branch_rules: Vec<Value>,
}

impl ScenarioBuilder {
    pub fn new() -> ScenarioBuilder {
        let mut b = ScenarioBuilder {
            pulls_for_commit: BTreeMap::new(),
            pulls: BTreeMap::new(),
            reviews: BTreeMap::new(),
            permissions: BTreeMap::new(),
            pull_commits: BTreeMap::new(),
            commits: BTreeMap::new(),
            blobs: BTreeMap::new(),
            issues: BTreeMap::new(),
            workflow_runs: Vec::new(),
            artifacts: BTreeMap::new(),
            artifact_lines: BTreeMap::new(),
            branch: None,
            branch_rules: Vec::new(),
        };
        b.permission(ALICE, "write");
        b.permission(BOB, "write");
        b
    }

    pub fn permission(&mut self, who: (&str, u64), level: &str) -> &mut Self {
        self.permissions
            .insert(who.0.to_string(), json!({ "permission": level, "role_name": level, "user": user(who.0, who.1) }));
        self
    }

    fn pr_value(
        number: u64,
        author: (&str, u64),
        merged: bool,
        merge_sha: Option<&str>,
        head_sha: &str,
        head_ref: &str,
    ) -> Value {
        json!({
            "number": number,
            "user": user(author.0, author.1),
            "merged": merged,
            "merged_at": if merged { Value::String("2026-10-18T10:00:00Z".into()) } else { Value::Null },
            "merge_commit_sha": merge_sha,
            "base": { "ref": "main", "sha": "0".repeat(40) },
            "head": { "ref": head_ref, "sha": head_sha },
        })
    }

    /// A PR merged as `merge_sha`, whose one commit (`merge_sha` itself) added
    /// or modified `files`, authored by `author`.
    pub fn merged_pr(&mut self, number: u64, author: (&str, u64), merge_sha: &str, files: &[&str]) -> &mut Self {
        let pr = Self::pr_value(number, author, true, Some(merge_sha), merge_sha, "feature");
        self.pulls_for_commit.entry(merge_sha.to_string()).or_default().push(pr.clone());
        self.pulls.insert(number.to_string(), pr);
        self.pr_commit(number, author, merge_sha, files, &[]);
        self
    }

    /// An open PR from `head_sha` into main.
    pub fn open_pr(&mut self, number: u64, author: (&str, u64), head_sha: &str) -> &mut Self {
        let pr = Self::pr_value(number, author, false, None, head_sha, "pr");
        self.pulls_for_commit.entry(head_sha.to_string()).or_default().push(pr.clone());
        self.pulls.insert(number.to_string(), pr);
        self
    }

    /// One non-merge commit of a PR, with the files it added or modified.
    pub fn pr_commit(
        &mut self,
        number: u64,
        author: (&str, u64),
        sha: &str,
        files: &[&str],
        parents: &[&str],
    ) -> &mut Self {
        let parents: Vec<Value> = parents.iter().map(|p| json!({ "sha": p })).collect();
        let summary = json!({ "sha": sha, "author": user(author.0, author.1), "committer": user(author.0, author.1), "parents": parents, "files": [] });
        let files: Vec<Value> = files.iter().map(|f| json!({ "filename": f, "status": "modified" })).collect();
        let mut full = summary.clone();
        full["files"] = Value::Array(files);
        self.pull_commits.entry(number.to_string()).or_default().push(summary);
        self.commits.insert(sha.to_string(), full);
        self
    }

    pub fn review(&mut self, number: u64, id: u64, who: (&str, u64), state: &str, commit_id: &str) -> &mut Self {
        self.reviews.entry(number.to_string()).or_default().push(json!({
            "id": id, "user": user(who.0, who.1), "state": state, "commit_id": commit_id, "submitted_at": "2026-10-18T09:00:00Z"
        }));
        self
    }

    /// The blob GitHub's contents API answers for `path` at `rev`.
    pub fn blob(&mut self, rev: &str, path: &str, sha: Option<&str>) -> &mut Self {
        self.blobs.insert(format!("{rev}:{path}"), sha.map_or(Value::Null, |s| Value::String(s.to_string())));
        self
    }

    pub fn governance(
        &mut self,
        protected: bool,
        review_rule: bool,
        gate_required: bool,
        codeowners: bool,
    ) -> &mut Self {
        self.branch = Some(json!({ "name": "main", "protected": protected }));
        if review_rule {
            self.branch_rules.push(json!({ "type": "pull_request" }));
        }
        if gate_required {
            self.branch_rules.push(json!({ "type": "required_status_checks", "parameters": { "required_status_checks": [{ "context": "proob gate" }] } }));
        }
        for path in [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"] {
            self.blob(
                "main",
                path,
                (codeowners && path == ".github/CODEOWNERS").then_some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            );
        }
        self
    }

    pub fn issue(&mut self, number: u64, open: bool) -> &mut Self {
        self.issues
            .insert(number.to_string(), json!({ "number": number, "state": if open { "open" } else { "closed" } }));
        self
    }

    /// An evidence-workflow run on main with the given records.
    pub fn evidence_run(
        &mut self,
        run_id: u64,
        head_sha: &str,
        event: &str,
        created_at: &str,
        records: Vec<Value>,
    ) -> &mut Self {
        self.workflow_runs.push(json!({
            "id": run_id, "path": ".github/workflows/proob-evidence.yml", "head_branch": "main", "head_sha": head_sha,
            "event": event, "conclusion": "success", "created_at": created_at
        }));
        let artifact_id = run_id * 100;
        self.artifacts.insert(
            run_id.to_string(),
            vec![json!({ "id": artifact_id, "name": "proob-evidence", "size_in_bytes": 1, "expired": false })],
        );
        self.artifact_lines.insert(artifact_id.to_string(), records);
        self
    }

    pub fn build(&self) -> Value {
        json!({
            "description": "built by crates/proob/tests/common",
            "default_branch": "main",
            "pulls_for_commit": self.pulls_for_commit,
            "pulls": self.pulls,
            "reviews": self.reviews,
            "permissions": self.permissions,
            "pull_commits": self.pull_commits,
            "commits": self.commits,
            "blobs": self.blobs,
            "issues": self.issues,
            "workflow_runs": self.workflow_runs,
            "artifacts": self.artifacts,
            "artifact_lines": self.artifact_lines,
            "branch": self.branch,
            "branch_rules": self.branch_rules,
        })
    }
}

/// The run an evidence record came from.
pub struct RunMeta<'a> {
    pub run_id: u64,
    pub head_sha: &'a str,
    pub workflow_blob: &'a str,
    pub at: u64,
}

/// An evidence record line, as the evidence job writes it.
pub fn record(id: &str, kind: &str, hashes: &Hashes, outcome: Value, run: &RunMeta<'_>) -> Value {
    json!({
        "schema_version": 1, "id": id, "kind": kind, "algo": "sha256",
        "content_hash": hashes.content, "binding_hash": hashes.binding, "scope_hash": hashes.scope,
        "outcome": outcome, "tool_versions": { "proptest": "1.12.0" },
        "run_id": run.run_id, "head_sha": run.head_sha, "workflow_blob": run.workflow_blob, "at": run.at
    })
}

/// A run-result line, as the execute job writes it.
pub fn result_line(id: &str, kind: &str, hashes: &Hashes, outcome: Value) -> String {
    json!({
        "schema_version": 1, "id": id, "kind": kind, "algo": "sha256",
        "content_hash": hashes.content, "binding_hash": hashes.binding, "scope_hash": hashes.scope,
        "outcome": outcome, "replay": { "seed": "7", "cases": 256 }, "tool_versions": { "proptest": "1.12.0" }
    })
    .to_string()
}

#[derive(Debug, Clone)]
pub struct Hashes {
    pub content: String,
    pub binding: String,
    pub scope: String,
}

/// The fixture directory the CLI reads through `PROOB_FIXTURE_DIR`.
pub struct Fixture {
    pub dir: TempDir,
}

impl Fixture {
    pub fn new(scenario: &Value, bindings: Option<&Map<String, Value>>) -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let f = Fixture { dir };
        f.write_scenario(scenario);
        if let Some(b) = bindings {
            std::fs::write(f.dir.path().join("bindings.json"), serde_json::to_string_pretty(b).expect("json"))
                .expect("write");
        }
        f
    }

    pub fn write_scenario(&self, scenario: &Value) {
        std::fs::write(self.dir.path().join("github.json"), serde_json::to_string_pretty(scenario).expect("json"))
            .expect("write");
    }

    pub fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }
}

/// The hashes `proob hash --json` reports for a property under a fixture.
pub fn hashes_of(repo: &TempRepo, fixture: &Fixture, id: &str) -> Hashes {
    let out = run(repo.proob().env("PROOB_FIXTURE_DIR", fixture.path()).args(["hash", id, "--json"]));
    assert_eq!(out.code, 0, "hash failed: {}", out.stderr);
    let v = out.json();
    Hashes {
        content: v["content_hash"].as_str().expect("content").to_string(),
        binding: v["binding_hash"].as_str().unwrap_or(&"b".repeat(64)).to_string(),
        scope: v["scope_hash"].as_str().unwrap_or(&"a".repeat(64)).to_string(),
    }
}

/// Compares `actual` with the golden file, or rewrites it under
/// `PROOB_UPDATE_GOLDEN=1`.
pub fn assert_golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/reports").join(name);
    if std::env::var_os("PROOB_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (set PROOB_UPDATE_GOLDEN=1 to create it)", path.display()));
    assert_eq!(actual, expected, "{} differs; set PROOB_UPDATE_GOLDEN=1 to accept", path.display());
}
