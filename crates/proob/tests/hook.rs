//! The Claude Code hooks. `session-start`: nothing without `proob/`, one JSON
//! object with a one-line `systemMessage` with it. `pre-tool-use`: a deny
//! object for a schema-invalid Write or Edit under `proob/`, nothing for
//! anything else. Neither ever exits non-zero.
mod common;

use common::*;
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn hook(mut cmd: Command, event: &str, stdin: &[u8], cwd: Option<&Path>) -> Run {
    cmd.args(["hook", event]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().expect("spawn");
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin);
    }
    let out = child.wait_with_output().expect("wait");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn repo_with_proob() -> TempRepo {
    let repo = TempRepo::new();
    repo.write("src/lib.rs", "pub fn f() {}\n");
    repo.write("proob/config.toml", CONFIG_TOML);
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "A", &["src/lib.rs"], "test"));
    repo.commit("introduce proob", NOW - DAY);
    repo.add_origin();
    repo
}

// ---------------------------------------------------------------------------
// session-start
// ---------------------------------------------------------------------------

fn session_start(cmd: Command, stdin: &[u8], cwd: Option<&Path>) -> Run {
    hook(cmd, "session-start", stdin, cwd)
}

#[test]
fn emits_nothing_without_a_proob_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gh = empty_gh_config();
    let payload = format!("{{\"cwd\": {:?}, \"hook_event_name\": \"SessionStart\"}}", dir.path().display().to_string());
    let out = session_start(proob(gh.path()), payload.as_bytes(), None);
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout, "");
}

#[test]
fn emits_one_line_system_message_with_a_proob_directory() {
    let repo = repo_with_proob();
    let payload = format!("{{\"cwd\": {:?}}}", repo.root().display().to_string());
    let out = session_start(proob(repo.gh_config.path()), payload.as_bytes(), None);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout.lines().count(), 1, "{}", out.stdout);
    let json: Value = serde_json::from_str(&out.stdout).expect("json");
    let message = json["systemMessage"].as_str().expect("systemMessage");
    assert!(message.starts_with("proob: "), "{message}");
    assert!(message.contains("unverified"), "{message}");
    assert!(!message.contains('\n'));
    assert_eq!(json["hookSpecificOutput"]["hookEventName"], "SessionStart");
    assert_eq!(json["hookSpecificOutput"]["additionalContext"], message);
}

#[test]
fn tolerates_empty_and_garbage_stdin_using_the_process_cwd() {
    let repo = repo_with_proob();
    let empty = session_start(proob(repo.gh_config.path()), b"", Some(repo.root()));
    assert_eq!(empty.code, 0);
    assert!(empty.stdout.contains("systemMessage"), "{}", empty.stdout);
    let garbage = session_start(proob(repo.gh_config.path()), b"not json {", Some(repo.root()));
    assert_eq!(garbage.code, 0);
    assert!(garbage.stdout.contains("systemMessage"), "{}", garbage.stdout);
}

#[test]
fn never_fails_even_when_proob_files_are_broken() {
    let repo = repo_with_proob();
    repo.write(&property_path(PROP_B), "id = 7\n");
    let payload = format!("{{\"cwd\": {:?}}}", repo.root().display().to_string());
    let out = session_start(proob(repo.gh_config.path()), payload.as_bytes(), None);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let json: Value = serde_json::from_str(&out.stdout).expect("json");
    let message = json["systemMessage"].as_str().expect("systemMessage");
    assert!(message.starts_with("proob: "), "{message}");
    assert!(message.contains(&property_path(PROP_B)), "{message}");
}

const PYTHON_ADAPTER: &str =
    "\n[[adapters]]\nroot = \".\"\nlanguage = \"python\"\nrunner = \"pytest\"\nmutation_tool = \"mutmut\"\n";

/// An adopted Python repository: one property over `src/billing/*.py`.
fn python_repo() -> TempRepo {
    let repo = TempRepo::new();
    repo.write("src/billing/charge.py", "def charge():\n    return 1\n");
    repo.write("proob/config.toml", &format!("{CONFIG_TOML}{PYTHON_ADAPTER}"));
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "A", &["src/billing/*.py"], "test"));
    repo.commit("introduce proob", NOW - DAY);
    repo.add_origin();
    repo
}

fn message(repo: &TempRepo) -> String {
    let payload = format!("{{\"cwd\": {:?}}}", repo.root().display().to_string());
    let out = session_start(proob(repo.gh_config.path()), payload.as_bytes(), None);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let json: Value = serde_json::from_str(&out.stdout).expect("json");
    json["systemMessage"].as_str().expect("systemMessage").to_string()
}

#[test]
fn the_line_points_at_declare_when_the_branch_or_the_work_adds_uncovered_code() {
    let repo = python_repo();
    let clean = message(&repo);
    assert!(!clean.contains("no property"), "nothing changed, nothing to say: {clean}");
    repo.branch("feature");
    repo.write("src/refunds/service.py", "def refund():\n    return 0\n");
    repo.commit("add refunds", NOW);
    repo.write("src/refunds/limits.py", "MAX = 3\n");
    repo.write("src/billing/tax.py", "RATE = 0\n");
    let line = message(&repo);
    assert!(line.starts_with(&clean), "the same line, with the pointer appended: {line}");
    assert!(line.ends_with(" · 2 changed files have no property: /proob:declare"), "{line}");
    assert!(!line.contains('\n'));
    repo.remove("src/refunds/limits.py");
    assert!(message(&repo).ends_with(" · 1 changed file has no property: /proob:declare"));
}

// ---------------------------------------------------------------------------
// pre-tool-use
// ---------------------------------------------------------------------------

fn pre_tool_use(repo: &TempRepo, payload: &Value) -> Run {
    hook(proob(repo.gh_config.path()), "pre-tool-use", payload.to_string().as_bytes(), None)
}

fn write_payload(repo: &TempRepo, file: &str, content: &str) -> Value {
    let path =
        if Path::new(file).is_absolute() { file.to_string() } else { repo.root().join(file).display().to_string() };
    json!({
        "session_id": "s", "cwd": repo.root(), "hook_event_name": "PreToolUse", "tool_name": "Write",
        "tool_input": { "file_path": path, "content": content }
    })
}

fn edit_payload(repo: &TempRepo, file: &str, old: &str, new: &str, all: bool) -> Value {
    json!({
        "session_id": "s", "cwd": repo.root(), "hook_event_name": "PreToolUse", "tool_name": "Edit",
        "tool_input": { "file_path": repo.root().join(file), "old_string": old, "new_string": new, "replace_all": all }
    })
}

/// The deny reason in a run's stdout, which must be exactly one JSON object.
fn deny_reason(out: &Run) -> String {
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout.lines().count(), 1, "{}", out.stdout);
    let json: Value = serde_json::from_str(&out.stdout).expect("json");
    assert_eq!(json["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
    json["hookSpecificOutput"]["permissionDecisionReason"].as_str().expect("reason").to_string()
}

fn assert_silent(out: &Run) {
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "", "{}", out.stdout);
}

#[test]
fn pre_tool_use_denies_a_status_key_in_a_write() {
    let repo = repo_with_proob();
    let content = format!("{}status = \"holds\"\n", property_toml(PROP_B, "B", &["src/lib.rs"], "test"));
    let out = pre_tool_use(&repo, &write_payload(&repo, &property_path(PROP_B), &content));
    let reason = deny_reason(&out);
    assert!(reason.starts_with(&format!("proob: {}: ", property_path(PROP_B))), "{reason}");
    assert!(reason.contains("`status`") && reason.contains("derived"), "{reason}");
    assert!(!reason.contains('\n'), "{reason}");
}

#[test]
fn pre_tool_use_denies_an_applied_edit_that_introduces_an_invalid_field() {
    let repo = repo_with_proob();
    let unknown = pre_tool_use(
        &repo,
        &edit_payload(&repo, &property_path(PROP_A), "severity = \"high\"", "severity = \"urgent\"", false),
    );
    let reason = deny_reason(&unknown);
    assert!(reason.contains("severity") || reason.contains("urgent"), "{reason}");
    assert!(!reason.contains('\n'), "{reason}");
    let status = pre_tool_use(
        &repo,
        &edit_payload(&repo, &property_path(PROP_A), "kind = \"test\"", "kind = \"test\"\nstatus = \"holds\"", true),
    );
    assert!(deny_reason(&status).contains("`status`"));
    // The file on disk is untouched: the hook reads, never writes.
    assert_eq!(repo.read(&property_path(PROP_A)), property_toml(PROP_A, "A", &["src/lib.rs"], "test"));
}

#[test]
fn pre_tool_use_allows_valid_property_content() {
    let repo = repo_with_proob();
    let write = pre_tool_use(
        &repo,
        &write_payload(&repo, &property_path(PROP_B), &property_toml(PROP_B, "B", &["src/lib.rs"], "test")),
    );
    assert_silent(&write);
    let edit = pre_tool_use(
        &repo,
        &edit_payload(&repo, &property_path(PROP_A), "title = \"A\"", "title = \"A, retitled\"", false),
    );
    assert_silent(&edit);
    // `reviewed` and `waiver` are schema-valid: the binary cannot know who writes them.
    let reviewed = format!("{}reviewed = 2026-10-28\n", property_toml(PROP_B, "B", &["src/lib.rs"], "test"));
    assert_silent(&pre_tool_use(&repo, &write_payload(&repo, &property_path(PROP_B), &reviewed)));
}

#[test]
fn pre_tool_use_checks_the_config_file_too() {
    let repo = repo_with_proob();
    let bad = pre_tool_use(
        &repo,
        &write_payload(
            &repo,
            "proob/config.toml",
            &CONFIG_TOML.replace("schema_version = 1\n", "schema_version = 1\nkill_floor = 0\n"),
        ),
    );
    let reason = deny_reason(&bad);
    assert!(reason.starts_with("proob: proob/config.toml: ") && reason.contains("kill_floor"), "{reason}");
    assert_silent(&pre_tool_use(&repo, &write_payload(&repo, "proob/config.toml", CONFIG_TOML)));
}

#[test]
fn pre_tool_use_enforces_the_waiver_window_from_the_repository_config() {
    let repo = repo_with_proob();
    let waived = format!(
        "{}\n[waiver]\nuntil = 2030-01-01\nissue = \"#1\"\nreason = \"later\"\n",
        property_toml(PROP_B, "B", &["src/lib.rs"], "test")
    );
    let reason = deny_reason(&pre_tool_use(&repo, &write_payload(&repo, &property_path(PROP_B), &waived)));
    assert!(reason.contains("waiver window"), "{reason}");
}

#[test]
fn pre_tool_use_ignores_paths_outside_proob() {
    let repo = repo_with_proob();
    for (file, content) in [
        ("src/lib.rs", "status = \"holds\"\n"),
        ("proob/README.md", "status = 1"),
        ("proob/properties/notes.md", "status = 1"),
        ("docs/proob/properties/x.toml", "nonsense = ["),
    ] {
        assert_silent(&pre_tool_use(&repo, &write_payload(&repo, file, content)));
    }
}

#[test]
fn pre_tool_use_resolves_a_relative_path_against_cwd() {
    let repo = repo_with_proob();
    let payload = json!({
        "cwd": repo.root(), "tool_name": "Write",
        "tool_input": { "file_path": property_path(PROP_B), "content": "status = \"holds\"\n" }
    });
    assert!(deny_reason(&pre_tool_use(&repo, &payload)).contains("`status`"));
}

#[test]
fn pre_tool_use_is_silent_on_garbage_missing_files_and_edits_the_tool_will_refuse() {
    let repo = repo_with_proob();
    for garbage in [
        &b""[..],
        b"not json {",
        b"{}",
        b"[1,2]",
        br#"{"tool_name":"Write","tool_input":{"file_path":"proob/properties/x.toml"}}"#,
    ] {
        let out = hook(proob(repo.gh_config.path()), "pre-tool-use", garbage, Some(repo.root()));
        assert_silent(&out);
    }
    // old_string absent from the file: the Edit tool itself fails.
    assert_silent(&pre_tool_use(
        &repo,
        &edit_payload(&repo, &property_path(PROP_A), "no such text", "status = 1", false),
    ));
    // The file does not exist: nothing to apply the edit to.
    assert_silent(&pre_tool_use(&repo, &edit_payload(&repo, &property_path(PROP_C), "id", "status", false)));
    // Ambiguous without replace_all: the Edit tool itself fails.
    assert_silent(&pre_tool_use(&repo, &edit_payload(&repo, &property_path(PROP_A), "\"", "status = 1 \"", false)));
}
