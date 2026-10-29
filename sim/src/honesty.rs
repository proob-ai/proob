//! Adversarial worlds that must never render `Holds`. Each case starts from
//! the same approved declaration (lifecycle's PR #1), adds the evidence a
//! GitHub would serve, and changes one thing. The control case changes
//! nothing and must render `Holds`, so the others are not vacuously honest.
//!
//! Evidence here is what an evidence run would have uploaded: records at the
//! hashes `proob hash` reports, served as a run's artifact. No toolchain runs,
//! so the scenario takes seconds; lifecycle runs the real jobs.

use serde_json::{Value, json};

use crate::github::{AGENT, BOB, rate_limited, unreachable};
use crate::lifecycle::{A, BOUND_TEST, declare, merge_declaration, pre_proob};
use crate::verdicts::Truth::{Holds, Unknown};
use crate::verdicts::{self, Expect, Log, Observed, observe, run_steps};
use crate::world::{Ctx, DAY, EVIDENCE_WORKFLOW, Fallible, NOW, World, run};

/// One evidence run as GitHub lists it.
#[derive(Clone, Copy)]
struct Upload {
    run_id: u64,
    event: &'static str,
    branch: &'static str,
    at: u64,
    /// The check record's outcome is always "no counterexample".
    check: bool,
    /// A score record with this many mutants killed (of four).
    killed: Option<u64>,
}

const PUSH: Upload = Upload { run_id: 301, event: "push", branch: "main", at: NOW - DAY, check: true, killed: None };
const SCHEDULE: Upload =
    Upload { run_id: 302, event: "schedule", branch: "main", at: NOW - DAY / 2, check: true, killed: Some(3) };

pub type Case = verdicts::Step<Ctx>;

/// Truth: the code is correct in every case; only the control's world holds
/// all the claim rests on. In the rest one fact is missing (an approved bound
/// test, default-branch evidence, a fresh or killing score, a reviewer with
/// write access, a readable GitHub), so the truth is `Unknown`.
pub const CASES: [(Expect, Case); 9] = [
    (Expect::status("control: approved, fresh check and score", "Holds", Holds), control),
    (
        Expect::status("agent commit pushed directly to main, fresh evidence", "Re-approve (check amended)", Unknown),
        agent_direct_push,
    ),
    (Expect::status("evidence only from a pull_request run", "Not checked", Unknown), pr_run_evidence),
    (Expect::status("evidence only from a non-default branch", "Not checked", Unknown), branch_evidence),
    (Expect::status("score older than freshness_days", "Checked (strength pending)", Unknown), stale_score),
    (
        Expect::status("the approving reviewer lost write access", "Re-approve (reviewer access revoked)", Unknown),
        access_revoked,
    ),
    (Expect::status("vacuous score: killed 0", "Checked (strength pending)", Unknown), vacuous_score),
    (Expect::unverified("GitHub unreachable", "Not checked", Unknown), github_unreachable),
    (
        Expect::unverified("GitHub rate-limited (403, no calls remaining)", "Unverified (rate limited)", Unknown),
        github_rate_limited,
    ),
];

pub fn run_cases(ctx: &Ctx, log: &Log) -> Fallible<Vec<String>> {
    run_steps("honesty", log, &mut ctx.clone(), &CASES)
}

/// main after PR #1, approved by bob: no evidence yet.
fn approved(ctx: &Ctx) -> Fallible<World> {
    let mut w = World::new(ctx)?;
    pre_proob(&w)?;
    declare(&w)?;
    merge_declaration(&mut w)?;
    Ok(w)
}

pub fn record(
    hashes: &Value,
    kind: &str,
    outcome: Value,
    run_id: u64,
    head: &str,
    workflow_blob: &str,
    at: u64,
) -> Value {
    json!({
        "schema_version": 1, "id": A, "kind": kind, "algo": "sha256",
        "content_hash": hashes["content_hash"], "binding_hash": hashes["binding_hash"], "scope_hash": hashes["scope_hash"],
        "outcome": outcome, "tool_versions": { "proptest": "1.12.0", "cargo-mutants": "27.1.0" },
        "run_id": run_id, "head_sha": head, "workflow_blob": workflow_blob, "at": at
    })
}

/// The run's records at main's head and the checkout's current hashes.
fn upload(w: &mut World, u: Upload) -> Fallible<()> {
    let hashes = w.hashes(A)?;
    let head = w.repo.rev("main")?;
    let blob = w.repo.blob("main", EVIDENCE_WORKFLOW)?;
    let check = u.check.then(|| json!({ "result": "no_counterexample", "budget": 256 }));
    let score =
        u.killed.map(|k| json!({ "result": "scored", "killed": k, "survived": 4 - k, "timeout": 0, "unviable": 0 }));
    let lines = [check.map(|o| ("check", o)), score.map(|o| ("score", o))]
        .into_iter()
        .flatten()
        .map(|(kind, o)| record(&hashes, kind, o, u.run_id, &head, &blob, u.at))
        .collect();
    w.github.evidence_run(u.run_id, &head, u.event, u.branch, u.at, lines);
    Ok(())
}

fn with_uploads(ctx: &Ctx, uploads: &[Upload]) -> Fallible<World> {
    let mut w = approved(ctx)?;
    uploads.iter().try_for_each(|u| upload(&mut w, *u))?;
    Ok(w)
}

fn status_of_a(w: &World) -> Fallible<Observed> {
    observe(&w.status()?, A)
}

// ---------------------------------------------------------------------------
// The cases
// ---------------------------------------------------------------------------

fn control(ctx: &mut Ctx) -> Fallible<Observed> {
    status_of_a(&with_uploads(ctx, &[PUSH, SCHEDULE])?)
}

/// A coding agent weakens the bound test and pushes it straight to main; the
/// evidence workflow then runs green at the new hashes.
fn agent_direct_push(ctx: &mut Ctx) -> Fallible<Observed> {
    let mut w = approved(ctx)?;
    let test = w.repo.read(BOUND_TEST)?;
    w.repo.write(
        BOUND_TEST,
        &test.replace("prop_assert_eq!(sorted(&v).len(), v.len());", "prop_assert!(sorted(&v).len() <= v.len() + 1);"),
    )?;
    w.repo.commit("tests: relax the length check", &AGENT, NOW - 2 * DAY)?;
    [PUSH, SCHEDULE].iter().try_for_each(|u| upload(&mut w, *u))?;
    status_of_a(&w)
}

fn pr_run_evidence(ctx: &mut Ctx) -> Fallible<Observed> {
    let from_pr = |u: Upload| Upload { event: "pull_request", ..u };
    status_of_a(&with_uploads(ctx, &[from_pr(PUSH), from_pr(SCHEDULE)])?)
}

fn branch_evidence(ctx: &mut Ctx) -> Fallible<Observed> {
    let on_branch = |u: Upload| Upload { branch: "feature", ..u };
    status_of_a(&with_uploads(ctx, &[on_branch(PUSH), on_branch(SCHEDULE)])?)
}

/// The fixture config's freshness_days is 14; the only score is 15 days old.
fn stale_score(ctx: &mut Ctx) -> Fallible<Observed> {
    let old = Upload { at: NOW - 15 * DAY, ..SCHEDULE };
    status_of_a(&with_uploads(ctx, &[old, PUSH])?)
}

fn access_revoked(ctx: &mut Ctx) -> Fallible<Observed> {
    let mut w = with_uploads(ctx, &[PUSH, SCHEDULE])?;
    w.github.permission(&BOB, "read");
    status_of_a(&w)
}

fn vacuous_score(ctx: &mut Ctx) -> Fallible<Observed> {
    status_of_a(&with_uploads(ctx, &[PUSH, Upload { killed: Some(0), ..SCHEDULE }])?)
}

/// The world of the control case, read through proob's REST client and a token.
fn live(ctx: &Ctx, api_url: &str) -> Fallible<Observed> {
    let w = with_uploads(ctx, &[PUSH, SCHEDULE])?;
    observe(&run(w.proob_live(&w.repo.dir, api_url).args(["status", "--json"]))?, A)
}

fn github_unreachable(ctx: &mut Ctx) -> Fallible<Observed> {
    live(ctx, &unreachable()?)
}

fn github_rate_limited(ctx: &mut Ctx) -> Fallible<Observed> {
    live(ctx, &rate_limited()?)
}
