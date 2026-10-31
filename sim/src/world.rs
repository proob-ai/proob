//! The local world a scenario runs in: a deterministic git repository (fixed
//! identities and dates), the fixture GitHub (`github.rs`), the `proob` binary
//! with a scrubbed environment, and the two jobs of a workflow run, each in its
//! own checkout with the results artifact carried between them.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use tempfile::TempDir;

use crate::github::{GitHub, Who};

/// 2026-10-28T00:00:00Z: the clock every proob run reads (`PROOB_NOW`), unless a world moves its `now`.
pub const NOW: u64 = 1_793_145_600;
pub const DAY: u64 = 86_400;
pub const REPOSITORY: &str = "acme/widgets";
pub const GATE_WORKFLOW: &str = ".github/workflows/proob-gate.yml";
pub const EVIDENCE_WORKFLOW: &str = ".github/workflows/proob-evidence.yml";
const RESULTS: &str = "proob/.cache/results";
const EVIDENCE_FILE: &str = "proob/.cache/evidence/evidence.jsonl";

pub type Fallible<T> = Result<T, String>;

/// Where the simulation's inputs live: the workspace, and the built binary.
#[derive(Clone)]
pub struct Ctx {
    pub root: PathBuf,
    pub proob: PathBuf,
}

impl Ctx {
    /// Shared across runs so the fixture's cargo build is incremental.
    pub fn rust_target(&self) -> PathBuf {
        self.root.join("target/sim/rust-repo")
    }

    /// The workspace's pinned channel, which the execute job's setup steps would
    /// install; outside the workspace rustup would pick the host default.
    pub fn toolchain(&self) -> Fallible<String> {
        let file = self.root.join("rust-toolchain.toml");
        let text = io(&file, fs::read_to_string(&file))?;
        channel(&text).ok_or_else(|| format!("{}: no channel", file.display()))
    }
}

fn channel(toml: &str) -> Option<String> {
    toml.lines()
        .find_map(|l| l.trim().strip_prefix("channel = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_owned)
}

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn json(&self) -> Fallible<Value> {
        serde_json::from_str(&self.stdout)
            .map_err(|e| format!("stdout is not JSON ({e}); exit {}; stderr: {}", self.code, first_line(&self.stderr)))
    }
}

pub fn first_line(text: &str) -> &str {
    text.lines().find(|l| !l.trim().is_empty()).unwrap_or("")
}

pub fn run(cmd: &mut Command) -> Fallible<Run> {
    let out = cmd.output().map_err(|e| format!("cannot start {:?}: {e}", cmd.get_program()))?;
    Ok(Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

fn io<T>(what: &Path, r: std::io::Result<T>) -> Fallible<T> {
    r.map_err(|e| format!("{}: {e}", what.display()))
}

// ---------------------------------------------------------------------------
// The repository
// ---------------------------------------------------------------------------

/// A git checkout whose every sha is the same on every machine.
pub struct Repo {
    pub dir: PathBuf,
}

impl Repo {
    pub fn init(dir: PathBuf) -> Fallible<Repo> {
        io(&dir, fs::create_dir_all(&dir))?;
        let repo = Repo { dir };
        repo.git(&["init", "-q", "-b", "main", "."])?;
        repo.git(&["config", "commit.gpgsign", "false"])?;
        repo.git(&["config", "core.autocrlf", "false"])?;
        Ok(repo)
    }

    pub fn git(&self, args: &[&str]) -> Fallible<String> {
        let out = run(git(&self.dir).args(args))?;
        match out.code {
            0 => Ok(out.stdout.trim().to_owned()),
            _ => Err(format!("git {}: {}", args.join(" "), first_line(&out.stderr))),
        }
    }

    pub fn write(&self, path: &str, text: &str) -> Fallible<()> {
        let full = self.dir.join(path);
        if let Some(parent) = full.parent() {
            io(parent, fs::create_dir_all(parent))?;
        }
        io(&full, fs::write(&full, text))
    }

    pub fn read(&self, path: &str) -> Fallible<String> {
        let full = self.dir.join(path);
        io(&full, fs::read_to_string(&full))
    }

    /// Commits everything as `who` at `at` (seconds since the epoch); the sha.
    pub fn commit(&self, message: &str, who: &Who, at: u64) -> Fallible<String> {
        self.git(&["add", "-A"])?;
        let date = format!("{at} +0000");
        let out = run(git(&self.dir)
            .args(["commit", "-q", "--allow-empty", "-m", message])
            .env("GIT_AUTHOR_NAME", who.login)
            .env("GIT_AUTHOR_EMAIL", who.email())
            .env("GIT_COMMITTER_NAME", who.login)
            .env("GIT_COMMITTER_EMAIL", who.email())
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date))?;
        match out.code {
            0 => self.rev("HEAD"),
            _ => Err(format!("commit '{message}': {}", first_line(&out.stderr))),
        }
    }

    pub fn rev(&self, spec: &str) -> Fallible<String> {
        self.git(&["rev-parse", spec])
    }

    pub fn blob(&self, rev: &str, path: &str) -> Fallible<String> {
        self.rev(&format!("{rev}:{path}"))
    }

    pub fn checkout(&self, args: &[&str]) -> Fallible<()> {
        self.git(&[&["checkout", "-q"], args].concat()).map(drop)
    }
}

fn git(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir).env("GIT_CONFIG_NOSYSTEM", "1").env("HOME", dir);
    cmd
}

/// Copies `from` into `to`, skipping build output.
pub fn copy_tree(from: &Path, to: &Path) -> Fallible<()> {
    io(to, fs::create_dir_all(to))?;
    io(from, fs::read_dir(from))?.filter_map(Result::ok).try_for_each(|entry| {
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        match (entry.file_name() == "target", src.is_dir()) {
            (true, _) => Ok(()),
            (false, true) => copy_tree(&src, &dst),
            (false, false) => io(&src, fs::copy(&src, &dst)).map(drop),
        }
    })
}

// ---------------------------------------------------------------------------
// The world
// ---------------------------------------------------------------------------

/// One scenario's world: the developer's checkout (also the remote the
/// workflow checkouts clone from), the fixture GitHub, and a sandbox whose
/// only program is `git`, so no keychain token or tool leaks into a run that
/// must have neither.
pub struct World {
    pub ctx: Ctx,
    /// Removed on drop, unless `SIM_KEEP` is set (then `None`, and the path is printed).
    _tmp: Option<TempDir>,
    dir: PathBuf,
    pub repo: Repo,
    pub github: GitHub,
    /// The clock `status` and the other developer-side runs read.
    pub now: u64,
}

impl World {
    pub fn new(ctx: &Ctx) -> Fallible<World> {
        let tmp = io(Path::new("tempdir"), tempfile::tempdir())?;
        let dir = tmp.path().to_path_buf();
        let repo = Repo::init(dir.join("repo"))?;
        let sandbox = dir.join("sandbox/bin");
        io(&sandbox, fs::create_dir_all(&sandbox))?;
        io(&sandbox, fs::create_dir_all(dir.join("sandbox/gh")))?;
        link_git(&sandbox)?;
        let kept = std::env::var_os("SIM_KEEP").is_some();
        let tmp = match kept {
            true => {
                println!("  (SIM_KEEP: world kept at {})", tmp.keep().display());
                None
            }
            false => Some(tmp),
        };
        Ok(World { ctx: ctx.clone(), _tmp: tmp, dir, repo, github: GitHub::new(), now: NOW })
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// The binary at `dir`, with the fixture GitHub as it stands now.
    pub fn proob(&self, dir: &Path) -> Fallible<Command> {
        let fixture = self.path("github");
        io(&fixture, fs::create_dir_all(&fixture))?;
        let text = serde_json::to_string_pretty(&self.github.build()).map_err(|e| e.to_string())?;
        io(&fixture, fs::write(fixture.join("github.json"), text))?;
        let mut cmd = self.sandboxed(dir);
        cmd.env("PROOB_FIXTURE_DIR", fixture);
        Ok(cmd)
    }

    /// Binding sets that stand in for discovery in every later run: the
    /// fixture's `bindings.json` beside `github.json`. An execute job binds
    /// its checks by discovery alone, so write this after the last workflow run.
    pub fn bindings(&self, sets: &Value) -> Fallible<()> {
        let fixture = self.path("github");
        io(&fixture, fs::create_dir_all(&fixture))?;
        io(&fixture, fs::write(fixture.join("bindings.json"), sets.to_string()))
    }

    /// The binary at `dir` talking to a real REST endpoint at `api_url` with a
    /// token: the live client, not the fixture.
    pub fn proob_live(&self, dir: &Path, api_url: &str) -> Command {
        let mut cmd = self.sandboxed(dir);
        cmd.env("GITHUB_TOKEN", "ghp_simulatedsimulatedsimulatedsimula")
            .env("GITHUB_REPOSITORY", REPOSITORY)
            .env("GITHUB_API_URL", api_url);
        cmd
    }

    /// Every `GITHUB_*`/`RUNNER_*` of the host removed, a fixed clock, and a
    /// PATH holding only git: what a judge job (and a test) gets.
    fn sandboxed(&self, dir: &Path) -> Command {
        let mut cmd = Command::new(&self.ctx.proob);
        std::env::vars_os()
            .filter_map(|(k, _)| k.into_string().ok())
            .filter(|k| k.starts_with("GITHUB_") || k.starts_with("RUNNER_") || k.starts_with("PROOB_"))
            .for_each(|k| {
                cmd.env_remove(k);
            });
        cmd.env_remove("GH_TOKEN")
            .env_remove("GH_ENTERPRISE_TOKEN")
            .env("GH_CONFIG_DIR", self.path("sandbox/gh"))
            .env("PATH", self.path("sandbox/bin"))
            .env("PROOB_NOW", self.now.to_string())
            .arg("--repo")
            .arg(dir);
        cmd
    }

    /// `proob status --json` in the developer's checkout.
    pub fn status(&self) -> Fallible<Run> {
        run(self.proob(&self.repo.dir)?.args(["status", "--json"]))
    }

    /// The hashes `proob hash` reports for `id` in the developer's checkout.
    pub fn hashes(&self, id: &str) -> Fallible<Value> {
        let out = run(self.proob(&self.repo.dir)?.args(["hash", id, "--json"]))?;
        match out.code {
            0 => out.json(),
            code => Err(format!("proob hash {id}: exit {code}: {}", first_line(&out.stderr))),
        }
    }

    // -----------------------------------------------------------------------
    // Workflow runs
    // -----------------------------------------------------------------------

    /// A fresh checkout of `sha`, as actions/checkout leaves it (full history).
    pub fn checkout_of(&self, name: &str, sha: &str) -> Fallible<Repo> {
        let dir = self.path(name);
        let origin = self.repo.dir.to_string_lossy().into_owned();
        let target = dir.to_string_lossy().into_owned();
        self.repo.git(&["clone", "-q", "--branch", "main", &origin, &target])?;
        let checkout = Repo { dir };
        checkout.checkout(&["--detach", sha])?;
        Ok(checkout)
    }

    /// Both jobs of one workflow run, as GitHub runs them: `execute` in its own
    /// checkout with the toolchain on PATH and no token; its results uploaded
    /// and downloaded into a second checkout; `judge` there with git alone on
    /// PATH and the (fixture) GitHub. The judge's run, and its checkout.
    pub fn workflow_run(&self, event: &Event, run_id: u64) -> Fallible<(Run, Repo)> {
        let execute = self.checkout_of(&format!("run-{run_id}-execute"), &event.sha)?;
        let env = self.path(&format!("run-{run_id}-env"));
        let mut cmd = self.proob(&execute.dir)?;
        cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("RUSTUP_TOOLCHAIN", self.ctx.toolchain()?)
            .env("CARGO_TARGET_DIR", self.ctx.rust_target());
        let exec =
            run(event.apply(&mut cmd, &env, run_id)?.args(["action", "--mode", event.mode, "--job", "execute"]))?;
        if exec.code != 0 {
            return Err(format!("execute job of run {run_id}: exit {}: {}", exec.code, first_line(&exec.stderr)));
        }
        let judge = self.checkout_of(&format!("run-{run_id}-judge"), &event.sha)?;
        copy_tree(&execute.dir.join(RESULTS), &judge.dir.join(RESULTS))?;
        let mut cmd = self.proob(&judge.dir)?;
        cmd.arg("--json");
        let out = run(event.apply(&mut cmd, &env, run_id)?.args(["action", "--mode", event.mode, "--job", "judge"]))?;
        Ok((out, judge))
    }

    /// An evidence run on the default branch whose judge succeeded: the
    /// `proob-evidence` artifact it uploaded becomes the run's artifact on the
    /// fixture GitHub, where every later `status` finds it.
    pub fn evidence_run(&mut self, event: &Event, run_id: u64) -> Fallible<()> {
        let (judge, lines) = self.evidence_lines(event, run_id)?;
        if judge.code != 0 {
            return Err(format!("evidence judge of run {run_id}: exit {}: {}", judge.code, first_line(&judge.stderr)));
        }
        self.github.evidence_run(run_id, &event.sha, event.name, "main", event.at, lines);
        Ok(())
    }

    /// Both jobs of an evidence run, and the records its judge wrote for the
    /// `proob-evidence` artifact, whatever the judge's exit.
    pub fn evidence_lines(&self, event: &Event, run_id: u64) -> Fallible<(Run, Vec<Value>)> {
        let (judge, checkout) = self.workflow_run(event, run_id)?;
        let lines = checkout
            .read(EVIDENCE_FILE)
            .map_err(|e| format!("{e} (judge exit {}: {})", judge.code, first_line(&judge.stderr)))?
            .lines()
            .map(|l| serde_json::from_str(l).map_err(|e| format!("{EVIDENCE_FILE}: {e}")))
            .collect::<Fallible<Vec<Value>>>()?;
        Ok((judge, lines))
    }
}

#[cfg(unix)]
fn link_git(bin: &Path) -> Fallible<()> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let found = std::env::split_paths(&path).map(|d| d.join("git")).find(|g| g.is_file());
    let git = found.ok_or("git is not on PATH")?;
    io(bin, std::os::unix::fs::symlink(git, bin.join("git")))
}

#[cfg(not(unix))]
fn link_git(_bin: &Path) -> Fallible<()> {
    Err("the simulation needs a unix host".to_owned())
}

// ---------------------------------------------------------------------------
// The Actions environment
// ---------------------------------------------------------------------------

/// The event a workflow run was triggered by, and the variables the runner sets.
pub struct Event {
    pub name: &'static str,
    pub mode: &'static str,
    pub sha: String,
    /// When the run ran: both jobs read it as `PROOB_NOW`, so its records are dated then.
    pub at: u64,
    payload: Value,
    vars: BTreeMap<&'static str, String>,
}

impl Event {
    fn new(
        name: &'static str,
        mode: &'static str,
        sha: &str,
        at: u64,
        git_ref: &str,
        workflow: &str,
        payload: Value,
    ) -> Event {
        let vars = BTreeMap::from([
            ("GITHUB_ACTIONS", "true".to_owned()),
            ("GITHUB_EVENT_NAME", name.to_owned()),
            ("GITHUB_REPOSITORY", REPOSITORY.to_owned()),
            ("GITHUB_SHA", sha.to_owned()),
            ("GITHUB_REF", git_ref.to_owned()),
            ("GITHUB_RUN_ATTEMPT", "1".to_owned()),
            ("GITHUB_WORKFLOW_REF", format!("{REPOSITORY}/{workflow}@{git_ref}")),
        ]);
        Event { name, mode, sha: sha.to_owned(), at, payload, vars }
    }

    fn default_branch(name: &'static str, sha: &str, at: u64) -> Event {
        let payload = json!({ "ref": "refs/heads/main", "after": sha, "repository": { "full_name": REPOSITORY, "default_branch": "main" } });
        Event::new(name, "evidence", sha, at, "refs/heads/main", EVIDENCE_WORKFLOW, payload)
    }

    pub fn push(sha: &str, at: u64) -> Event {
        Event::default_branch("push", sha, at)
    }

    pub fn schedule(sha: &str, at: u64) -> Event {
        Event::default_branch("schedule", sha, at)
    }

    /// PR `number` from branch `pr` into main, the checkout at `head`.
    pub fn pull_request(number: u64, base: &str, head: &str, at: u64) -> Event {
        let payload = json!({
            "action": "synchronize", "number": number,
            "pull_request": { "number": number, "base": { "ref": "main", "sha": base }, "head": { "ref": "pr", "sha": head } },
            "repository": { "full_name": REPOSITORY, "default_branch": "main" }
        });
        let mut event =
            Event::new("pull_request", "check", head, at, &format!("refs/pull/{number}/merge"), GATE_WORKFLOW, payload);
        event.vars.insert("GITHUB_BASE_REF", "main".to_owned());
        event.vars.insert("GITHUB_HEAD_REF", "pr".to_owned());
        event
    }

    /// The runner's variables for run `run_id`, its files under `dir`
    /// (shared by both jobs, as one run's summary is).
    fn apply<'a>(&self, cmd: &'a mut Command, dir: &Path, run_id: u64) -> Fallible<&'a mut Command> {
        io(dir, fs::create_dir_all(dir))?;
        let event_path = dir.join("event.json");
        io(&event_path, fs::write(&event_path, self.payload.to_string()))?;
        self.vars.iter().for_each(|(k, v)| {
            cmd.env(k, v);
        });
        Ok(cmd
            .env("GITHUB_EVENT_PATH", event_path)
            .env("GITHUB_RUN_ID", run_id.to_string())
            .env("GITHUB_STEP_SUMMARY", dir.join("summary.md"))
            .env("GITHUB_OUTPUT", dir.join("output.txt"))
            .env("RUNNER_TEMP", dir)
            .env("PROOB_NOW", self.at.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_channel_is_read_from_rust_toolchain_toml() {
        assert_eq!(channel("[toolchain]\nchannel = \"1.99.1\"\ncomponents = []\n"), Some("1.99.1".to_owned()));
        assert_eq!(channel("[toolchain]\n"), None);
    }
}
