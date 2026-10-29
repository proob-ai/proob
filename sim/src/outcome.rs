//! What a scenario came to, and the summary table.

/// A scenario's result. `Skipped` is for a stand-in this machine lacks
/// (Docker not running, dist not installed), never for a wrong verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ok(String),
    Failed(String),
    Skipped(String),
}

impl Outcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, Outcome::Ok(_))
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Outcome::Failed(_))
    }

    /// Wrong verdicts are a failure listing each; a step that could not run is one.
    pub fn of_verdicts(result: Result<Vec<String>, String>, steps: usize) -> Outcome {
        match result {
            Ok(wrong) if wrong.is_empty() => Outcome::Ok(format!("{steps} verdicts as expected")),
            Ok(wrong) => Outcome::Failed(wrong.join("; ")),
            Err(e) => Outcome::Failed(e),
        }
    }

    fn cell(&self) -> String {
        match self {
            Outcome::Ok(what) => format!("ok       {what}"),
            Outcome::Failed(why) => format!("failed   {why}"),
            Outcome::Skipped(why) => format!("skipped  ({why})"),
        }
    }
}

pub fn table(rows: &[(&str, Outcome, f64)]) -> String {
    let lines: String =
        rows.iter().map(|(name, o, secs)| format!("  {name:<11} {secs:>6.1}s  {}\n", o.cell())).collect();
    format!("\nsim summary\n{lines}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_lists_become_outcomes() {
        assert_eq!(Outcome::of_verdicts(Ok(vec![]), 3), Outcome::Ok("3 verdicts as expected".into()));
        assert_eq!(Outcome::of_verdicts(Ok(vec!["a".into(), "b".into()]), 3), Outcome::Failed("a; b".into()));
        assert!(Outcome::of_verdicts(Err("x".into()), 3).is_failed());
    }

    #[test]
    fn the_table_names_every_scenario_and_its_reason() {
        let text = table(&[
            ("shim", Outcome::Ok("s".into()), 1.0),
            ("workflows", Outcome::Skipped("docker not running".into()), 0.0),
        ]);
        assert!(text.contains("shim") && text.contains("ok       s"));
        assert!(text.contains("skipped  (docker not running)"));
    }
}
