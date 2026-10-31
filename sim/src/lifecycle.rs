//! The path a team takes with one property, from declaration to amendment,
//! over a copy of the Rust fixture (`tests/fixtures/rust-repo`): every
//! evidence and gate run is the real `proob action` execute job (cargo-nextest,
//! cargo-mutants) and judge job, in separate checkouts.

use std::fs;
use std::path::Path;

use crate::github::{ALICE, BOB};
use crate::verdicts::Truth::{Unknown, Violated};
use crate::verdicts::{self, Expect, Log, Observed, observe, run_steps};
use crate::world::{Ctx, DAY, EVIDENCE_WORKFLOW, Event, Fallible, GATE_WORKFLOW, NOW, World, copy_tree};

/// "Sorting preserves length", bound to `sorted_preserves_length` in tests/props.rs.
pub const A: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XHA";
pub const PROPERTY: &str = "proob/properties/01K8V5NQ4A0F3R7Y2M9D6P1XHA.toml";
pub const BOUND_TEST: &str = "tests/props.rs";

/// The code that predates proob, pushed to main before anyone declared anything.
const BEFORE: [&str; 4] = ["Cargo.toml", "Cargo.lock", ".gitignore", "src"];
/// What PR #1 adds: the declaration, its checks, and both workflows.
pub const INTRODUCED: [&str; 8] = [
    "proob/config.toml",
    PROPERTY,
    GATE_WORKFLOW,
    EVIDENCE_WORKFLOW,
    BOUND_TEST,
    "tests/common/mod.rs",
    "tests/unrelated.rs",
    "tests/vacuous.rs",
];
const GATE_YML: &str = "name: proob gate\non: pull_request\njobs: {}\n";
const EVIDENCE_YML: &str = "name: proob evidence\non: [push, schedule]\njobs: {}\n";

pub type Step = verdicts::Step<World>;

/// Truth: the code is correct at every step but the breaking PR, whose
/// `out.pop()` is a real counterexample; elsewhere it is `Unknown`, since A's
/// check is a test: even with bob's approval, a passing push run, and a score
/// that kills, it is tested on sampled inputs, not proved.
pub const STEPS: [(Expect, Step); 7] = [
    (Expect::status("declared on a branch", "Proposed", Unknown), declared),
    (Expect::status("merged with an eligible second reviewer", "Not checked", Unknown), merged),
    (
        Expect::status(
            "default-branch evidence run (push: execute, judge)",
            "Checked (tested, strength pending)",
            Unknown,
        ),
        push_run,
    ),
    (Expect::status("scheduled score run (cargo-mutants)", "Checked (tested)", Unknown), scheduled_run),
    (
        Expect::blocks("a PR that breaks the code (gate judge)", "Broken", Violated)
            .because("counterexample on this PR"),
        breaking_pr,
    ),
    (
        Expect::status(
            "merge reviewed only by its author (allow_self_signed)",
            "Checked (tested, self-signed)",
            Unknown,
        ),
        self_signed,
    ),
    (Expect::status("the bound test amended on a branch", "Re-approve (check amended)", Unknown), amended_test),
];

pub fn run(ctx: &Ctx, log: &Log) -> Fallible<Vec<String>> {
    let mut world = World::new(ctx)?;
    pre_proob(&world)?;
    run_steps("lifecycle", log, &mut world, &STEPS)
}

fn fixture(ctx: &Ctx) -> std::path::PathBuf {
    ctx.root.join("tests/fixtures/rust-repo")
}

/// Copies fixture paths (files or directories) into the checkout.
fn bring(w: &World, paths: &[&str]) -> Fallible<()> {
    paths.iter().filter(|p| !p.starts_with(".github/")).try_for_each(|p| {
        let (from, to) = (fixture(&w.ctx).join(p), w.repo.dir.join(p));
        match from.is_dir() {
            true => copy_tree(&from, &to),
            false => copy_file(&from, &to),
        }
    })
}

fn copy_file(from: &Path, to: &Path) -> Fallible<()> {
    let parent = to.parent().ok_or("a path with no parent")?;
    fs::create_dir_all(parent)
        .and_then(|()| fs::copy(from, to))
        .map(drop)
        .map_err(|e| format!("{}: {e}", from.display()))
}

/// main before proob: the code alone, pushed directly.
pub fn pre_proob(w: &World) -> Fallible<()> {
    bring(w, &BEFORE)?;
    w.repo.commit("the code, before proob", &ALICE, NOW - 20 * DAY).map(drop)
}

/// Branch `declare`: alice's PR #1 adds the declaration (one property), its
/// bound tests, and both workflows.
pub fn declare(w: &World) -> Fallible<String> {
    w.repo.checkout(&["-b", "declare"])?;
    bring(w, &INTRODUCED)?;
    w.repo.write(GATE_WORKFLOW, GATE_YML)?;
    w.repo.write(EVIDENCE_WORKFLOW, EVIDENCE_YML)?;
    w.repo.commit("declare: sorting preserves length", &ALICE, NOW - 10 * DAY)
}

/// PR #1, adding `files`, merges with bob's approval at its head: an eligible second reviewer.
pub fn merge_declaration(w: &mut World, files: &[&str]) -> Fallible<String> {
    let (sha, _) = merge_unreviewed(w, files)?;
    w.github.review(1, 101, &BOB, &sha);
    Ok(sha)
}

/// PR #1, adding `files`, merges with no review yet; its merge and parent shas.
pub fn merge_unreviewed(w: &mut World, files: &[&str]) -> Fallible<(String, String)> {
    let parent = w.repo.rev("main")?;
    let sha = merge(w, "declare")?;
    w.github.merged_pr(1, &ALICE, &sha, &parent, files);
    publish_blobs(w, &sha, files)?;
    Ok((sha, parent))
}

/// Fast-forwards main to `branch` (a squash merge of a one-commit PR).
fn merge(w: &World, branch: &str) -> Fallible<String> {
    w.repo.checkout(&["main"])?;
    w.repo.git(&["merge", "-q", "--ff-only", branch])?;
    w.repo.rev("main")
}

/// The contents API's answer for each path at `sha`: the blob git has.
fn publish_blobs(w: &mut World, sha: &str, paths: &[&str]) -> Fallible<()> {
    paths.iter().try_for_each(|p| {
        let blob = w.repo.blob(sha, p)?;
        w.github.blob(sha, p, Some(&blob));
        Ok(())
    })
}

fn status_of_a(w: &World) -> Fallible<Observed> {
    observe(&w.status()?, A)
}

// ---------------------------------------------------------------------------
// The steps
// ---------------------------------------------------------------------------

fn declared(w: &mut World) -> Fallible<Observed> {
    declare(w)?;
    status_of_a(w)
}

fn merged(w: &mut World) -> Fallible<Observed> {
    merge_declaration(w, &INTRODUCED)?;
    status_of_a(w)
}

fn push_run(w: &mut World) -> Fallible<Observed> {
    w.evidence_run(&Event::push(&w.repo.rev("main")?, NOW - 5 * DAY), 201)?;
    status_of_a(w)
}

fn scheduled_run(w: &mut World) -> Fallible<Observed> {
    w.evidence_run(&Event::schedule(&w.repo.rev("main")?, NOW - 4 * DAY), 202)?;
    status_of_a(w)
}

/// PR #2 makes `sorted` drop an element; the gate's execute job runs the bound
/// test, the judge blocks. The PR is abandoned and main is untouched.
fn breaking_pr(w: &mut World) -> Fallible<Observed> {
    let base = w.repo.rev("main")?;
    w.repo.checkout(&["-b", "break"])?;
    let code = w.repo.read("src/ordering.rs")?;
    w.repo.write("src/ordering.rs", &code.replace("out.sort_unstable();", "out.sort_unstable();\n    out.pop();"))?;
    let head = w.repo.commit("sorted: drop the largest element", &ALICE, NOW - 3 * DAY - 3600)?;
    w.github.open_pr(2, &ALICE, &head, &base, &["src/ordering.rs"]);
    let (judge, _) = w.workflow_run(&Event::pull_request(2, &base, &head, NOW - 3 * DAY), 203)?;
    w.repo.checkout(&["main"])?;
    observe(&judge, A)
}

/// PR #3 rewords the property's rationale; its only approval is alice's own.
/// The config allows self-signed merges. The merge triggers a push run.
fn self_signed(w: &mut World) -> Fallible<Observed> {
    let parent = w.repo.rev("main")?;
    w.repo.checkout(&["-b", "reword"])?;
    let text = w.repo.read(PROPERTY)?;
    w.repo.write(PROPERTY, &text.replace("would corrupt them.", "would corrupt them silently."))?;
    w.repo.commit("reword the rationale", &ALICE, NOW - 2 * DAY - 3600)?;
    let sha = merge(w, "reword")?;
    w.github.merged_pr(3, &ALICE, &sha, &parent, &[PROPERTY]).review(3, 301, &ALICE, &sha);
    publish_blobs(w, &sha, &[PROPERTY])?;
    w.evidence_run(&Event::push(&sha, NOW - 2 * DAY), 204)?;
    status_of_a(w)
}

/// A branch widens the strategy the bound test draws from.
fn amended_test(w: &mut World) -> Fallible<Observed> {
    w.repo.checkout(&["-b", "amend-test"])?;
    let text = w.repo.read("tests/common/mod.rs")?;
    w.repo.write("tests/common/mod.rs", &text.replace("0..8)", "0..16)"))?;
    w.repo.commit("tests: longer vectors", &ALICE, NOW - DAY)?;
    status_of_a(w)
}
