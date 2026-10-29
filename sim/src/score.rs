//! `cargo xtask bench soundness`: target/sim/verdicts.jsonl scored with the
//! SV-COMP scheme of bench/README.md ("Measure 3"). `Holds` is proob's "true"
//! claim and `Broken` its "false" claim; each is scored against the world's
//! `truth`, never against what the scenario expected. Any wrong `Holds`
//! fails the run, whatever the total.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::verdicts::Truth;
use crate::world::Fallible;

/// What a rendered word claims about the property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    Holds,
    Broken,
    Other,
}

pub fn claim(rendered: &str) -> Claim {
    match rendered {
        "Holds" => Claim::Holds,
        "Broken" => Claim::Broken,
        _ => Claim::Other,
    }
}

/// The SV-COMP cells, in the order the table prints them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    CorrectHolds,
    CorrectBroken,
    Other,
    WrongBroken,
    WrongHolds,
}

const CELLS: [Cell; 5] = [Cell::CorrectHolds, Cell::CorrectBroken, Cell::Other, Cell::WrongBroken, Cell::WrongHolds];

/// `Broken` is wrong unless the code is violated; `Holds` is wrong unless the
/// world holds (an `Unknown` world lacks what `Holds` claims).
pub fn cell(c: Claim, t: Truth) -> Cell {
    match (c, t) {
        (Claim::Holds, Truth::Holds) => Cell::CorrectHolds,
        (Claim::Holds, _) => Cell::WrongHolds,
        (Claim::Broken, Truth::Violated) => Cell::CorrectBroken,
        (Claim::Broken, _) => Cell::WrongBroken,
        (Claim::Other, _) => Cell::Other,
    }
}

impl Cell {
    pub fn points(self) -> i64 {
        match self {
            Cell::CorrectHolds => 2,
            Cell::CorrectBroken => 1,
            Cell::Other => 0,
            Cell::WrongBroken => -16,
            Cell::WrongHolds => -32,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Cell::CorrectHolds => "correct_holds",
            Cell::CorrectBroken => "correct_broken",
            Cell::Other => "other",
            Cell::WrongBroken => "wrong_broken",
            Cell::WrongHolds => "wrong_holds",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Cell::CorrectHolds => "correct Holds",
            Cell::CorrectBroken => "correct Broken",
            Cell::Other => "other",
            Cell::WrongBroken => "wrong Broken",
            Cell::WrongHolds => "wrong Holds",
        }
    }
}

/// The best a verdict could score in a world of this truth.
pub fn best(t: Truth) -> i64 {
    match t {
        Truth::Holds => 2,
        Truth::Violated => 1,
        Truth::Unknown => 0,
    }
}

/// One line of verdicts.jsonl, as far as scoring reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub scenario: String,
    pub step: String,
    pub rendered: String,
    pub truth: Truth,
}

pub fn parse_line(n: usize, line: &str) -> Fallible<Verdict> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("line {n}: {e}"))?;
    let text = |k: &str| v[k].as_str().map(str::to_owned).ok_or_else(|| format!("line {n}: no \"{k}\""));
    let truth = v["truth"]
        .as_str()
        .and_then(Truth::parse)
        .ok_or_else(|| format!("line {n}: no \"truth\" of holds, violated, unknown"))?;
    Ok(Verdict { scenario: text("scenario")?, step: text("step")?, rendered: text("rendered")?, truth })
}

pub fn parse(text: &str) -> Fallible<Vec<Verdict>> {
    let verdicts = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| parse_line(i + 1, l))
        .collect::<Fallible<Vec<_>>>()?;
    match verdicts.is_empty() {
        true => Err("no verdicts to score; run cargo xtask sim first".to_owned()),
        false => Ok(verdicts),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// Per cell, in `CELLS` order.
    pub counts: [usize; 5],
    pub score: i64,
    pub max: i64,
    /// `scenario: step (rendered, truth)` of every wrong Holds: the reproductions.
    pub wrong_holds: Vec<String>,
}

pub fn tally(verdicts: &[Verdict]) -> Table {
    let cells: Vec<Cell> = verdicts.iter().map(|v| cell(claim(&v.rendered), v.truth)).collect();
    let counts = CELLS.map(|c| cells.iter().filter(|x| **x == c).count());
    let wrong_holds = verdicts
        .iter()
        .zip(&cells)
        .filter(|(_, c)| **c == Cell::WrongHolds)
        .map(|(v, _)| format!("{}: {} (rendered {}, truth {})", v.scenario, v.step, v.rendered, v.truth.name()))
        .collect();
    Table {
        counts,
        score: cells.iter().map(|c| c.points()).sum(),
        max: verdicts.iter().map(|v| best(v.truth)).sum(),
        wrong_holds,
    }
}

impl Table {
    pub fn sound(&self) -> bool {
        self.wrong_holds.is_empty()
    }

    pub fn render(&self) -> String {
        let rows: String = CELLS
            .iter()
            .zip(self.counts)
            .map(|(c, n)| format!("  {:<16} {:>+4} {:>5} {:>+7}\n", c.title(), c.points(), n, c.points() * n as i64))
            .collect();
        let worlds: usize = self.counts.iter().sum();
        let verdict = match self.sound() {
            true => "sound: no wrong Holds".to_owned(),
            false => format!(
                "UNSOUND: {} wrong Holds\n{}",
                self.wrong_holds.len(),
                self.wrong_holds.iter().map(|w| format!("  {w}\n")).collect::<String>()
            ),
        };
        format!(
            "verdict soundness (SV-COMP scoring), {worlds} verdicts\n  {:<16} {:>4} {:>5} {:>7}\n{rows}  {:<16} {:>4} {worlds:>5} {:>+7}\n  maximum possible {:>18}\n{verdict}\n",
            "cell", "pts", "count", "score", "total", "", self.score, self.max
        )
    }

    pub fn json(&self) -> Value {
        let mut out = json!({ "score": self.score, "max": self.max, "verdicts": self.counts.iter().sum::<usize>(), "wrong_holds_steps": self.wrong_holds });
        CELLS.iter().zip(self.counts).for_each(|(c, n)| out[c.key()] = json!(n));
        out
    }
}

/// Reads `<dir>/verdicts.jsonl`, writes `<dir>/soundness.json`, prints the
/// table; `Ok(false)` when any Holds is wrong.
pub fn score_file(dir: &Path) -> Fallible<bool> {
    let input = dir.join("verdicts.jsonl");
    let text =
        fs::read_to_string(&input).map_err(|e| format!("{}: {e}; run cargo xtask sim first", input.display()))?;
    let table = tally(&parse(&text)?);
    let output = dir.join("soundness.json");
    let pretty = serde_json::to_string_pretty(&table.json()).map_err(|e| e.to_string())?;
    fs::write(&output, pretty + "\n").map_err(|e| format!("{}: {e}", output.display()))?;
    print!("{}", table.render());
    println!("written: {}", output.display());
    Ok(table.sound())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(rendered: &str, truth: Truth) -> Verdict {
        Verdict { scenario: "s".into(), step: "w".into(), rendered: rendered.into(), truth }
    }

    #[test]
    fn every_scoring_cell() {
        let cases = [
            ("Holds", Truth::Holds, Cell::CorrectHolds, 2),
            ("Broken", Truth::Violated, Cell::CorrectBroken, 1),
            ("Not checked", Truth::Holds, Cell::Other, 0),
            ("Checked (strength pending)", Truth::Violated, Cell::Other, 0),
            ("Re-approve (check amended)", Truth::Unknown, Cell::Other, 0),
            ("Broken", Truth::Holds, Cell::WrongBroken, -16),
            ("Broken", Truth::Unknown, Cell::WrongBroken, -16),
            ("Holds", Truth::Violated, Cell::WrongHolds, -32),
            ("Holds", Truth::Unknown, Cell::WrongHolds, -32),
        ];
        for (word, truth, want, points) in cases {
            let got = cell(claim(word), truth);
            assert_eq!((got, got.points()), (want, points), "{word} / {truth:?}");
        }
    }

    #[test]
    fn the_total_and_the_maximum() {
        let t = tally(&[
            v("Holds", Truth::Holds),
            v("Broken", Truth::Violated),
            v("Not checked", Truth::Unknown),
            v("Broken", Truth::Holds),
        ]);
        assert_eq!(t.counts, [1, 1, 1, 1, 0]);
        assert_eq!((t.score, t.max), (2 + 1 - 16, 2 + 1 + 2), "the false alarm was a Holds world");
        assert!(t.sound(), "a wrong Broken does not block");
    }

    #[test]
    fn any_wrong_holds_is_unsound_and_named() {
        let t = tally(&[v("Holds", Truth::Holds), v("Holds", Truth::Unknown)]);
        assert!(!t.sound());
        assert_eq!(t.wrong_holds, ["s: w (rendered Holds, truth unknown)"]);
        assert!(t.render().contains("UNSOUND: 1 wrong Holds"));
        assert_eq!(t.json()["wrong_holds"], 1);
    }

    #[test]
    fn lines_need_a_truth_and_a_file_needs_a_line() {
        let ok = r#"{"scenario":"a","step":"b","expected":null,"rendered":"Holds","truth":"holds"}"#;
        assert_eq!(
            parse(ok).expect("parses"),
            [Verdict { scenario: "a".into(), step: "b".into(), rendered: "Holds".into(), truth: Truth::Holds }]
        );
        assert!(
            parse(r#"{"scenario":"a","step":"b","expected":"Holds","rendered":"Holds"}"#)
                .unwrap_err()
                .contains("truth")
        );
        assert!(parse("\n").unwrap_err().contains("no verdicts"));
    }

    #[test]
    fn score_file_writes_the_json_and_reports_soundness() {
        let dir = tempfile::tempdir().expect("a dir");
        let line = |r: &str, t: &str| {
            format!("{{\"scenario\":\"a\",\"step\":\"b\",\"expected\":null,\"rendered\":\"{r}\",\"truth\":\"{t}\"}}\n")
        };
        fs::write(dir.path().join("verdicts.jsonl"), line("Holds", "holds") + &line("Broken", "violated"))
            .expect("writes");
        assert_eq!(score_file(dir.path()), Ok(true));
        let json: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("soundness.json")).expect("reads")).expect("json");
        assert_eq!(
            (json["score"].as_i64(), json["max"].as_i64(), json["correct_holds"].as_u64()),
            (Some(3), Some(3), Some(1))
        );
        fs::write(dir.path().join("verdicts.jsonl"), line("Holds", "violated")).expect("writes");
        assert_eq!(score_file(dir.path()), Ok(false));
    }
}
