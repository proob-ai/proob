//! The soundness matrix: every combination of the facts proob reads about one
//! property, over code whose truth is real, each world recording what proob
//! rendered beside the truth and asserting nothing about the word.
//!
//! The axes vary independently: the kind, test-kind `A` or proof-kind `P`
//! (honesty's) (2), approval of PR #1 (5), where and when the check ran (6),
//! the score (4), and the code (2): 2 x 5 x 6 x 4 x 2 = 480 worlds. Only a
//! proof can render `Holds`, so `P` crosses every axis too: a wrong `Holds`
//! can only come from a `P` world.
//! The code is real: `Correct` is the fixture's `sorted` with a
//! behaviour-preserving edit (`sort` for `sort_unstable`), `Planted` drops the
//! largest element, a counterexample to "sorting preserves length". Both are
//! confirmed by running the bound test with plain `cargo test`, not proob.
//! A check record's outcome is never chosen: it is what the real evidence job
//! recorded at that commit, so a positive record on planted code (a CI that
//! lies) is not a world here, and a negative one is `Fresh` x `Planted`.
//! P's proof record is derived from that same real run at the same commit,
//! never chosen (`verified` where it found no counterexample, `proof_failed`
//! where it found one): no verifier runs here, since CI's test job has no Verus.
//!
//! One repository is built and the real jobs run four times (the check at the
//! merge, at each head, and cargo-mutants at the correct head); each world
//! then varies only the fixture GitHub, with main reset to its head.

use std::fs;
use std::io::ErrorKind;

use serde_json::{Value, json};

use crate::github::{ALICE, BOB, GitHub};
use crate::honesty::{P, bind_proof, introduced, propose_proof, record};
use crate::lifecycle::{A, declare, merge_unreviewed, pre_proob};
use crate::verdicts::{Log, Truth, observe};
use crate::world::{Ctx, DAY, EVIDENCE_WORKFLOW, Event, Fallible, NOW, Repo, World, first_line, run};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// bob, a second human with write access, approved the merge head.
    Team,
    /// alice approved her own PR; the fixture config allows self-signed merges.
    SelfSigned,
    Unreviewed,
    /// bob approved, but he also authored a commit of the PR that touched the governed files.
    AuthorOnly,
    /// bob approved, then lost write access.
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// A push run on main at the head, a day ago.
    Fresh,
    /// A push run on main at the merge (10 days ago), before the head's edit to the scope (8 days ago).
    EarlierCommit,
    /// The `Fresh` run, read 15 days later: older than freshness_days (14).
    Stale,
    /// A pull_request run at the head.
    PrOnly,
    /// A push run at the head on a branch that is not the default.
    OtherBranch,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strength {
    /// The real cargo-mutants record: killed at least the floor (1).
    AtFloor,
    /// Every mutant survived.
    KilledZero,
    NotScored,
    /// Every mutant beyond the cap: none verified.
    UnsampledOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Correct,
    Planted,
}

/// Which property the world is about: `A`, bound to a test, or `P`, its statement bound to a proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Test,
    Proof,
}

impl Kind {
    fn id(self) -> &'static str {
        match self {
            Kind::Test => A,
            Kind::Proof => P,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub kind: Kind,
    pub approval: Approval,
    pub evidence: Evidence,
    pub strength: Strength,
    pub code: Code,
}

const APPROVALS: [Approval; 5] =
    [Approval::Team, Approval::SelfSigned, Approval::Unreviewed, Approval::AuthorOnly, Approval::Revoked];
const EVIDENCE: [Evidence; 6] = [
    Evidence::Fresh,
    Evidence::EarlierCommit,
    Evidence::Stale,
    Evidence::PrOnly,
    Evidence::OtherBranch,
    Evidence::Absent,
];
const STRENGTHS: [Strength; 4] =
    [Strength::AtFloor, Strength::KilledZero, Strength::NotScored, Strength::UnsampledOnly];
const CODES: [Code; 2] = [Code::Correct, Code::Planted];

const KINDS: [Kind; 2] = [Kind::Test, Kind::Proof];

/// The full cross product, `A`'s worlds then `P`'s.
pub fn cells() -> Vec<Cell> {
    KINDS
        .iter()
        .flat_map(|&kind| CODES.iter().map(move |&code| (kind, code)))
        .flat_map(|(kind, code)| APPROVALS.iter().map(move |&approval| (kind, code, approval)))
        .flat_map(|(kind, code, approval)| EVIDENCE.iter().map(move |&evidence| (kind, code, approval, evidence)))
        .flat_map(|(kind, code, approval, evidence)| {
            STRENGTHS.iter().map(move |&strength| Cell { kind, approval, evidence, strength, code })
        })
        .collect()
}

/// The world's truth, from how it was built: planted code is violated
/// whatever proob was told; correct code holds only when the world has an
/// eligible approval, a passing default-branch proof of this very code, and a
/// fresh score that kills. Anything less, a passing test included (it samples
/// inputs), is `Unknown`.
pub fn truth(c: &Cell) -> Truth {
    match (c.code, c.kind, c.approval, c.evidence, c.strength) {
        (Code::Planted, ..) => Truth::Violated,
        (Code::Correct, Kind::Proof, Approval::Team, Evidence::Fresh, Strength::AtFloor) => Truth::Holds,
        (Code::Correct, ..) => Truth::Unknown,
    }
}

impl Cell {
    pub fn label(&self) -> String {
        format!(
            "kind={:?} approval={:?} evidence={:?} strength={:?} code={:?}",
            self.kind, self.approval, self.evidence, self.strength, self.code
        )
    }
}

// ---------------------------------------------------------------------------
// Deliveries: which runs GitHub lists, as a pure function of the cell
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Head,
    Merge,
}

/// One evidence run as GitHub lists it, and which records its artifact holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    pub run_id: u64,
    pub event: &'static str,
    pub branch: &'static str,
    pub at: u64,
    pub point: At,
    pub check: bool,
    pub score: bool,
}

const fn push(at: u64, point: At) -> Delivery {
    Delivery { run_id: 601, event: "push", branch: "main", at, point, check: true, score: false }
}

fn check_run(e: Evidence) -> Option<Delivery> {
    match e {
        Evidence::Fresh => Some(push(NOW - DAY, At::Head)),
        Evidence::EarlierCommit => Some(push(NOW - 9 * DAY, At::Merge)),
        Evidence::Stale => Some(push(NOW - DAY, At::Head)),
        Evidence::PrOnly => Some(Delivery { event: "pull_request", ..push(NOW - DAY, At::Head) }),
        Evidence::OtherBranch => Some(Delivery { branch: "feature", ..push(NOW - DAY, At::Head) }),
        Evidence::Absent => None,
    }
}

/// The clock the world is read at: `Stale` is the fresh world, 15 days on.
pub fn clock(e: Evidence) -> u64 {
    match e {
        Evidence::Stale => NOW + 15 * DAY,
        _ => NOW,
    }
}

/// The score rides the check's run; with no check run, a fresh scheduled run on main brings it alone.
pub fn deliveries(e: Evidence, s: Strength) -> Vec<Delivery> {
    let scored = s != Strength::NotScored;
    let alone = Delivery { run_id: 602, event: "schedule", check: false, score: true, ..push(NOW - DAY, At::Head) };
    match (check_run(e), scored) {
        (Some(run), _) => vec![Delivery { score: scored, ..run }],
        (None, true) => vec![alone],
        (None, false) => vec![],
    }
}

/// The score record's outcome: the real one, or its counts redistributed.
pub fn score_outcome(s: Strength, real: &Value) -> Option<Value> {
    let n = |k: &str| real[k].as_u64().unwrap_or(0);
    let total = n("killed") + n("survived") + n("timeout") + n("unviable") + n("unsampled");
    match s {
        Strength::AtFloor => Some(real.clone()),
        Strength::KilledZero => Some(
            json!({ "result": "scored", "killed": 0, "survived": n("killed") + n("survived"), "timeout": n("timeout"), "unviable": n("unviable") }),
        ),
        Strength::UnsampledOnly => Some(
            json!({ "result": "scored", "killed": 0, "survived": 0, "timeout": 0, "unviable": 0, "unsampled": total }),
        ),
        Strength::NotScored => None,
    }
}

// ---------------------------------------------------------------------------
// The built repository
// ---------------------------------------------------------------------------

/// A commit and what is known at it: proob's hashes for A, and the outcome
/// the real evidence job recorded for A's check there.
struct Point {
    sha: String,
    hashes: Value,
    check: Value,
}

/// P at a commit: proob's hashes for it, and its proof outcome derived from the real check there.
struct Proved {
    hashes: Value,
    outcome: Value,
}

struct Built {
    w: World,
    /// The fixture GitHub after PR #1 merged, with no review.
    base: GitHub,
    merge: String,
    blob: String,
    at_merge: Point,
    correct: Point,
    planted: Point,
    proved_merge: Proved,
    proved_correct: Proved,
    proved_planted: Proved,
    /// The real cargo-mutants outcome at the correct head.
    score: Value,
}

/// A commit bob authored in PR #1 (facts only; no such object in git).
const BOB_COMMIT: &str = "b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0";

fn approve(gh: &mut GitHub, a: Approval, merge: &str) {
    match a {
        Approval::Team => {
            gh.review(1, 101, &BOB, merge);
        }
        Approval::SelfSigned => {
            gh.review(1, 101, &ALICE, merge);
        }
        Approval::Unreviewed => {}
        Approval::AuthorOnly => {
            gh.review(1, 101, &BOB, merge).pr_commit(1, &BOB, BOB_COMMIT, merge, &introduced());
        }
        Approval::Revoked => {
            gh.review(1, 101, &BOB, merge).permission(&BOB, "read");
        }
    }
}

/// The outcome of A's record of `kind` among a run's records.
fn outcome_of(lines: &[Value], kind: &str) -> Fallible<Value> {
    lines
        .iter()
        .find(|l| l["id"] == A && l["kind"] == kind)
        .map(|l| l["outcome"].clone())
        .ok_or_else(|| format!("the evidence run wrote no {kind} record for {A}"))
}

/// main reset to `sha`, the developer's checkout with it.
fn reset_main(w: &World, sha: &str) -> Fallible<()> {
    w.repo.checkout(&["-B", "main", sha])
}

/// A real push run at main's head, at `at`: its check outcome, and the head's hashes.
fn point(w: &World, sha: &str, run_id: u64, at: u64) -> Fallible<Point> {
    reset_main(w, sha)?;
    let (_, lines) = w.evidence_lines(&Event::push(sha, at), run_id)?;
    Ok(Point { sha: sha.to_owned(), hashes: w.hashes(A)?, check: outcome_of(&lines, "check")? })
}

/// A head on top of the merge: `sorted` edited as `code` says, committed on its own branch.
fn head(w: &World, merge: &str, code: Code) -> Fallible<String> {
    let (branch, from, to, message) = match code {
        Code::Correct => ("head-correct", "out.sort_unstable();", "out.sort();", "sorted: a stable sort"),
        Code::Planted => (
            "head-planted",
            "out.sort_unstable();",
            "out.sort_unstable();\n    out.pop();",
            "sorted: drop the largest element",
        ),
    };
    w.repo.checkout(&["-B", branch, merge])?;
    let text = w.repo.read("src/ordering.rs")?;
    w.repo.write("src/ordering.rs", &text.replace(from, to))?;
    w.repo.commit(message, &ALICE, NOW - 8 * DAY)
}

/// P's proof outcome at a commit, derived from the real check there and never
/// chosen: `verified` where the bound test found no counterexample,
/// `proof_failed` where it found one. Any other pairing of head and check is
/// refused, so a proof record is never fabricated against what the code does.
fn proof_outcome(code: Code, check: &Value) -> Fallible<Value> {
    match (code, check["result"].as_str()) {
        (Code::Correct, Some("no_counterexample")) => Ok(json!({ "result": "verified", "rlimit": 10 })),
        (Code::Planted, Some("counterexample")) => Ok(json!({
            "result": "proof_failed",
            "diagnostics": format!("postcondition not satisfied: {}", check["witness"].as_str().unwrap_or(""))
        })),
        _ => Err(format!("the {code:?} head's real check recorded {check}: no proof outcome follows")),
    }
}

/// P at `at`, whose code is `code`, main reset to it, once bindings.json binds P.
fn proved(w: &World, at: &Point, code: Code) -> Fallible<Proved> {
    let outcome = proof_outcome(code, &at.check)?;
    reset_main(w, &at.sha)?;
    Ok(Proved { hashes: w.hashes(P)?, outcome })
}

/// The oracle, independent of proob: the bound test run by plain cargo in a
/// clean checkout passes on correct code and fails on planted code.
fn confirm(w: &World, sha: &str, code: Code) -> Fallible<()> {
    let checkout: Repo = w.checkout_of(&format!("oracle-{code:?}"), sha)?;
    let mut cmd = std::process::Command::new("cargo");
    cmd.current_dir(&checkout.dir)
        .args(["test", "--locked", "--quiet", "--test", "props", "sorted_preserves_length"])
        .env("RUSTUP_TOOLCHAIN", w.ctx.toolchain()?)
        .env("CARGO_TARGET_DIR", w.ctx.rust_target());
    let out = run(&mut cmd)?;
    match (code, out.code) {
        (Code::Correct, 0) => Ok(()),
        (Code::Planted, c) if c != 0 => Ok(()),
        (_, c) => {
            Err(format!("the {code:?} head is not what it says: cargo test exit {c}: {}", first_line(&out.stderr)))
        }
    }
}

fn build(ctx: &Ctx) -> Fallible<Built> {
    let mut w = World::new(ctx)?;
    pre_proob(&w)?;
    propose_proof(&w)?;
    declare(&w)?;
    let (merge, _) = merge_unreviewed(&mut w, &introduced())?;
    let base = w.github.clone();
    // The real jobs run with bob's approval, as a team's would.
    approve(&mut w.github, Approval::Team, &merge);
    let blob = w.repo.blob("main", EVIDENCE_WORKFLOW)?;
    let at_merge = point(&w, &merge, 501, NOW - 9 * DAY)?;
    let (correct_sha, planted_sha) = (head(&w, &merge, Code::Correct)?, head(&w, &merge, Code::Planted)?);
    confirm(&w, &correct_sha, Code::Correct)?;
    confirm(&w, &planted_sha, Code::Planted)?;
    let correct = point(&w, &correct_sha, 502, NOW - 7 * DAY)?;
    let (_, scored) = w.evidence_lines(&Event::schedule(&correct_sha, NOW - 7 * DAY), 503)?;
    let score = outcome_of(&scored, "score")?;
    let planted = point(&w, &planted_sha, 504, NOW - 7 * DAY)?;
    if !matches!(score["killed"].as_u64(), Some(k) if k >= 1) {
        return Err(format!("the real score does not kill at the floor: {score}"));
    }
    // The real jobs are done; from here P is bound through bindings.json.
    bind_proof(&w)?;
    // The merge's `sorted` is the fixture's, before either head's edit.
    let proved_merge = proved(&w, &at_merge, Code::Correct)?;
    let proved_correct = proved(&w, &correct, Code::Correct)?;
    let proved_planted = proved(&w, &planted, Code::Planted)?;
    Ok(Built { w, base, merge, blob, at_merge, correct, planted, proved_merge, proved_correct, proved_planted, score })
}

impl Built {
    fn head(&self, code: Code) -> &Point {
        match code {
            Code::Correct => &self.correct,
            Code::Planted => &self.planted,
        }
    }

    fn point(&self, at: At, code: Code) -> &Point {
        match at {
            At::Head => self.head(code),
            At::Merge => &self.at_merge,
        }
    }

    fn proved(&self, at: At, code: Code) -> &Proved {
        match (at, code) {
            (At::Merge, _) => &self.proved_merge,
            (At::Head, Code::Correct) => &self.proved_correct,
            (At::Head, Code::Planted) => &self.proved_planted,
        }
    }

    /// The run's records for the cell's property, at the commit the run was at.
    fn records(&self, d: &Delivery, cell: &Cell) -> Vec<Value> {
        let p = self.point(d.point, cell.code);
        let (hashes, kind, outcome) = match cell.kind {
            Kind::Test => (&p.hashes, "check", &p.check),
            Kind::Proof => {
                let proved = self.proved(d.point, cell.code);
                (&proved.hashes, "proof", &proved.outcome)
            }
        };
        let line = |kind: &str, outcome: Value| record(hashes, kind, outcome, d.run_id, &p.sha, &self.blob, d.at);
        let check = d.check.then(|| line(kind, outcome.clone()));
        let score = score_outcome(cell.strength, &self.score).filter(|_| d.score).map(|o| line("score", o));
        [check, score].into_iter().flatten().collect()
    }

    fn github_for(&self, cell: &Cell) -> GitHub {
        let mut gh = self.base.clone();
        approve(&mut gh, cell.approval, &self.merge);
        deliveries(cell.evidence, cell.strength).iter().for_each(|d| {
            let sha = &self.point(d.point, cell.code).sha;
            gh.evidence_run(d.run_id, sha, d.event, d.branch, d.at, self.records(d, cell));
        });
        gh
    }

    /// proob's word for the cell's property in its world, with no fact cached from another world.
    fn rendered(&mut self, cell: &Cell) -> Fallible<String> {
        self.w.github = self.github_for(cell);
        self.w.now = clock(cell.evidence);
        let facts = self.w.repo.dir.join("proob/.cache/facts");
        match fs::remove_dir_all(&facts) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(format!("{}: {e}", facts.display())),
            _ => {}
        }
        Ok(observe(&self.w.status()?, cell.kind.id())?.word)
    }
}

/// Builds the repository, renders every world, logs each; the number of worlds.
pub fn run_scenario(ctx: &Ctx, log: &Log) -> Fallible<usize> {
    let mut b = build(ctx)?;
    let all = cells();
    let mut tally: Vec<(String, Truth, usize)> = Vec::new();
    for code in CODES {
        reset_main(&b.w, &b.head(code).sha.clone())?;
        for cell in all.iter().filter(|c| c.code == code) {
            let word = b.rendered(cell)?;
            log.record("soundness", &cell.label(), None, &word, truth(cell))?;
            count(&mut tally, word, truth(cell));
        }
    }
    tally.iter().for_each(|(word, t, n)| println!("  {n:>4}  {word:<40} truth {}", t.name()));
    Ok(all.len())
}

fn count(tally: &mut Vec<(String, Truth, usize)>, word: String, t: Truth) {
    match tally.iter_mut().find(|(w, tt, _)| *w == word && *tt == t) {
        Some(row) => row.2 += 1,
        None => tally.push((word, t, 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matrix_is_the_full_cross_product_without_repeats() {
        let all = cells();
        let proved = all.iter().filter(|c| c.kind == Kind::Proof).count();
        assert_eq!((all.len(), proved), (2 * 5 * 6 * 4 * 2, 5 * 6 * 4 * 2));
        assert!(all.iter().enumerate().all(|(i, c)| !all[..i].contains(c)));
    }

    #[test]
    fn only_the_proved_world_on_correct_code_holds_and_planted_code_is_always_violated() {
        let holds: Vec<Cell> = cells().into_iter().filter(|c| truth(c) == Truth::Holds).collect();
        let world = Cell {
            kind: Kind::Proof,
            approval: Approval::Team,
            evidence: Evidence::Fresh,
            strength: Strength::AtFloor,
            code: Code::Correct,
        };
        assert_eq!(holds, [world], "Holds stays reachable, and only through a proof");
        assert!(cells().contains(&Cell { kind: Kind::Test, ..world }));
        assert_eq!(truth(&Cell { kind: Kind::Test, ..world }), Truth::Unknown, "a test samples its inputs");
        assert!(cells().iter().filter(|c| c.code == Code::Planted).all(|c| truth(c) == Truth::Violated));
    }

    #[test]
    fn a_proof_outcome_follows_the_real_check_and_nothing_else() {
        let pass = json!({ "result": "no_counterexample", "budget": 256 });
        let fail = json!({ "result": "counterexample", "witness": "v = [0]" });
        assert_eq!(proof_outcome(Code::Correct, &pass), Ok(json!({ "result": "verified", "rlimit": 10 })));
        assert_eq!(proof_outcome(Code::Planted, &fail).expect("derives")["result"], "proof_failed");
        assert!(proof_outcome(Code::Planted, &pass).is_err(), "planted code never gets a passing proof");
        assert!(proof_outcome(Code::Correct, &fail).is_err());
        assert!(proof_outcome(Code::Correct, &json!({ "result": "errored", "reason": "x" })).is_err());
    }

    #[test]
    fn the_score_rides_the_check_run_or_comes_alone() {
        assert_eq!(deliveries(Evidence::Absent, Strength::NotScored), []);
        let alone = deliveries(Evidence::Absent, Strength::AtFloor);
        assert_eq!((alone.len(), alone[0].check, alone[0].score, alone[0].event), (1, false, true, "schedule"));
        let pr = deliveries(Evidence::PrOnly, Strength::KilledZero);
        assert_eq!((pr.len(), pr[0].check, pr[0].score, pr[0].event), (1, true, true, "pull_request"));
        assert_eq!(deliveries(Evidence::EarlierCommit, Strength::NotScored)[0].point, At::Merge);
        assert_eq!(deliveries(Evidence::Stale, Strength::AtFloor), deliveries(Evidence::Fresh, Strength::AtFloor));
        assert!(clock(Evidence::Stale) - deliveries(Evidence::Stale, Strength::AtFloor)[0].at > 14 * DAY);
    }

    #[test]
    fn score_variants_keep_the_mutant_count_and_kill_nothing() {
        let real = json!({ "result": "scored", "killed": 3, "survived": 1, "timeout": 0, "unviable": 0 });
        assert_eq!(score_outcome(Strength::AtFloor, &real), Some(real.clone()));
        let zero = score_outcome(Strength::KilledZero, &real).expect("a record");
        assert_eq!((zero["killed"].as_u64(), zero["survived"].as_u64()), (Some(0), Some(4)));
        let unsampled = score_outcome(Strength::UnsampledOnly, &real).expect("a record");
        assert_eq!((unsampled["killed"].as_u64(), unsampled["unsampled"].as_u64()), (Some(0), Some(4)));
        assert_eq!(score_outcome(Strength::NotScored, &real), None);
    }
}
