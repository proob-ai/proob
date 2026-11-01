//! One `tlc2.TLC` model-checking run, as TLC 2.19 prints it
//! (`tests/fixtures/tla/README.md`), read into a closed class.
//!
//! TLC prints everything that decides the class on stdout: `Model checking
//! completed. No error has been found.` for a run that explored the whole
//! state space, `Error: Invariant <name> is violated.`, `Error: Temporal
//! properties were violated.`, or `Error: Deadlock reached.` followed by the
//! error trace (`State <n>: <action>` blocks), `***Parse Error***` or
//! `Semantic errors:` before `Error: Parsing or semantic analysis failed.`
//!
//! Classification fails closed: `NoError` needs the completion line and no
//! `Error:` line at all; a violation needs exactly one recognised error and
//! nothing else TLC calls an error; anything else is `Unrecognised`, never a
//! pass and never a violation. The exit code is the adapter's to check.

/// What one run says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    /// TLC explored every reachable state and found no error.
    NoError,
    /// A state violates the invariant `invariant`; `trace` is the error
    /// trace's first and last states.
    InvariantViolated {
        invariant: String,
        trace: String,
    },
    /// A behaviour violates a temporal property; `trace` as above.
    TemporalViolated {
        trace: String,
    },
    /// A reachable state has no successor and the cfg checks deadlock.
    Deadlock {
        trace: String,
    },
    /// SANY could not parse a module.
    ParseError {
        detail: String,
    },
    /// SANY parsed the modules but rejected their meaning (an unknown name, a
    /// wrong arity).
    SemanticError {
        detail: String,
    },
    /// The JVM ran out of memory.
    OutOfMemory,
    Unrecognised {
        detail: String,
    },
}

const NO_ERROR: &str = "Model checking completed. No error has been found.";
const INVARIANT: (&str, &str) = ("Error: Invariant ", " is violated.");
const TEMPORAL: &str = "Error: Temporal properties were violated.";
const DEADLOCK: &str = "Error: Deadlock reached.";
const PARSE: &str = "***Parse Error***";
const SEMANTIC: &str = "Semantic errors:";
const FRONT_END: &str = "Error: Parsing or semantic analysis failed.";
const OOM: &str = "java.lang.OutOfMemoryError";
/// `Error:` lines that introduce a trace rather than name an error.
const TRACE_INTROS: [&str; 2] =
    ["Error: The behavior up to this point is:", "Error: The following behavior constitutes a counter-example:"];
const VERSION: &str = "TLC2 Version ";
const DETAIL_LINES: usize = 6;

/// The class of one run, from its stdout and stderr.
pub fn classify(stdout: &str, stderr: &str) -> Class {
    let all = format!("{stdout}\n{stderr}");
    let lines: Vec<&str> = all.lines().collect();
    if all.contains(OOM) {
        return Class::OutOfMemory;
    }
    if lines.iter().any(|l| l.trim() == PARSE) {
        return Class::ParseError { detail: detail_after(&lines, PARSE) };
    }
    if lines.iter().any(|l| l.trim() == SEMANTIC) {
        return Class::SemanticError { detail: detail_after(&lines, SEMANTIC) };
    }
    let errors: Vec<(usize, &str)> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with("Error:") && !TRACE_INTROS.contains(&l.trim_end()))
        .map(|(i, l)| (i, l.trim_end()))
        .collect();
    let completed = lines.iter().any(|l| l.trim_end() == NO_ERROR);
    match (completed, errors.as_slice()) {
        (true, []) => Class::NoError,
        (false, [(at, line)]) => violation(&lines, *at, line),
        (_, []) => unrecognised("no completion line and no error", &lines),
        _ => unrecognised(&errors.iter().map(|(_, l)| *l).collect::<Vec<_>>().join("; "), &lines),
    }
}

/// The class of the one `Error:` line at `at`.
fn violation(lines: &[&str], at: usize, line: &str) -> Class {
    let trace = || trace(&lines[at + 1..]);
    if line == TEMPORAL {
        return Class::TemporalViolated { trace: trace() };
    }
    if line == DEADLOCK {
        return Class::Deadlock { trace: trace() };
    }
    if line == FRONT_END {
        return unrecognised("parsing or semantic analysis failed with no parse or semantic error printed", lines);
    }
    match line.strip_prefix(INVARIANT.0).and_then(|r| r.strip_suffix(INVARIANT.1)) {
        Some(name) if is_name(name) => Class::InvariantViolated { invariant: name.to_string(), trace: trace() },
        _ => Class::Unrecognised { detail: line.to_string() },
    }
}

fn unrecognised(why: &str, lines: &[&str]) -> Class {
    let last: Vec<&str> = lines.iter().rev().filter(|l| !l.trim().is_empty()).take(3).copied().collect();
    let tail: Vec<&str> = last.into_iter().rev().collect();
    Class::Unrecognised { detail: format!("{why} (last lines: {})", tail.join(" | ")) }
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Up to `DETAIL_LINES` non-blank lines after the `marker` line, before TLC
/// moves on (`Starting...`) or gives up.
fn detail_after(lines: &[&str], marker: &str) -> String {
    let stops = ["Starting...", "Residual stack trace", "Fatal errors", "Error:", "tla2sany."];
    lines
        .iter()
        .skip_while(|l| l.trim() != marker)
        .skip(1)
        .map(|l| l.trim())
        .take_while(|l| !stops.iter().any(|s| l.starts_with(s)))
        .filter(|l| !l.is_empty())
        .take(DETAIL_LINES)
        .collect::<Vec<_>>()
        .join("; ")
}

/// The error trace's first and last states, with how many there are:
/// `State <n>: …` and `Back to state <n>: …` headers, each with the
/// variable lines under it.
pub fn trace(lines: &[&str]) -> String {
    let blocks = states(lines);
    match blocks.as_slice() {
        [] => "(TLC printed no trace)".to_string(),
        [only] => only.join("\n"),
        [first, .., last] => {
            let gap = if blocks.len() > 2 { format!("\n… ({} states in all)\n", blocks.len()) } else { "\n".into() };
            format!("{}{gap}{}", first.join("\n"), last.join("\n"))
        }
    }
}

fn states<'a>(lines: &[&'a str]) -> Vec<Vec<&'a str>> {
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut open = false;
    for line in lines {
        if is_state_header(line) {
            blocks.push(vec![line.trim_end()]);
            open = true;
        } else if open && is_state_line(line) {
            if let Some(b) = blocks.last_mut() {
                b.push(line.trim_end());
            }
        } else {
            open = false;
        }
    }
    blocks
}

fn is_state_header(line: &str) -> bool {
    let rest = line.strip_prefix("State ").or_else(|| line.strip_prefix("Back to state "));
    rest.is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()) && r.contains(':'))
}

/// A variable's line in a state: `/\ x = …`, a continuation (indented), or
/// a lone `x = …` when the spec has one variable.
fn is_state_line(line: &str) -> bool {
    let lone = line.split_once(" = ").is_some_and(|(name, _)| is_name(name));
    line.starts_with("/\\ ") || line.starts_with([' ', '\t']) && !line.trim().is_empty() || lone
}

/// The TLC release a run printed, from `TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802)`.
pub fn version(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|l| l.strip_prefix(VERSION)).map(|v| v.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn read(name: &str) -> String {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tla");
        std::fs::read_to_string(dir.join(name)).unwrap_or_default()
    }

    fn captured(case: &str) -> Class {
        classify(&read(&format!("{case}.stdout.txt")), &read(&format!("{case}.stderr.txt")))
    }

    #[test]
    fn a_complete_run_without_error_is_no_error_and_names_its_release() {
        assert_eq!(captured("no_error"), Class::NoError);
        assert_eq!(version(&read("no_error.stdout.txt")).as_deref(), Some("2.19 of 08 August 2024 (rev: 5a47802)"));
    }

    #[test]
    fn an_invariant_violation_names_the_invariant_and_keeps_the_first_and_last_states() {
        let Class::InvariantViolated { invariant, trace } = captured("invariant_violated") else {
            panic!("{:?}", captured("invariant_violated"))
        };
        assert_eq!(invariant, "CountBelowTwo");
        assert!(trace.starts_with("State 1: <Initial predicate>\n/\\ lock = FALSE"), "{trace}");
        assert!(trace.contains("… (4 states in all)"), "{trace}");
        assert!(trace.ends_with("/\\ count = 2"), "{trace}");
        assert!(!trace.contains("State 2:"), "only the first and last: {trace}");
    }

    #[test]
    fn a_temporal_violation_and_a_deadlock_carry_their_traces() {
        let Class::TemporalViolated { trace } = captured("temporal_violated") else { panic!() };
        assert!(trace.starts_with("State 1: <Initial predicate>") && trace.ends_with("State 2: Stuttering"), "{trace}");
        let Class::Deadlock { trace } = captured("deadlock") else { panic!("{:?}", captured("deadlock")) };
        assert!(trace.contains("State 2: <Acquire"), "{trace}");
    }

    #[test]
    fn front_end_errors_say_where() {
        let Class::ParseError { detail } = captured("parse_error") else { panic!("{:?}", captured("parse_error")) };
        assert!(detail.contains("line 34, column 18"), "{detail}");
        let Class::SemanticError { detail } = captured("semantic_error") else { panic!() };
        assert!(detail.contains("Unknown operator: `cnt'."), "{detail}");
    }

    #[test]
    fn anything_else_fails_closed() {
        assert!(matches!(captured("unrecognised"), Class::Unrecognised { .. }));
        assert!(matches!(classify("", ""), Class::Unrecognised { .. }));
        let header = "TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802)\nStarting...\n";
        assert!(matches!(classify(header, ""), Class::Unrecognised { .. }), "started, never finished");
        let both = format!("{NO_ERROR}\nError: Invariant Inv is violated.\n");
        assert!(matches!(classify(&both, ""), Class::Unrecognised { .. }), "a completion and an error");
        let unknown =
            format!("{NO_ERROR}\nError: The invariant Inv specified in the configuration file is not defined.\n");
        assert!(matches!(classify(&unknown, ""), Class::Unrecognised { .. }), "an error TLC printed beside completing");
        let two = "Error: Invariant A is violated.\nError: Deadlock reached.\n";
        assert!(matches!(classify(two, ""), Class::Unrecognised { .. }));
        assert!(matches!(classify("Error: Invariant a b is violated.\n", ""), Class::Unrecognised { .. }));
        // Not captured: a JVM out of memory, whatever else it printed.
        assert_eq!(
            classify(NO_ERROR, "Exception in thread \"main\" java.lang.OutOfMemoryError: Java heap space"),
            Class::OutOfMemory
        );
    }

    #[test]
    fn a_lone_variable_and_a_lasso_are_read_as_states() {
        let lines = [
            "State 1: <Initial predicate>",
            "x = 0",
            "",
            "State 2: <Inc>",
            "x = 1",
            "",
            "Back to state 1: <Inc>",
            "Finished",
        ];
        assert_eq!(trace(&lines), "State 1: <Initial predicate>\nx = 0\n… (3 states in all)\nBack to state 1: <Inc>");
    }
}
