//! Expectations as data, what proob rendered, and the comparison of the two.
//! Every comparison is one line of `target/sim/verdicts.jsonl`
//! (`{"scenario","step","expected","rendered","truth"}`), the input
//! `cargo xtask bench soundness` scores with the SV-COMP scheme of
//! bench/README.md: `rendered` against `truth`, never against `expected`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::world::{Fallible, Run};

/// Whether the report as a whole must say it was derived from verified facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Verified,
    Unverified,
}

/// Whether the property actually holds in the world a step builds, known from
/// how the world was built and never from proob: `Violated` when the code
/// under the verdict carries a real counterexample (a planted bug its bound
/// test catches), `Holds` when the code is correct and the world holds what
/// the claim rests on (an eligible approval of what is checked, a passing
/// default-branch check of this code by a proof or a model, never a test,
/// which samples its inputs, and a fresh score that kills), `Unknown` when the
/// code is correct but the world lacks one of those.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    Holds,
    Violated,
    Unknown,
}

impl Truth {
    pub const fn name(self) -> &'static str {
        match self {
            Truth::Holds => "holds",
            Truth::Violated => "violated",
            Truth::Unknown => "unknown",
        }
    }

    pub fn parse(name: &str) -> Option<Truth> {
        [Truth::Holds, Truth::Violated, Truth::Unknown].into_iter().find(|t| t.name() == name)
    }
}

/// What one step must render for the property it is about.
#[derive(Debug, Clone, Copy)]
pub struct Expect {
    pub step: &'static str,
    /// The verdict word as the plan names it ("Checked (strength pending)",
    /// "Re-approve (check amended)").
    pub word: &'static str,
    /// The exit code of the command that rendered it.
    pub exit: i32,
    pub view: View,
    /// When set, the reason column must say this too (a `Broken` for a runner
    /// error is not the counterexample the step is about).
    pub reason: Option<&'static str>,
    /// The ground truth of the step's world, recorded beside the word.
    pub truth: Truth,
}

impl Expect {
    pub const fn status(step: &'static str, word: &'static str, truth: Truth) -> Expect {
        Expect { step, word, exit: 0, view: View::Verified, reason: None, truth }
    }

    pub const fn unverified(step: &'static str, word: &'static str, truth: Truth) -> Expect {
        Expect { step, word, exit: 0, view: View::Unverified, reason: None, truth }
    }

    pub const fn blocks(step: &'static str, word: &'static str, truth: Truth) -> Expect {
        Expect { step, word, exit: 1, view: View::Verified, reason: None, truth }
    }

    pub const fn because(self, reason: &'static str) -> Expect {
        Expect { reason: Some(reason), ..self }
    }
}

/// What a report said about one property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub word: String,
    pub exit: i32,
    pub view: View,
    /// Some row of the report, any property, rendered `Holds`.
    pub holds_anywhere: bool,
    /// The reason column, printed beside the word (not compared).
    pub reason: String,
}

/// The rendered word of the row for `id` in a `--json` report: the status
/// column, and for `Re-approve` its reason too, as the headline prints it.
pub fn observe(run: &Run, id: &str) -> Fallible<Observed> {
    let report = run.json()?;
    let rows = report["properties"].as_array().ok_or("the report has no properties")?;
    let row = rows.iter().find(|r| r["id"] == id).ok_or_else(|| format!("no row for {id}"))?;
    let view = match report["verification"]["state"].as_str() {
        Some("verified") => View::Verified,
        _ => View::Unverified,
    };
    let reason = row["reason"].as_str().unwrap_or("").to_owned();
    Ok(Observed { word: rendered(row), exit: run.code, view, holds_anywhere: rows.iter().any(is_holds), reason })
}

fn rendered(row: &Value) -> String {
    match (row["word"].as_str(), row["reason"].as_str()) {
        (Some("Re-approve"), Some(reason)) => format!("Re-approve ({reason})"),
        (Some(word), _) => word.to_owned(),
        (None, _) => "<no word>".to_owned(),
    }
}

fn is_holds(row: &Value) -> bool {
    row["word"] == "Holds" || row["verdict"]["word"] == "holds"
}

/// Every way an observation can disagree with its expectation; empty is a pass.
/// `Holds` must be absent from every report that does not expect it.
pub fn disagreements(e: &Expect, o: &Observed) -> Vec<String> {
    [
        (o.word != e.word).then(|| format!("rendered '{}', expected '{}'", o.word, e.word)),
        (o.exit != e.exit).then(|| format!("exit {}, expected {}", o.exit, e.exit)),
        (o.view != e.view).then(|| format!("view {:?}, expected {:?}", o.view, e.view)),
        e.reason.filter(|r| *r != o.reason).map(|r| format!("reason '{}', expected '{r}'", o.reason)),
        (e.word != "Holds" && o.holds_anywhere).then(|| "Holds rendered where it must not be".to_owned()),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The verdict log, truncated at the start of a run.
pub struct Log {
    path: PathBuf,
}

impl Log {
    pub fn create(path: &Path) -> Fallible<Log> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(path, "").map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Log { path: path.to_path_buf() })
    }

    /// One line; `expected` is `None` for a world that asserts nothing in advance.
    pub fn record(
        &self,
        scenario: &str,
        step: &str,
        expected: Option<&str>,
        rendered: &str,
        truth: Truth,
    ) -> Fallible<()> {
        let line = json!({ "scenario": scenario, "step": step, "expected": expected, "rendered": rendered, "truth": truth.name() });
        let mut file =
            OpenOptions::new().append(true).open(&self.path).map_err(|e| format!("{}: {e}", self.path.display()))?;
        writeln!(file, "{line}").map_err(|e| format!("{}: {e}", self.path.display()))
    }
}

/// Runs `(expectation, step)` pairs in order, logging each comparison. A step
/// that cannot run (`Err`) stops the sequence, since later steps build on it;
/// a disagreement does not, so one run shows every verdict that is wrong.
pub type Step<S> = fn(&mut S) -> Fallible<Observed>;

pub fn run_steps<S>(scenario: &str, log: &Log, state: &mut S, steps: &[(Expect, Step<S>)]) -> Fallible<Vec<String>> {
    let mut wrong = Vec::new();
    for (expect, step) in steps {
        let observed = step(state).map_err(|e| format!("{}: {e}", expect.step))?;
        log.record(scenario, expect.step, Some(expect.word), &observed.word, expect.truth)?;
        let found = disagreements(expect, &observed);
        let mark = if found.is_empty() { "ok" } else { "WRONG" };
        println!("  {mark:<5} {:<58} {} ({}; exit {})", expect.step, observed.word, observed.reason, observed.exit);
        wrong.extend(found.into_iter().map(|d| format!("{}: {d}", expect.step)));
    }
    Ok(wrong)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(word: &str, holds_anywhere: bool) -> Observed {
        Observed { word: word.to_owned(), exit: 0, view: View::Verified, holds_anywhere, reason: String::new() }
    }

    #[test]
    fn a_matching_observation_has_no_disagreement() {
        assert!(disagreements(&Expect::status("s", "Holds", Truth::Holds), &seen("Holds", true)).is_empty());
        assert!(
            disagreements(&Expect::status("s", "Not checked", Truth::Unknown), &seen("Not checked", false)).is_empty()
        );
    }

    #[test]
    fn holds_anywhere_fails_a_step_that_does_not_expect_it() {
        let found = disagreements(&Expect::status("s", "Not checked", Truth::Unknown), &seen("Not checked", true));
        assert_eq!(found, ["Holds rendered where it must not be"]);
    }

    #[test]
    fn word_exit_and_view_are_each_compared() {
        let o = Observed {
            word: "Broken".into(),
            exit: 0,
            view: View::Unverified,
            holds_anywhere: false,
            reason: String::new(),
        };
        assert_eq!(disagreements(&Expect::blocks("s", "Broken", Truth::Violated), &o).len(), 2);
        assert_eq!(disagreements(&Expect::unverified("s", "Broken", Truth::Violated), &o).len(), 0);
        assert_eq!(
            disagreements(&Expect::unverified("s", "Broken", Truth::Violated).because("counterexample on this PR"), &o)
                .len(),
            1
        );
    }

    #[test]
    fn re_approve_carries_its_reason() {
        let row = json!({ "word": "Re-approve", "reason": "check amended" });
        assert_eq!(rendered(&row), "Re-approve (check amended)");
        assert_eq!(rendered(&json!({ "word": "Holds", "reason": "approved" })), "Holds");
    }

    #[test]
    fn observe_reads_the_row_and_the_view() {
        let stdout = json!({
            "verification": { "state": "unverified" },
            "properties": [{ "id": "A", "word": "Not checked", "reason": "GitHub unreachable", "verdict": { "word": "not_checked" } }]
        });
        let run = Run { code: 0, stdout: stdout.to_string(), stderr: String::new() };
        let o = observe(&run, "A").expect("observes");
        let reason = "GitHub unreachable".to_owned();
        assert_eq!(
            o,
            Observed { word: "Not checked".into(), exit: 0, view: View::Unverified, holds_anywhere: false, reason }
        );
        assert!(observe(&run, "B").is_err());
    }
}
