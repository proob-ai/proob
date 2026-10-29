//! The scenarios whose stand-ins already exist: the shell suites (a fake
//! release server, Claude Code's hook protocol), `act` running ci.yml's jobs in
//! Docker as a GitHub runner would, and `dist plan` for the release.

use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::outcome::Outcome;
use crate::world::{Ctx, first_line};

pub fn shim(ctx: &Ctx) -> Outcome {
    suite(ctx, "tests/shim/test_shim.sh")
}

pub fn hooks(ctx: &Ctx) -> Outcome {
    suite(ctx, "tests/hooks/test_hooks.sh")
}

/// A shell suite, its output passed through.
fn suite(ctx: &Ctx, script: &str) -> Outcome {
    match Command::new(ctx.root.join(script)).current_dir(&ctx.root).status() {
        Ok(s) if s.success() => Outcome::Ok(script.to_owned()),
        Ok(s) => Outcome::Failed(format!("{script}: exit {}", s.code().unwrap_or(-1))),
        Err(e) => Outcome::Failed(format!("{script}: {e}")),
    }
}

/// The ci.yml jobs `act` runs: the layering check, and the action's own
/// execute-then-judge self-test (`uses: ./`).
const ACT_JOBS: [&str; 2] = ["crate-graph", "action-self-test"];
const IMAGE: &str = "catthehacker/ubuntu:act-latest";
const PROBE_SECS: u64 = 30;
const PULL_SECS: u64 = 600;
const JOB_SECS: u64 = 1800;

/// What a command came to within its deadline.
#[derive(Debug, PartialEq, Eq)]
enum Probe {
    Works,
    Fails,
    Missing,
    TimedOut,
}

/// Docker, then the runner image (pulled when absent), then each job. A
/// stand-in this machine cannot provide is a skip with its reason; only a job
/// that ran and failed fails.
pub fn workflows(ctx: &Ctx) -> Outcome {
    let quiet = |args: &[&str], secs| within(Command::new("docker").args(args), secs, None);
    match (quiet(&["info"], PROBE_SECS), within(Command::new("act").arg("--version"), PROBE_SECS, None)) {
        (Probe::Missing, _) => Outcome::Skipped("docker not installed".to_owned()),
        (Probe::Fails | Probe::TimedOut, _) => Outcome::Skipped("docker not running".to_owned()),
        (_, Probe::Works) => match runner_image(&quiet) {
            Ok(()) => jobs(ctx),
            Err(why) => Outcome::Skipped(why),
        },
        (_, _) => Outcome::Skipped("act not installed".to_owned()),
    }
}

fn runner_image(docker: &dyn Fn(&[&str], u64) -> Probe) -> Result<(), String> {
    match docker(&["image", "inspect", IMAGE], PROBE_SECS) {
        Probe::Works => Ok(()),
        Probe::TimedOut => Err(format!("docker did not answer an image query in {PROBE_SECS}s")),
        _ => {
            println!("  docker pull {IMAGE}");
            match docker(&["pull", "-q", IMAGE], PULL_SECS) {
                Probe::Works => Ok(()),
                Probe::TimedOut => {
                    Err(format!("runner image {IMAGE} absent; docker pull timed out after {PULL_SECS}s"))
                }
                _ => Err(format!("runner image {IMAGE} absent; docker pull failed")),
            }
        }
    }
}

fn jobs(ctx: &Ctx) -> Outcome {
    ACT_JOBS.iter().map(|job| act(ctx, job)).find(|o| !o.is_ok()).unwrap_or_else(|| Outcome::Ok(ACT_JOBS.join(", ")))
}

/// Runs `cmd` to completion or kills it at the deadline; output to `log`, or nowhere.
fn within(cmd: &mut Command, secs: u64, log: Option<&Path>) -> Probe {
    let sink = |l: Option<&Path>| l.and_then(|p| fs::File::create(p).ok()).map_or_else(Stdio::null, Stdio::from);
    let child = cmd
        .stdin(Stdio::null())
        .stdout(sink(log))
        .stderr(sink(log.map(|p| p.with_extension("err")).as_deref()))
        .spawn();
    match child {
        Err(e) if e.kind() == ErrorKind::NotFound => Probe::Missing,
        Err(_) => Probe::Fails,
        Ok(mut child) => wait(&mut child, Instant::now() + Duration::from_secs(secs)),
    }
}

fn wait(child: &mut Child, deadline: Instant) -> Probe {
    loop {
        match child.try_wait() {
            Ok(Some(s)) if s.success() => return Probe::Works,
            Ok(Some(_)) | Err(_) => return Probe::Fails,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe::TimedOut;
            }
            Ok(None) => thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// One job of ci.yml on a push event, on the image already present; its log
/// under target/sim/.
fn act(ctx: &Ctx, job: &str) -> Outcome {
    let log = ctx.root.join(format!("target/sim/act-{job}.log"));
    println!("  act push -W .github/workflows/ci.yml -j {job}  (log: {})", log.display());
    let platform = format!("ubuntu-latest={IMAGE}");
    let args = ["push", "-W", ".github/workflows/ci.yml", "-j", job, "-P", &platform, "--pull=false"];
    match within(Command::new("act").args(args).current_dir(&ctx.root), JOB_SECS, Some(&log)) {
        Probe::Works => Outcome::Ok(job.to_owned()),
        Probe::TimedOut => Outcome::Failed(format!("act {job}: timed out after {JOB_SECS}s")),
        _ => {
            Outcome::Failed(format!("act {job}: failed ({})", last_line(&fs::read_to_string(&log).unwrap_or_default())))
        }
    }
}

fn last_line(text: &str) -> &str {
    text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("")
}

/// The apps and targets dist-workspace.toml releases.
const APPS: [&str; 1] = ["proob"];
const TARGETS: [&str; 5] = [
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-musl",
];

pub fn release(ctx: &Ctx) -> Outcome {
    let out = Command::new("dist").args(["plan", "--output-format=json"]).current_dir(&ctx.root).output();
    match out {
        Err(e) if e.kind() == ErrorKind::NotFound => Outcome::Skipped("dist not installed".to_owned()),
        Err(e) => Outcome::Failed(format!("dist plan: {e}")),
        Ok(o) if !o.status.success() => {
            Outcome::Failed(format!("dist plan: {}", first_line(&String::from_utf8_lossy(&o.stderr))))
        }
        Ok(o) => match serde_json::from_slice(&o.stdout) {
            Ok(plan) => plan_outcome(&plan),
            Err(e) => Outcome::Failed(format!("dist plan: not JSON: {e}")),
        },
    }
}

/// Only `proob` is released, as one archive for each of the five targets.
fn plan_outcome(plan: &Value) -> Outcome {
    let (apps, targets) = released(plan);
    let want_apps: BTreeSet<String> = APPS.iter().map(|s| (*s).to_owned()).collect();
    let want_targets: BTreeSet<String> = TARGETS.iter().map(|s| (*s).to_owned()).collect();
    match (apps == want_apps, targets == want_targets) {
        (true, true) => Outcome::Ok(format!("{} on {} targets", APPS.join(", "), TARGETS.len())),
        _ => Outcome::Failed(format!("dist plan releases {apps:?} for {targets:?}")),
    }
}

fn released(plan: &Value) -> (BTreeSet<String>, BTreeSet<String>) {
    let strings = |v: &Value| v.as_str().map(str::to_owned);
    let apps = plan["releases"].as_array().into_iter().flatten().filter_map(|r| strings(&r["app_name"])).collect();
    let targets = plan["artifacts"]
        .as_object()
        .into_iter()
        .flat_map(|a| a.values())
        .filter(|a| a["kind"] == "executable-zip")
        .flat_map(|a| a["target_triples"].as_array().into_iter().flatten().filter_map(strings))
        .collect();
    (apps, targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan(apps: &[&str], targets: &[&str]) -> Value {
        let artifacts: serde_json::Map<String, Value> = targets
            .iter()
            .map(|t| (format!("proob-{t}.tar.xz"), json!({ "kind": "executable-zip", "target_triples": [t] })))
            .chain([(
                "proob-installer.sh".to_owned(),
                json!({ "kind": "installer", "target_triples": ["x86_64-unknown-linux-gnu"] }),
            )])
            .collect();
        json!({ "releases": apps.iter().map(|a| json!({ "app_name": a })).collect::<Vec<_>>(), "artifacts": artifacts })
    }

    #[test]
    fn within_reports_success_failure_absence_and_the_deadline() {
        assert_eq!(within(&mut Command::new("true"), 5, None), Probe::Works);
        assert_eq!(within(&mut Command::new("false"), 5, None), Probe::Fails);
        assert_eq!(within(&mut Command::new("no-such-program-sim"), 5, None), Probe::Missing);
        assert_eq!(within(Command::new("sleep").arg("5"), 0, None), Probe::TimedOut);
    }

    #[test]
    fn an_image_docker_cannot_produce_is_a_reason_to_skip() {
        let hung = |_: &[&str], _| Probe::TimedOut;
        assert!(runner_image(&hung).unwrap_err().contains("did not answer"));
        let unpullable = |args: &[&str], _| if args[0] == "image" { Probe::Fails } else { Probe::TimedOut };
        assert!(runner_image(&unpullable).unwrap_err().contains("docker pull timed out"));
        assert_eq!(runner_image(&|_: &[&str], _| Probe::Works), Ok(()));
    }

    #[test]
    fn the_plan_passes_with_proob_on_the_five_targets_alone() {
        assert!(plan_outcome(&plan(&APPS, &TARGETS)).is_ok());
    }

    #[test]
    fn another_app_or_target_fails_the_plan() {
        assert!(!plan_outcome(&plan(&["proob", "sim"], &TARGETS)).is_ok());
        assert!(!plan_outcome(&plan(&APPS, &TARGETS[..4])).is_ok());
    }
}
