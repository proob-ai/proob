//! `mutants.out/outcomes.json`, as cargo-mutants 27 writes it: the counts
//! over every mutant (`total_mutants`, `caught`, `missed`, `timeout`,
//! `unviable`) and one entry per scenario, the first of which is the
//! unmutated baseline. The baseline's `summary` says whether the selected
//! tests pass before any mutation; when they do not, no count means anything.
//!
//! Also the output of `cargo mutants --list --json`: an array with one
//! object per mutant, of which only `name` is read (the text `--re` matches).
use crate::error::ParseError;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutantsOutcome {
    pub total_mutants: u32,
    pub caught: u32,
    pub missed: u32,
    pub timeout: u32,
    pub unviable: u32,
    pub baseline: Baseline,
}

/// What the unmutated tree did under the selected tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Baseline {
    Passed,
    Failed,
    /// `--baseline skip`, or a file from a run that recorded none.
    NotRun,
}

#[derive(Deserialize)]
struct Lab {
    #[serde(default)]
    outcomes: Vec<ScenarioOutcome>,
    total_mutants: u32,
    #[serde(default)]
    caught: u32,
    #[serde(default)]
    missed: u32,
    #[serde(default)]
    timeout: u32,
    #[serde(default)]
    unviable: u32,
}

#[derive(Deserialize)]
struct ScenarioOutcome {
    scenario: serde_json::Value,
    summary: String,
}

pub fn parse_outcomes(json: &str) -> Result<MutantsOutcome, ParseError> {
    let lab: Lab = serde_json::from_str(json).map_err(|e| ParseError::Schema(format!("outcomes.json: {e}")))?;
    Ok(MutantsOutcome {
        total_mutants: lab.total_mutants,
        caught: lab.caught,
        missed: lab.missed,
        timeout: lab.timeout,
        unviable: lab.unviable,
        baseline: baseline_of(&lab.outcomes),
    })
}

#[derive(Deserialize)]
struct Listed {
    name: String,
}

/// The names `cargo mutants --list --json` printed, in its order.
pub fn parse_list(json: &str) -> Result<Vec<String>, ParseError> {
    let listed: Vec<Listed> =
        serde_json::from_str(json).map_err(|e| ParseError::Schema(format!("cargo mutants --list --json: {e}")))?;
    Ok(listed.into_iter().map(|l| l.name).collect())
}

fn baseline_of(outcomes: &[ScenarioOutcome]) -> Baseline {
    match outcomes.iter().find(|o| o.scenario == serde_json::Value::String("Baseline".into())) {
        Some(b) if b.summary == "Success" => Baseline::Passed,
        Some(_) => Baseline::Failed,
        None => Baseline::NotRun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_listed_names() {
        let json = r#"[{"name":"src/a.rs:4:5: replace f -> i64 with 0","package":"p","file":"src/a.rs","function":null,"span":{},"replacement":"0","genre":"FnValue","diff":""}]"#;
        assert_eq!(parse_list(json).expect("parses"), vec!["src/a.rs:4:5: replace f -> i64 with 0".to_string()]);
        assert_eq!(parse_list("[]").expect("parses"), Vec::<String>::new());
        assert!(parse_list(r#"{"name":"x"}"#).is_err());
    }

    #[test]
    fn reads_counts_and_the_baseline() {
        let json = r#"{"outcomes":[{"scenario":"Baseline","summary":"Success","log_path":"l","phase_results":[]},
            {"scenario":{"Mutant":{"name":"m","package":"p","file":"src/a.rs"}},"summary":"MissedMutant","log_path":"l","phase_results":[]}],
            "total_mutants":5,"missed":5,"caught":0,"timeout":0,"unviable":0,"success":0,"cargo_mutants_version":"27.1.0"}"#;
        let o = parse_outcomes(json).expect("parses");
        assert_eq!(
            o,
            MutantsOutcome {
                total_mutants: 5,
                caught: 0,
                missed: 5,
                timeout: 0,
                unviable: 0,
                baseline: Baseline::Passed
            }
        );
    }

    #[test]
    fn a_failed_baseline_and_an_absent_one_are_distinct() {
        let failed = r#"{"outcomes":[{"scenario":"Baseline","summary":"Failure"}],"total_mutants":0}"#;
        assert_eq!(parse_outcomes(failed).expect("parses").baseline, Baseline::Failed);
        let none = r#"{"outcomes":[],"total_mutants":0}"#;
        assert_eq!(parse_outcomes(none).expect("parses").baseline, Baseline::NotRun);
        assert!(parse_outcomes("{}").is_err());
    }
}
