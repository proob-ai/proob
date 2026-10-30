//! Adversarial worlds that must never render `Holds`. Each case starts from
//! the same approved declaration (lifecycle's PR #1, which here also declares
//! `P`, A's statement as a proof), adds the evidence a GitHub would serve, and
//! changes one thing. The control case changes nothing and must render
//! `Holds`, so the others are not vacuously honest; only a proof or a model
//! can, so the control is about `P`. Its twin gives test-kind `A` the same
//! facts and must render `Checked (tested)`.
//!
//! Evidence here is what an evidence run would have uploaded: records at the
//! hashes `proob hash` reports, served as a run's artifact. No toolchain runs,
//! so the scenario takes seconds; lifecycle runs the real jobs. `P`'s proof
//! record says `verified` because the code is the fixture's correct `sorted`,
//! whose bound test passes in lifecycle: no verifier runs here (CI's test job
//! has no Verus), and `P` is bound through the fixture's `bindings.json`.

use serde_json::{Value, json};

use crate::github::{AGENT, BOB, rate_limited, unreachable};
use crate::lifecycle::{A, BOUND_TEST, INTRODUCED, declare, merge_declaration, pre_proob};
use crate::verdicts::Truth::{Holds, Unknown};
use crate::verdicts::{self, Expect, Log, Observed, observe, run_steps};
use crate::world::{Ctx, DAY, EVIDENCE_WORKFLOW, Fallible, NOW, World, run};

/// "Sorting preserves length", proved: A's statement with `kind = "proof"`,
/// bound to a proof file PR #1 adds. A stays a test (its binding is a test,
/// and `derive` admits only records of a property's own kind), so `Holds` is
/// reachable only through a second property.
pub const P: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHP";
const P_PROPERTY: &str = "proob/properties/01K8V5NQ4A0F3R7Y2M9D6P1XHP.toml";
const P_PROOF: &str = "proofs/ordering.rs";
const P_TOML: &str = r#"id = "01K8V5NQ4A0F3R7Y2M9D6P1XHP"
title = "Sorting preserves length, proved"
statement = "Sorting a list never adds or drops an element."
scope = ["src/ordering.rs"]
severity = "medium"
rationale = "Callers size buffers from the input; a proof covers every input, where a test samples some."
kind = "proof"
"#;
/// Outside every crate root, so cargo never builds it and no adapter discovers it.
/// A stand-in file the fixture binds, never compiled or verified: not valid Verus.
const P_RS: &str = "// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHP\nproof fn sorted_preserves_length(v: Seq<u32>) ensures sorted(v).len() == v.len() {}\n";
/// The verifier the CI pins (`PROOB_VERUS_VERSION`), and the z3 it ships with.
const VERUS: &str = "0.2026.10.26.d17fbd0";
const Z3: &str = "4.12.5";

/// One evidence run as GitHub lists it.
#[derive(Clone, Copy)]
struct Upload {
    run_id: u64,
    event: &'static str,
    branch: &'static str,
    at: u64,
    /// Whose records the run's artifact holds: `A` or `P`.
    of: &'static str,
    /// The check record's outcome is always a pass: "no counterexample", or for `P` "verified".
    check: bool,
    /// A score record with this many mutants killed (of four).
    killed: Option<u64>,
}

const PUSH: Upload =
    Upload { run_id: 301, event: "push", branch: "main", at: NOW - DAY, of: A, check: true, killed: None };
const SCHEDULE: Upload =
    Upload { run_id: 302, event: "schedule", branch: "main", at: NOW - DAY / 2, of: A, check: true, killed: Some(3) };

pub type Case = verdicts::Step<Ctx>;

/// Truth: the code is correct in every case; only the control's world holds
/// all the claim rests on. In the rest one fact is missing (a proof rather
/// than a test, an approved bound check, default-branch evidence, a fresh or
/// killing score, a reviewer with write access, a readable GitHub), so the
/// truth is `Unknown`.
pub const CASES: [(Expect, Case); 10] = [
    (Expect::status("control: P approved, fresh proof and score", "Holds", Holds), control),
    (Expect::status("twin: A approved, the control's facts on a test", "Checked (tested)", Unknown), tested_twin),
    (
        Expect::status("agent commit pushed directly to main, fresh evidence", "Re-approve (check amended)", Unknown),
        agent_direct_push,
    ),
    (Expect::status("evidence only from a pull_request run", "Not checked", Unknown), pr_run_evidence),
    (Expect::status("evidence only from a non-default branch", "Not checked", Unknown), branch_evidence),
    (Expect::status("score older than freshness_days", "Checked (tested, strength pending)", Unknown), stale_score),
    (
        Expect::status("the approving reviewer lost write access", "Re-approve (reviewer access revoked)", Unknown),
        access_revoked,
    ),
    (Expect::status("vacuous score: killed 0", "Checked (tested, strength pending)", Unknown), vacuous_score),
    (Expect::unverified("GitHub unreachable", "Not checked", Unknown), github_unreachable),
    (
        Expect::unverified("GitHub rate-limited (403, no calls remaining)", "Unverified (rate limited)", Unknown),
        github_rate_limited,
    ),
];

pub fn run_cases(ctx: &Ctx, log: &Log) -> Fallible<Vec<String>> {
    run_steps("honesty", log, &mut ctx.clone(), &CASES)
}

/// main after PR #1, approved by bob: no evidence yet. A and P are bound.
fn approved(ctx: &Ctx) -> Fallible<World> {
    let mut w = World::new(ctx)?;
    pre_proob(&w)?;
    propose_proof(&w)?;
    declare(&w)?;
    merge_declaration(&mut w, &introduced())?;
    bind_proof(&w)?;
    Ok(w)
}

/// P's declaration and proof file in the tree, for `declare` to commit into PR #1.
pub fn propose_proof(w: &World) -> Fallible<()> {
    w.repo.write(P_PROPERTY, P_TOML)?;
    w.repo.write(P_PROOF, P_RS)
}

/// What PR #1 adds when it declares P too.
pub fn introduced() -> Vec<&'static str> {
    [&INTRODUCED[..], &[P_PROPERTY, P_PROOF]].concat()
}

/// Writes `bindings.json`: A's binding as the Rust adapter discovers it
/// (read through `proob hash` while discovery still runs) and P's proof file.
/// A's binding hash must come out the same, or its evidence would go stale.
pub fn bind_proof(w: &World) -> Fallible<()> {
    let discovered = w.hashes(A)?;
    w.bindings(&json!({ A: { "files": discovered["binding_files"] }, P: { "files": [P_PROOF] } }))?;
    let bound = w.hashes(A)?;
    let same = |k: &str| bound[k] == discovered[k];
    match same("binding_hash") && same("binding_files") {
        true => Ok(()),
        false => Err(format!("bindings.json moves A's binding: discovered {discovered}, bound {bound}")),
    }
}

/// One record of the property `hashes` were reported for.
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
        "schema_version": 1, "id": hashes["id"], "kind": kind, "algo": "sha256",
        "content_hash": hashes["content_hash"], "binding_hash": hashes["binding_hash"], "scope_hash": hashes["scope_hash"],
        "outcome": outcome, "tool_versions": tools(kind),
        "run_id": run_id, "head_sha": head, "workflow_blob": workflow_blob, "at": at
    })
}

/// A proof record names the verifier; every other record, P's score included
/// (borrowed from cargo-mutants on A's test), the test runner and the mutation tool.
fn tools(kind: &str) -> Value {
    match kind {
        "proof" => json!({ "verus": VERUS, "z3": Z3 }),
        _ => json!({ "proptest": "1.12.0", "cargo-mutants": "27.1.0" }),
    }
}

/// A passing check of `id`: a test that found no counterexample, or P's proof.
fn passing(id: &str) -> (&'static str, Value) {
    match id {
        P => ("proof", json!({ "result": "verified", "rlimit": 10 })),
        _ => ("check", json!({ "result": "no_counterexample", "budget": 256 })),
    }
}

/// The run's records at main's head and the checkout's current hashes.
fn upload(w: &mut World, u: Upload) -> Fallible<()> {
    let hashes = w.hashes(u.of)?;
    let head = w.repo.rev("main")?;
    let blob = w.repo.blob("main", EVIDENCE_WORKFLOW)?;
    let check = u.check.then(|| passing(u.of));
    let score =
        u.killed.map(|k| json!({ "result": "scored", "killed": k, "survived": 4 - k, "timeout": 0, "unviable": 0 }));
    let lines = [check, score.map(|o| ("score", o))]
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
    let of_p = |u: Upload| Upload { of: P, ..u };
    observe(&with_uploads(ctx, &[of_p(PUSH), of_p(SCHEDULE)])?.status()?, P)
}

/// The control's facts on A: approved, a fresh passing check and a killing score, but a test.
fn tested_twin(ctx: &mut Ctx) -> Fallible<Observed> {
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

/// The twin's world, read through proob's REST client and a token.
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
