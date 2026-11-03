//! `proob shadow report [--json]`: what the shadow tried, and how to make it real.
//!
//! The rows come from the shadow's `work/`: its property files, the adapters'
//! discovery there, and the latest local runs in
//! `proob/.cache/results/results.jsonl` whose content and binding hashes
//! match the files as they are now (a run of an older version of the
//! property or its test does not count). Nothing is asked of GitHub: in
//! shadow every property reads `Proposed`, because no one has approved it.
//!
//! The JSON (`--json`, and `report.json` beside the shadow) is stable,
//! `schema_version` 1:
//!
//! ```text
//! { "schema_version": 1, "repository": "<abs path>", "work": "<abs path>",
//!   "base": "<sha>", "headline": "<the first line>",
//!   "verdict": { "word": "Proposed", "why": "no one has approved it: shadow results are local" },
//!   "counts": { "tried", "fails", "passes", "not_run", "errored" },
//!   "properties": [ { "id", "title", "statement", "verdict": "Proposed",
//!       "result": { "state": "no_test" | "not_run" | "passes" (cases, seed)
//!                   | "checks" | "fails" (input, seed?) | "errored" (reason) },
//!       "strength": { "state": "not_scored" | "can_fail" (killed, mutants, unsampled?)
//!                   | "cannot_fail" (mutants) | "none_killed_in_sample" (mutants, unsampled)
//!                   | "nothing_to_mutate" | "errored" (reason) },
//!       "line": "<the terminal line>" } ],
//!   "make_it_real": [ "<step line>", ... ], "adopted_branch": "<branch>" | null }
//! ```
//!
//! `unsampled` is present only on a sampled score: that many mutants in scope
//! were listed and not run (`max_mutants`), and `killed` of `mutants` is the
//! sample's. A sample that kills nothing is `none_killed_in_sample`, never
//! `cannot_fail`: the mutants not run may be the ones the test reads.
use crate::cli::adapters::{Discovered, discover};
use crate::cli::backend::with_offline_backend;
use crate::cli::error::CliError;
use crate::cli::hash::hashes;
use crate::cli::workspace::{LoadedProperty, Workspace};
use proob_effects::github::RepoId;
use proob_facts::config::Config;
use proob_facts::evidence::{EvidenceKind, OutcomeRecord, Replay};
use proob_facts::paths::RESULTS_DIR;
use proob_facts::results::{RESULTS_FILE, RunResultRecord, parse_results};
use serde::Serialize;
use std::path::Path;

pub const WHY: &str = "no one has approved it: shadow results are local";

/// The primary check's latest local result for the files as they are now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Outcome {
    /// No test carries the property's token yet.
    NoTest,
    /// A test is bound but has not run since the property or its test last changed.
    NotRun,
    Passes {
        cases: u64,
        seed: String,
    },
    /// A proof or a model checked locally; a Kani proof only within its harness's bounds.
    Checks {
        within_bounds: bool,
    },
    Fails {
        input: String,
        seed: Option<String>,
    },
    Errored {
        reason: String,
    },
}

/// Whether the bound test can fail: its latest local mutation score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Strength {
    NotScored,
    CanFail {
        killed: u32,
        mutants: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        unsampled: Option<u32>,
    },
    /// Every mutant in scope ran and none was killed.
    CannotFail {
        mutants: u32,
    },
    /// A sample ran and killed none; `unsampled` more were not run.
    NoneKilledInSample {
        mutants: u32,
        unsampled: u32,
    },
    NothingToMutate,
    Errored {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub statement: String,
    pub outcome: Outcome,
    pub strength: Strength,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub tried: usize,
    pub fails: usize,
    pub passes: usize,
    pub not_run: usize,
    pub errored: usize,
}

/// Where the user's repository goes next: what the "Make it real" steps name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Next {
    pub branch: String,
    pub default_branch: String,
    pub origin: Option<String>,
    pub gh: bool,
    pub adopted: Option<String>,
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// Every property in `work`, with its latest current local results.
pub fn rows(work: &Path) -> Result<Vec<Row>, CliError> {
    let ws = Workspace::open(Some(work))?;
    let properties = ws.load_properties()?;
    let config = ws.load_config()?.candidate();
    let discovered = discover(&ws, &config)?;
    let records = records(work);
    properties.iter().map(|p| row(&ws, &config, discovered.as_ref(), &records, p)).collect()
}

/// The local results, oldest first; a line that does not parse is skipped.
fn records(work: &Path) -> Vec<RunResultRecord> {
    let text = std::fs::read_to_string(work.join(RESULTS_DIR).join(RESULTS_FILE)).unwrap_or_default();
    parse_results(&text).filter_map(|(_, r)| r.ok()).collect()
}

fn row(
    ws: &Workspace,
    config: &Config,
    d: Option<&Discovered>,
    records: &[RunResultRecord],
    p: &LoadedProperty,
) -> Result<Row, CliError> {
    let (outcome, strength) = match d.filter(|d| d.binding(p.id).is_some()) {
        None => (Outcome::NoTest, Strength::NotScored),
        Some(d) => {
            let bindings = d.bindings();
            let h = with_offline_backend(ws, Some(&bindings), |b| hashes(ws, b, p, config.waiver_max_days))?;
            let current: Vec<&RunResultRecord> = records
                .iter()
                .filter(|r| {
                    r.id == p.file.id
                        && r.content_hash == h.content_hash
                        && Some(&r.binding_hash) == h.binding_hash.as_ref()
                })
                .collect();
            let primary = primary(&current);
            let score = current.iter().rfind(|r| r.kind == EvidenceKind::Score);
            (
                primary.map_or(Outcome::NotRun, |r| outcome_of(&r.outcome, r.replay.as_ref())),
                score.map_or(Strength::NotScored, |r| strength_of(&r.outcome)),
            )
        }
    };
    Ok(Row {
        id: p.file.id.clone(),
        title: p.file.title.clone(),
        statement: p.file.statement.clone(),
        outcome,
        strength,
    })
}

/// The check run a row reports: the newest failure on this content and binding, else the newest
/// run. A counterexample stays true of that code however many other seeds pass after it.
fn primary<'a>(current: &[&'a RunResultRecord]) -> Option<&'a RunResultRecord> {
    let checks = || current.iter().copied().filter(|r| r.kind != EvidenceKind::Score);
    checks().rfind(|r| matches!(outcome_of(&r.outcome, None), Outcome::Fails { .. })).or_else(|| checks().next_back())
}

fn seed(replay: Option<&Replay>) -> Option<String> {
    replay.map(|r| r.seed.clone())
}

pub fn outcome_of(o: &OutcomeRecord, replay: Option<&Replay>) -> Outcome {
    match o {
        OutcomeRecord::NoCounterexample { budget } => {
            Outcome::Passes { cases: *budget, seed: seed(replay).unwrap_or_default() }
        }
        OutcomeRecord::Counterexample { witness } => Outcome::Fails { input: one_line(witness), seed: seed(replay) },
        OutcomeRecord::ProofFailed { diagnostics } => Outcome::Fails { input: one_line(diagnostics), seed: None },
        OutcomeRecord::Violation { trace } => Outcome::Fails { input: one_line(trace), seed: None },
        OutcomeRecord::Verified { checks, .. } => Outcome::Checks { within_bounds: checks.is_some() },
        OutcomeRecord::NoViolation => Outcome::Checks { within_bounds: false },
        OutcomeRecord::Errored { reason } => Outcome::Errored { reason: one_line(reason) },
        OutcomeRecord::Scored { .. } | OutcomeRecord::NoMutants => Outcome::NotRun,
    }
}

pub fn strength_of(o: &OutcomeRecord) -> Strength {
    match o {
        OutcomeRecord::Scored { killed, survived, timeout, unsampled, .. } => {
            let unsampled = unsampled.filter(|n| *n > 0);
            match (killed, killed + survived + timeout, unsampled) {
                (_, 0, _) => Strength::NothingToMutate,
                (0, mutants, None) => Strength::CannotFail { mutants },
                (0, mutants, Some(unsampled)) => Strength::NoneKilledInSample { mutants, unsampled },
                (k, mutants, unsampled) => Strength::CanFail { killed: *k, mutants, unsampled },
            }
        }
        OutcomeRecord::NoMutants => Strength::NothingToMutate,
        OutcomeRecord::Errored { reason } => Strength::Errored { reason: one_line(reason) },
        _ => Strength::NotScored,
    }
}

/// Whitespace collapsed, at most 120 characters.
fn one_line(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(120) {
        Some((cut, _)) => format!("{}…", &flat[..cut]),
        None => flat,
    }
}

// ---------------------------------------------------------------------------
// Words
// ---------------------------------------------------------------------------

pub fn counts(rows: &[Row]) -> Counts {
    rows.iter().fold(Counts { tried: rows.len(), ..Counts::default() }, |c, r| match r.outcome {
        Outcome::Fails { .. } => Counts { fails: c.fails + 1, ..c },
        Outcome::Passes { .. } | Outcome::Checks { .. } => Counts { passes: c.passes + 1, ..c },
        Outcome::NoTest | Outcome::NotRun => Counts { not_run: c.not_run + 1, ..c },
        Outcome::Errored { .. } => Counts { errored: c.errored + 1, ..c },
    })
}

/// "3 properties tried", then each non-zero bucket.
pub fn tally(c: &Counts) -> Vec<String> {
    let verb = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let buckets = [
        (c.fails, verb(c.fails, "fails", "fail")),
        (c.passes, verb(c.passes, "passes", "pass")),
        (c.not_run, format!("{} not run yet", c.not_run)),
        (c.errored, format!("{} could not run", c.errored)),
    ];
    std::iter::once(verb(c.tried, "property tried", "properties tried"))
        .chain(buckets.into_iter().filter(|(n, _)| *n > 0).map(|(_, s)| s))
        .collect()
}

fn untouched(adopted: Option<&str>) -> String {
    match adopted {
        None => "nothing in your repository was changed".to_string(),
        Some(b) => format!("adopt made branch {b}; nothing else in your repository was changed"),
    }
}

pub fn headline(c: &Counts, adopted: Option<&str>) -> String {
    let what = match c.tried {
        0 => "no properties tried yet · /proob:try proposes some".to_string(),
        _ => tally(c).join(" · "),
    };
    format!("shadow: {what} · {}", untouched(adopted))
}

fn result_text(r: &Row) -> String {
    let id = &r.id;
    match &r.outcome {
        Outcome::NoTest => "not run yet: /proob:falsify".to_string(),
        Outcome::NotRun => format!("not run yet: proob --shadow run check {id}"),
        Outcome::Passes { cases, seed } => {
            format!("passes {cases} cases (seed {seed}) · {}", strength_text(id, &r.strength))
        }
        Outcome::Checks { within_bounds: false } => {
            format!("the proof checks locally · {}", strength_text(id, &r.strength))
        }
        Outcome::Checks { within_bounds: true } => {
            format!("verified locally within the harness's bounds · {}", strength_text(id, &r.strength))
        }
        Outcome::Fails { input, seed: Some(s) } => {
            format!("fails: input {input} · replay: proob --shadow run check {id} --seed {s}")
        }
        Outcome::Fails { input, seed: None } => format!("fails: {input} · rerun: proob --shadow run check {id}"),
        Outcome::Errored { reason } => format!("could not run: {reason} · retry: proob --shadow run check {id}"),
    }
}

fn strength_text(id: &str, s: &Strength) -> String {
    match s {
        Strength::NotScored => format!("not scored yet: proob --shadow run score {id}"),
        Strength::CanFail { killed, mutants, unsampled } => {
            format!("test can fail: killed {killed} of {mutants} {}", sampled(*mutants, *unsampled))
        }
        Strength::CannotFail { mutants } => {
            format!("test cannot fail: killed 0 of {mutants} mutants; make it look at the code")
        }
        Strength::NoneKilledInSample { mutants, unsampled } => format!(
            "not shown able to fail: killed 0 of {mutants} {}; the sample may miss the code the test reads: \
             raise max_mutants in proob/config.toml and rescore: proob --shadow run score {id}",
            sampled(*mutants, Some(*unsampled))
        ),
        Strength::NothingToMutate => "nothing in its scope to mutate".to_string(),
        Strength::Errored { reason } => format!("score could not run: {reason}"),
    }
}

/// "mutants", or for a sampled score what the sample left out.
fn sampled(mutants: u32, unsampled: Option<u32>) -> String {
    match unsampled {
        None => "mutants".to_string(),
        Some(n) => format!("sampled mutant{} ({n} more in scope not run)", if mutants == 1 { "" } else { "s" }),
    }
}

/// One line per property: its statement, then its local result.
pub fn line(r: &Row) -> String {
    format!("  - {} — {}", r.statement.trim().trim_end_matches('.'), result_text(r))
}

/// The "Make it real" steps for this repository, numbered.
pub fn steps(c: &Counts, n: &Next) -> Vec<String> {
    let first = match (c.tried, c.fails) {
        (0, _) => Some("0. run /proob:try first: there is nothing to adopt yet".to_string()),
        (_, 0) => None,
        (_, f) => Some(format!(
            "0. fix the code or drop {} before adopting",
            if f == 1 { "the failing property" } else { "the failing properties" }
        )),
    };
    let adopt = match &n.adopted {
        Some(b) => format!("1. done: proob shadow adopt made branch {b} in your repository"),
        None => "1. proob shadow adopt — puts the declaration, the tests, and the two workflows on a new branch of your repository".to_string(),
    };
    let branch = n.adopted.as_deref().unwrap_or(&n.branch);
    [first, Some(adopt), Some(format!("2. {} — {}", push(n, branch), REVIEW)), Some(format!("3. {}", require(n)))]
        .into_iter()
        .flatten()
        .collect()
}

const REVIEW: &str = "a teammate's review is the approval (a merge you review yourself counts as self-signed)";

/// The commands that put `branch` up for review.
pub fn push(n: &Next, branch: &str) -> String {
    match (&n.origin, n.gh) {
        (None, _) => format!(
            "add your GitHub remote (git remote add origin <url>), then git push -u origin {branch} and open a pull request"
        ),
        (Some(_), true) => {
            format!("git push -u origin {branch} && gh pr create --head {branch} --base {} --fill", n.default_branch)
        }
        (Some(_), false) => format!("git push -u origin {branch}, then open a pull request into {}", n.default_branch),
    }
}

fn require(n: &Next) -> String {
    let base = format!("make the `proob gate` check required on {}", n.default_branch);
    match n.origin.as_deref().filter(|u| u.contains("github")).and_then(RepoId::from_remote_url) {
        Some(id) => format!(
            "{base}: https://github.com/{}/{}/settings/branches → Require status checks → proob gate",
            id.owner, id.name
        ),
        None => format!("{base} in your host's branch rules"),
    }
}

/// The terminal text, which `report.md` also holds.
pub fn text(rows: &[Row], n: &Next) -> String {
    let c = counts(rows);
    let body: String = rows.iter().map(|r| format!("{}\n", line(r))).collect();
    let steps: String = steps(&c, n).iter().map(|s| format!("  {s}\n")).collect();
    let verdict = if rows.is_empty() { String::new() } else { format!("every property is Proposed ({WHY})\n\n{body}") };
    format!("{}\n{verdict}\nMake it real:\n{steps}", headline(&c, n.adopted.as_deref()))
}

#[derive(Serialize)]
struct VerdictJ {
    word: &'static str,
    why: &'static str,
}

#[derive(Serialize)]
struct RowJ<'a> {
    id: &'a str,
    title: &'a str,
    statement: &'a str,
    verdict: &'static str,
    result: &'a Outcome,
    strength: &'a Strength,
    line: String,
}

#[derive(Serialize)]
pub struct ReportJ<'a> {
    schema_version: u32,
    repository: String,
    work: String,
    base: &'a str,
    headline: String,
    verdict: VerdictJ,
    counts: Counts,
    properties: Vec<RowJ<'a>>,
    make_it_real: Vec<String>,
    adopted_branch: Option<&'a str>,
}

pub fn json<'a>(rows: &'a [Row], n: &'a Next, repo: &Path, work: &Path, base: &'a str) -> ReportJ<'a> {
    let c = counts(rows);
    ReportJ {
        schema_version: 1,
        repository: repo.display().to_string(),
        work: work.display().to_string(),
        base,
        headline: headline(&c, n.adopted.as_deref()),
        verdict: VerdictJ { word: "Proposed", why: WHY },
        counts: c,
        properties: rows
            .iter()
            .map(|r| RowJ {
                id: &r.id,
                title: &r.title,
                statement: &r.statement,
                verdict: "Proposed",
                result: &r.outcome,
                strength: &r.strength,
                line: line(r),
            })
            .collect(),
        make_it_real: steps(&c, n),
        adopted_branch: n.adopted.as_deref(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(outcome: &str) -> RunResultRecord {
        let json = format!(
            r#"{{"schema_version":1,"id":"01K8V5NQ4A0F3R7Y2M9D6P1XPB","kind":"check","algo":"sha256","content_hash":"00","binding_hash":"00","scope_hash":"00","outcome":{outcome}}}"#
        );
        proob_facts::results::parse_result(&json).expect("a valid record")
    }

    #[test]
    fn a_later_passing_seed_does_not_hide_a_counterexample() {
        let fail = record(r#"{"result":"counterexample","witness":"[6]"}"#);
        let pass = record(r#"{"result":"no_counterexample","budget":256}"#);
        let err = record(r#"{"result":"errored","reason":"timeout"}"#);
        assert_eq!(primary(&[&fail, &pass]), Some(&fail));
        assert_eq!(primary(&[&fail, &err, &pass]), Some(&fail));
        assert_eq!(primary(&[&pass, &err]), Some(&err), "with no failure, the newest run");
        assert_eq!(primary(&[]), None);
    }

    #[test]
    fn a_kani_proof_is_reported_within_its_harness_bounds_and_a_verus_one_is_not() {
        let text = |o: &str| result_text(&row(outcome_of(&record(o).outcome, None), Strength::NotScored));
        assert!(
            text(r#"{"result":"verified","checks":13}"#).starts_with("verified locally within the harness's bounds")
        );
        assert!(text(r#"{"result":"verified","rlimit":10}"#).starts_with("the proof checks locally"));
    }

    fn row(outcome: Outcome, strength: Strength) -> Row {
        Row {
            id: "01K8V5NQ4A0F3R7Y2M9D6P1XPB".into(),
            title: "t".into(),
            statement: "The balance is never below zero.".into(),
            outcome,
            strength,
        }
    }

    fn next(origin: Option<&str>, gh: bool) -> Next {
        Next {
            branch: "proob/adopt-2026-10-29".into(),
            default_branch: "main".into(),
            origin: origin.map(str::to_string),
            gh,
            adopted: None,
        }
    }

    #[test]
    fn a_failing_row_names_its_input_and_the_replay_command() {
        let r = row(Outcome::Fails { input: "[6]".into(), seed: Some("42".into()) }, Strength::NotScored);
        assert_eq!(
            line(&r),
            "  - The balance is never below zero — fails: input [6] · replay: proob --shadow run check 01K8V5NQ4A0F3R7Y2M9D6P1XPB --seed 42"
        );
    }

    #[test]
    fn the_headline_counts_only_what_happened() {
        let rows = [
            row(Outcome::Fails { input: "x".into(), seed: None }, Strength::NotScored),
            row(
                Outcome::Passes { cases: 256, seed: "1".into() },
                Strength::CanFail { killed: 9, mutants: 12, unsampled: None },
            ),
            row(Outcome::NoTest, Strength::NotScored),
        ];
        assert_eq!(
            headline(&counts(&rows), None),
            "shadow: 3 properties tried · 1 fails · 1 passes · 1 not run yet · nothing in your repository was changed"
        );
        assert_eq!(
            headline(&counts(&[]), None),
            "shadow: no properties tried yet · /proob:try proposes some · nothing in your repository was changed"
        );
    }

    #[test]
    fn make_it_real_starts_with_a_fix_when_something_fails_and_names_this_repository() {
        let c = Counts { tried: 2, fails: 1, passes: 1, ..Counts::default() };
        let s = steps(&c, &next(Some("git@github.com:acme/pay.git"), true));
        assert_eq!(s.len(), 4);
        assert!(s[0].starts_with("0. fix the code or drop the failing property"), "{}", s[0]);
        assert!(s[2].contains(
            "git push -u origin proob/adopt-2026-10-29 && gh pr create --head proob/adopt-2026-10-29 --base main --fill"
        ));
        assert!(s[3].contains("https://github.com/acme/pay/settings/branches"), "{}", s[3]);
        let clean = steps(&Counts { tried: 1, passes: 1, ..Counts::default() }, &next(None, false));
        assert_eq!(clean.len(), 3);
        assert!(clean[1].contains("git remote add origin"), "{}", clean[1]);
    }

    #[test]
    fn scores_read_as_whether_the_test_can_fail() {
        let scored =
            |killed, survived| OutcomeRecord::Scored { killed, survived, timeout: 0, unviable: 3, unsampled: None };
        assert_eq!(strength_of(&scored(9, 3)), Strength::CanFail { killed: 9, mutants: 12, unsampled: None });
        assert_eq!(strength_of(&scored(0, 4)), Strength::CannotFail { mutants: 4 });
        assert_eq!(strength_of(&scored(0, 0)), Strength::NothingToMutate);
        let sample = OutcomeRecord::Scored { killed: 9, survived: 18, timeout: 2, unviable: 1, unsampled: Some(382) };
        let s = strength_of(&sample);
        assert_eq!(s, Strength::CanFail { killed: 9, mutants: 29, unsampled: Some(382) });
        assert_eq!(strength_text("X", &s), "test can fail: killed 9 of 29 sampled mutants (382 more in scope not run)");
    }

    #[test]
    fn a_sample_that_kills_nothing_is_not_cannot_fail() {
        // vaultwarden: 17 of a file's 160 mutants sampled, likely none in the two functions the test calls.
        let sampled =
            |unsampled| OutcomeRecord::Scored { killed: 0, survived: 17, timeout: 0, unviable: 13, unsampled };
        let s = strength_of(&sampled(Some(143)));
        assert_eq!(s, Strength::NoneKilledInSample { mutants: 17, unsampled: 143 });
        let text = strength_text("X", &s);
        assert!(!text.contains("cannot fail"), "{text}");
        assert!(text.starts_with("not shown able to fail: killed 0 of 17 sampled mutants (143 more in scope not run)"));
        assert!(text.contains("raise max_mutants") && text.ends_with("proob --shadow run score X"), "{text}");
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json, serde_json::json!({ "state": "none_killed_in_sample", "mutants": 17, "unsampled": 143 }));
        // a sample of everything is the whole scope
        assert_eq!(strength_of(&sampled(Some(0))), Strength::CannotFail { mutants: 17 });
    }

    #[test]
    fn a_passing_row_without_a_score_says_so() {
        let r = row(Outcome::Passes { cases: 256, seed: "1".into() }, Strength::NotScored);
        assert!(
            line(&r).ends_with("not scored yet: proob --shadow run score 01K8V5NQ4A0F3R7Y2M9D6P1XPB"),
            "{}",
            line(&r)
        );
    }
}
