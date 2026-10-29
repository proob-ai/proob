//! The stand-ins for GitHub. `GitHub` builds the fixture scenario proob reads
//! through `PROOB_FIXTURE_DIR` (the shape of `proob_effects::fixture::Scenario`:
//! REST payloads, never an approval or a verdict). `rate_limited` and
//! `unreachable` are endpoints for proob's real REST client.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use serde_json::{Value, json};

use crate::world::Fallible;

/// A GitHub account, and the git identity its commits carry.
pub struct Who {
    pub login: &'static str,
    pub id: u64,
    pub kind: &'static str,
}

impl Who {
    pub fn email(&self) -> String {
        format!("{}@example.com", self.login)
    }

    fn user(&self) -> Value {
        json!({ "login": self.login, "id": self.id, "type": self.kind })
    }
}

pub const ALICE: Who = Who { login: "alice", id: 1001, kind: "User" };
pub const BOB: Who = Who { login: "bob", id: 1002, kind: "User" };
/// A coding agent's GitHub App identity.
pub const AGENT: Who = Who { login: "claude[bot]", id: 4001, kind: "Bot" };

#[derive(Default, Clone)]
pub struct GitHub {
    permissions: BTreeMap<String, Value>,
    pulls_for_commit: BTreeMap<String, Vec<Value>>,
    pulls: BTreeMap<String, Value>,
    reviews: BTreeMap<String, Vec<Value>>,
    pull_commits: BTreeMap<String, Vec<Value>>,
    commits: BTreeMap<String, Value>,
    blobs: BTreeMap<String, Value>,
    workflow_runs: Vec<Value>,
    artifacts: BTreeMap<String, Vec<Value>>,
    artifact_lines: BTreeMap<String, Vec<Value>>,
}

impl GitHub {
    /// alice and bob can write; main is protected, needs a review and the gate,
    /// and has a CODEOWNERS file.
    pub fn new() -> GitHub {
        let mut gh = GitHub::default();
        gh.permission(&ALICE, "write").permission(&BOB, "write");
        ["CODEOWNERS", "docs/CODEOWNERS"].iter().for_each(|p| {
            gh.blob("main", p, None);
        });
        gh.blob("main", ".github/CODEOWNERS", Some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"));
        gh
    }

    pub fn permission(&mut self, who: &Who, level: &str) -> &mut Self {
        let value = json!({ "permission": level, "role_name": level, "user": who.user() });
        self.permissions.insert(who.login.to_owned(), value);
        self
    }

    fn pr(number: u64, author: &Who, merge_sha: Option<&str>, head_sha: &str) -> Value {
        json!({
            "number": number, "user": author.user(),
            "merged": merge_sha.is_some(),
            "merged_at": merge_sha.map(|_| "2026-10-18T10:00:00Z"),
            "merge_commit_sha": merge_sha,
            "base": { "ref": "main", "sha": "0".repeat(40) },
            "head": { "ref": "pr", "sha": head_sha },
        })
    }

    /// PR `number`, squash-merged as `sha`: its one commit, by `author`, added
    /// or modified `files`.
    pub fn merged_pr(&mut self, number: u64, author: &Who, sha: &str, parent: &str, files: &[&str]) -> &mut Self {
        let pr = GitHub::pr(number, author, Some(sha), sha);
        self.pulls_for_commit.entry(sha.to_owned()).or_default().push(pr.clone());
        self.pulls.insert(number.to_string(), pr);
        self.pr_commit(number, author, sha, parent, files)
    }

    pub fn open_pr(&mut self, number: u64, author: &Who, head: &str, parent: &str, files: &[&str]) -> &mut Self {
        let pr = GitHub::pr(number, author, None, head);
        self.pulls_for_commit.entry(head.to_owned()).or_default().push(pr.clone());
        self.pulls.insert(number.to_string(), pr);
        self.pr_commit(number, author, head, parent, files)
    }

    /// A commit of PR `number` by `author` that added or modified `files`.
    pub fn pr_commit(&mut self, number: u64, author: &Who, sha: &str, parent: &str, files: &[&str]) -> &mut Self {
        let summary = json!({ "sha": sha, "author": author.user(), "committer": author.user(), "parents": [{ "sha": parent }], "files": [] });
        let mut full = summary.clone();
        full["files"] = files.iter().map(|f| json!({ "filename": f, "status": "modified" })).collect();
        self.pull_commits.entry(number.to_string()).or_default().push(summary);
        self.commits.insert(sha.to_owned(), full);
        self
    }

    pub fn review(&mut self, number: u64, id: u64, who: &Who, commit: &str) -> &mut Self {
        let review = json!({ "id": id, "user": who.user(), "state": "APPROVED", "commit_id": commit, "submitted_at": "2026-10-18T09:00:00Z" });
        self.reviews.entry(number.to_string()).or_default().push(review);
        self
    }

    /// The blob GitHub's contents API answers for `path` at `rev` (`None`: 404).
    pub fn blob(&mut self, rev: &str, path: &str, sha: Option<&str>) -> &mut Self {
        self.blobs.insert(format!("{rev}:{path}"), sha.map_or(Value::Null, |s| Value::String(s.to_owned())));
        self
    }

    /// A successful evidence-workflow run with one `proob-evidence` artifact
    /// holding `lines`, listed first as GitHub lists the newest run.
    pub fn evidence_run(&mut self, run_id: u64, head_sha: &str, event: &str, branch: &str, at: u64, lines: Vec<Value>) {
        self.workflow_runs.insert(
            0,
            json!({
                "id": run_id, "path": crate::world::EVIDENCE_WORKFLOW, "head_branch": branch, "head_sha": head_sha,
                "event": event, "conclusion": "success", "created_at": iso8601(at)
            }),
        );
        let artifact = run_id * 100;
        self.artifacts.insert(
            run_id.to_string(),
            vec![json!({ "id": artifact, "name": "proob-evidence", "size_in_bytes": 1, "expired": false })],
        );
        self.artifact_lines.insert(artifact.to_string(), lines);
    }

    pub fn build(&self) -> Value {
        json!({
            "description": "built by sim/ (cargo xtask sim)",
            "default_branch": "main",
            "pulls_for_commit": self.pulls_for_commit,
            "pulls": self.pulls,
            "reviews": self.reviews,
            "permissions": self.permissions,
            "pull_commits": self.pull_commits,
            "commits": self.commits,
            "blobs": self.blobs,
            "issues": {},
            "workflow_runs": self.workflow_runs,
            "artifacts": self.artifacts,
            "artifact_lines": self.artifact_lines,
            "branch": { "name": "main", "protected": true },
            "branch_rules": [
                { "type": "pull_request" },
                { "type": "required_status_checks", "parameters": { "required_status_checks": [{ "context": "proob gate" }] } }
            ],
        })
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` for seconds since the epoch (Hinnant's civil_from_days).
pub fn iso8601(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let z = days + 719_468;
    let (era, doe) = (z / 146_097, z % 146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

// ---------------------------------------------------------------------------
// REST stand-ins for the live client
// ---------------------------------------------------------------------------

/// A GitHub that answers every request with the primary rate limit's 403
/// (`x-ratelimit-remaining: 0`). Serves until the process exits.
pub fn rate_limited() -> Fallible<String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind: {e}"))?;
    let url = format!("http://{}", listener.local_addr().map_err(|e| e.to_string())?);
    thread::spawn(move || listener.incoming().flatten().for_each(answer_rate_limited));
    Ok(url)
}

fn answer_rate_limited(mut stream: TcpStream) {
    let mut seen = Vec::new();
    let mut buf = [0u8; 4096];
    while !seen.windows(4).any(|w| w == b"\r\n\r\n") && seen.len() < 65_536 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => seen.extend_from_slice(&buf[..n]),
        }
    }
    let body = r#"{"message":"API rate limit exceeded","documentation_url":"https://docs.github.com/rest"}"#;
    let reply = format!(
        "HTTP/1.1 403 Forbidden\r\ncontent-type: application/json\r\nx-ratelimit-limit: 5000\r\nx-ratelimit-remaining: 0\r\nx-ratelimit-reset: 1793149200\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes());
}

/// An address nothing listens on: bound, then released, so connecting is refused.
pub fn unreachable() -> Fallible<String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind: {e}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    drop(listener);
    Ok(format!("http://{addr}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_matches_known_instants() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_793_145_600), "2026-10-28T00:00:00Z");
        assert_eq!(iso8601(951_782_400 + 3661), "2000-02-29T01:01:01Z");
    }

    #[test]
    fn the_rate_limited_stand_in_answers_403_with_no_remaining_calls() {
        let url = rate_limited().expect("serves");
        let mut stream = TcpStream::connect(url.trim_start_matches("http://")).expect("connects");
        stream.write_all(b"GET /repos/acme/widgets HTTP/1.1\r\nhost: x\r\n\r\n").expect("writes");
        let mut reply = String::new();
        stream.read_to_string(&mut reply).expect("reads");
        assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
        assert!(reply.contains("x-ratelimit-remaining: 0"), "{reply}");
    }

    #[test]
    fn the_unreachable_stand_in_refuses() {
        let url = unreachable().expect("an address");
        assert!(TcpStream::connect(url.trim_start_matches("http://")).is_err());
    }
}
