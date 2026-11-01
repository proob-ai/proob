//! `mutants.out/outcomes.json`, as cargo-mutants 27 writes it: the counts
//! over every mutant (`total_mutants`, `caught`, `missed`, `timeout`,
//! `unviable`) and one entry per scenario, the first of which is the
//! unmutated baseline. The baseline's `summary` says whether the selected
//! tests pass before any mutation; when they do not, no count means anything.
//!
//! Also the output of `cargo mutants --list --json [--diff]`: an array with
//! one object per mutant, read for its `name` (the text `--re` matches), its
//! `file`, the line its span starts on, the function it sits in (named by
//! its path inside the file: inline modules and impls, not file modules),
//! and, with `--diff`, the unified diff that makes it.
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

/// One listed mutant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    /// Relative to the directory cargo-mutants ran in.
    pub file: String,
    /// 1-based line the mutated span starts on.
    pub line: usize,
    /// The enclosing function's path inside its file, when there is one.
    pub function: Option<String>,
    /// The unified diff of `file`; empty without `--diff`.
    pub diff: String,
}

#[derive(Deserialize)]
struct Wire {
    name: String,
    file: String,
    span: Span,
    function: Option<Function>,
    #[serde(default)]
    diff: String,
}

#[derive(Deserialize)]
struct Span {
    start: Position,
}

#[derive(Deserialize)]
struct Position {
    line: usize,
}

#[derive(Deserialize)]
struct Function {
    function_name: String,
}

/// The mutants `cargo mutants --list --json` printed, in its order.
pub fn parse_listing(json: &str) -> Result<Vec<Listed>, ParseError> {
    let listed: Vec<Wire> =
        serde_json::from_str(json).map_err(|e| ParseError::Schema(format!("cargo mutants --list --json: {e}")))?;
    Ok(listed
        .into_iter()
        .map(|w| Listed {
            name: w.name,
            file: w.file,
            line: w.span.start.line,
            function: w.function.map(|f| f.function_name),
            diff: w.diff,
        })
        .collect())
}

/// The names `cargo mutants --list --json` printed, in its order.
pub fn parse_list(json: &str) -> Result<Vec<String>, ParseError> {
    Ok(parse_listing(json)?.into_iter().map(|l| l.name).collect())
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
        let json = r#"[{"name":"src/a.rs:4:5: replace f -> i64 with 0","package":"p","file":"src/a.rs","function":null,"span":{"start":{"line":4,"column":5},"end":{"line":4,"column":9}},"replacement":"0","genre":"FnValue"}]"#;
        assert_eq!(parse_list(json).expect("parses"), vec!["src/a.rs:4:5: replace f -> i64 with 0".to_string()]);
        let listed = parse_listing(json).expect("parses");
        assert_eq!((listed[0].file.as_str(), listed[0].line, &listed[0].function), ("src/a.rs", 4, &None));
        assert_eq!(listed[0].diff, "", "no --diff");
        assert_eq!(parse_list("[]").expect("parses"), Vec::<String>::new());
        assert!(parse_list(r#"{"name":"x"}"#).is_err());
    }

    /// `tests/fixtures/kani/mutants_list.json`: `--list --json --diff` over the Kani fixture.
    #[test]
    fn reads_a_captured_listing_with_its_diffs() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/kani");
        let json = std::fs::read_to_string(dir.join("mutants_list.json")).expect("captured listing");
        let listed = parse_listing(&json).expect("parses");
        assert!(!listed.is_empty());
        assert!(listed.iter().all(|m| m.file == "src/lib.rs" && m.line > 0 && m.diff.starts_with("--- src/lib.rs\n")));
        let first = &listed[0];
        assert_eq!(first.name, "src/lib.rs:6:5: replace clamp -> i32 with 0");
        assert_eq!((first.line, first.function.as_deref()), (6, Some("clamp")));
        assert!(first.diff.contains("\n+    0 /* ~ changed by cargo-mutants ~ */\n"), "{}", first.diff);
        assert!(listed.iter().any(|m| m.function.as_deref() == Some("verification::clamp_stays_within_bounds")));
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
