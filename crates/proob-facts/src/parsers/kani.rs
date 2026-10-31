//! One `cargo kani --harness <h> --exact --sarif <file>` run, as Kani 0.68.0
//! prints it (`tests/fixtures/kani/README.md`), read into a closed class.
//!
//! stdout carries everything that decides the class: the `Checking harness`
//! line, the `RESULTS:` block (one `Check N: <property>` per check with its
//! `- Status:`, `- Description:`, and `- Location:` lines), the `Failed
//! Checks:` summary, the per-harness `VERIFICATION:- SUCCESSFUL|FAILED` line,
//! and Kani's warnings. SARIF lists only checks that did not succeed and never
//! covers, so it is read to fail closed (a `SUCCESSFUL` run must have written
//! one with no results) and for the timeout and out-of-memory rule ids.
//!
//! Classification fails closed: a `SUCCESSFUL` verdict is `Verified` only
//! with exactly the one harness checked, no check `FAILURE` or `UNDETERMINED`,
//! every cover `SATISFIED`, and an empty SARIF; anything this module does not
//! recognise is `Unrecognised`, never a pass and never a failure.
use serde::Deserialize;

/// What one run says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    /// Every check held: `checks` is how many checks were proved (`SUCCESS`),
    /// covers and unreachable checks not counted.
    Verified {
        checks: u32,
    },
    /// A harness assertion or a Kani safety check failed; the diagnostics are
    /// the `Failed Checks:` lines with their locations.
    Failed {
        diagnostics: String,
    },
    /// The harness reached a construct Kani does not support.
    Unsupported {
        construct: String,
    },
    /// An unwinding assertion failed: the bound is too small for a loop.
    Unwinding {
        at: String,
    },
    /// Checks Kani could not decide, with no other reason given.
    Undetermined {
        checks: Vec<String>,
    },
    /// CBMC timed out.
    Timeout,
    /// CBMC ran out of memory.
    OutOfMemory,
    /// A cover the harness states was not satisfied: it cannot reach that case.
    VacuousCover {
        cover: String,
    },
    /// CBMC's C preprocessor (`goto-cc`) failed on Kani's own C library.
    PreprocessorFailed {
        detail: String,
    },
    /// The crate did not compile under Kani.
    CompileError {
        detail: String,
    },
    /// `--harness` with `--exact` matched nothing.
    HarnessNotFound,
    Unrecognised {
        detail: String,
    },
}

/// One `Check N:` entry of the `RESULTS:` block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckResult {
    /// `<function>.<class>.<n>`, such as `verification::pass.cover.1`.
    pub property: String,
    pub status: String,
    pub description: String,
    pub location: String,
}

impl CheckResult {
    fn class(&self) -> &str {
        self.property.rsplit('.').nth(1).unwrap_or("")
    }

    fn is_cover(&self) -> bool {
        self.class() == "cover"
    }

    fn summary(&self) -> String {
        format!("{} ({})", self.description, self.location)
    }
}

const PREPROCESSOR: [&str; 2] = ["preprocessing has failed", "goto-cc exited with status"];
const NOT_FOUND: &str = "Failed to match the following harness(es)";
const OOM: &str = "CBMC appears to have run out of memory";
const TIMEOUT: &str = "CBMC timed out";
const UNSUPPORTED: &str = "A Rust construct that is not currently supported by Kani was found to be reachable";
const UNWINDING: &str = "Verification output shows one or more unwinding failures";
const SUCCESSFUL: &str = "VERIFICATION:- SUCCESSFUL";
const FAILED: &str = "VERIFICATION:- FAILED";

/// The class of one run of `harness` (its fully qualified name). `sarif` is
/// the file's text when Kani wrote one.
pub fn classify(stdout: &str, stderr: &str, sarif: Option<&str>, harness: &str) -> Class {
    let all = format!("{stdout}\n{stderr}");
    let rules = sarif.map(sarif_rules);
    let has_rule = |id: &str| rules.as_ref().and_then(|r| r.as_ref().ok()).is_some_and(|r| r.iter().any(|x| x == id));
    if PREPROCESSOR.iter().any(|m| all.contains(m)) {
        return Class::PreprocessorFailed { detail: preprocessor_detail(&all) };
    }
    if all.contains(NOT_FOUND) {
        return Class::HarnessNotFound;
    }
    if all.contains(OOM) || has_rule("kani.cbmc.oom") {
        return Class::OutOfMemory;
    }
    if all.contains(TIMEOUT) || has_rule("kani.cbmc.timeout") {
        return Class::Timeout;
    }
    let verdicts: Vec<&str> = stdout.lines().map(str::trim).filter(|l| *l == SUCCESSFUL || *l == FAILED).collect();
    let Some(&verdict) = verdicts.first() else {
        return no_verdict(&all);
    };
    let harnesses: Vec<&str> = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("Checking harness "))
        .map(|h| h.trim_end().trim_end_matches("..."))
        .collect();
    if harnesses != [harness] || verdicts.len() != 1 {
        return Class::Unrecognised {
            detail: format!(
                "expected one run of harness `{harness}`, found {harnesses:?} with {} verdicts",
                verdicts.len()
            ),
        };
    }
    judge(stdout, verdict == SUCCESSFUL, rules)
}

/// The verdict line was found: read the checks.
fn judge(stdout: &str, successful: bool, sarif: Option<Result<Vec<String>, String>>) -> Class {
    let checks = results(stdout);
    if let Some(c) = with(&checks, "FAILURE").find(|c| c.class() == "unsupported_construct") {
        return Class::Unsupported { construct: c.summary() };
    }
    if stdout.contains(UNSUPPORTED) {
        return Class::Unsupported { construct: "(Kani named no construct)".to_string() };
    }
    if let Some(c) = with(&checks, "FAILURE").find(|c| c.class() == "unwind") {
        return Class::Unwinding { at: c.summary() };
    }
    if stdout.contains(UNWINDING) {
        return Class::Unwinding { at: "(Kani named no loop)".to_string() };
    }
    if with(&checks, "FAILURE").next().is_some() {
        return if successful {
            Class::Unrecognised { detail: "a check FAILURE under a SUCCESSFUL verdict".to_string() }
        } else {
            Class::Failed { diagnostics: failed_checks(stdout, &checks) }
        };
    }
    let undetermined: Vec<String> = with(&checks, "UNDETERMINED").map(CheckResult::summary).collect();
    if !undetermined.is_empty() {
        return Class::Undetermined { checks: undetermined };
    }
    if !successful {
        return Class::Unrecognised { detail: "a FAILED verdict with no failed check".to_string() };
    }
    if let Some(c) = checks.iter().find(|c| c.is_cover() && c.status != "SATISFIED") {
        return Class::VacuousCover { cover: format!("{} is {}", c.summary(), c.status) };
    }
    if let Some(c) = checks.iter().find(|c| !c.is_cover() && !matches!(c.status.as_str(), "SUCCESS" | "UNREACHABLE")) {
        return Class::Unrecognised { detail: format!("check status `{}` for {}", c.status, c.property) };
    }
    match sarif {
        Some(Ok(rules)) if rules.is_empty() => {
            Class::Verified { checks: with(&checks, "SUCCESS").filter(|c| !c.is_cover()).count() as u32 }
        }
        Some(Ok(rules)) => {
            Class::Unrecognised { detail: format!("SARIF reports {rules:?} under a SUCCESSFUL verdict") }
        }
        Some(Err(e)) => Class::Unrecognised { detail: format!("unreadable SARIF: {e}") },
        None => Class::Unrecognised { detail: "a SUCCESSFUL verdict but no SARIF file".to_string() },
    }
}

/// The checks with `status`.
fn with<'a>(checks: &'a [CheckResult], status: &'a str) -> impl Iterator<Item = &'a CheckResult> {
    checks.iter().filter(move |c| c.status == status)
}

/// The `Check N:` entries of the `RESULTS:` block, in order.
pub fn results(stdout: &str) -> Vec<CheckResult> {
    let mut out: Vec<CheckResult> = Vec::new();
    for line in stdout.lines() {
        let field = |name: &str| line.trim().strip_prefix(name).map(str::to_string);
        if let Some(rest) = line.strip_prefix("Check ").and_then(|r| r.split_once(": ")) {
            if rest.0.bytes().all(|b| b.is_ascii_digit()) {
                out.push(CheckResult { property: rest.1.trim().to_string(), ..CheckResult::default() });
            }
        } else if let Some(last) = out.last_mut() {
            if let Some(v) = field("- Status: ") {
                last.status = v;
            } else if let Some(v) = field("- Description: ") {
                last.description = v.trim_matches('"').to_string();
            } else if let Some(v) = field("- Location: ") {
                last.location = v;
            }
        }
    }
    out
}

/// The `Failed Checks:` lines and the `File:` lines under them; the failed
/// checks themselves when that summary is missing.
fn failed_checks(stdout: &str, checks: &[CheckResult]) -> String {
    let block: Vec<&str> = stdout
        .lines()
        .skip_while(|l| !l.starts_with("Failed Checks:"))
        .take_while(|l| !l.trim().is_empty())
        .map(str::trim)
        .collect();
    if block.is_empty() {
        checks.iter().filter(|c| c.status == "FAILURE").map(CheckResult::summary).collect::<Vec<_>>().join("\n")
    } else {
        block.join("\n")
    }
}

/// No verdict line: a compile error, or something else.
fn no_verdict(all: &str) -> Class {
    let errors: Vec<&str> = all.lines().map(str::trim).filter(|l| l.starts_with("error")).collect();
    let compiled = all.contains("could not compile") || all.contains("Failed to execute cargo");
    match (compiled, errors.is_empty()) {
        (true, _) => Class::CompileError { detail: errors.join("\n") },
        (false, false) => Class::Unrecognised { detail: errors.join("\n") },
        (false, true) => Class::Unrecognised { detail: "no VERIFICATION line".to_string() },
    }
}

/// The compiler's `fatal error:` line, else the `goto-cc` line.
fn preprocessor_detail(all: &str) -> String {
    let lines: Vec<&str> = all.lines().map(str::trim).collect();
    lines
        .iter()
        .find(|l| l.contains("fatal error:"))
        .or_else(|| lines.iter().find(|l| l.contains("goto-cc exited")))
        .unwrap_or(&"preprocessing has failed")
        .to_string()
}

#[derive(Deserialize)]
struct Sarif {
    runs: Vec<SarifRun>,
}

#[derive(Deserialize)]
struct SarifRun {
    #[serde(default)]
    results: Vec<SarifResult>,
}

#[derive(Deserialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
}

/// The rule id of every result in the SARIF log.
pub fn sarif_rules(text: &str) -> Result<Vec<String>, String> {
    let log: Sarif = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if log.runs.is_empty() {
        return Err("no runs".to_string());
    }
    Ok(log.runs.into_iter().flat_map(|r| r.results).map(|r| r.rule_id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn read(name: &str) -> Option<String> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/kani");
        std::fs::read_to_string(dir.join(name)).ok()
    }

    /// The class of a captured case, run as harness `verification::<case>`.
    fn captured(case: &str) -> Class {
        let stdout = read(&format!("{case}.stdout.txt")).expect("stdout capture");
        let stderr = read(&format!("{case}.stderr.txt")).expect("stderr capture");
        classify(&stdout, &stderr, read(&format!("{case}.sarif")).as_deref(), &format!("verification::{case}"))
    }

    fn pass() -> (String, String, String) {
        (read("pass.stdout.txt").unwrap(), read("pass.stderr.txt").unwrap(), read("pass.sarif").unwrap())
    }

    #[test]
    fn a_passing_harness_is_verified_with_its_proved_checks_counted() {
        assert_eq!(captured("pass"), Class::Verified { checks: 1 }, "two satisfied covers are not counted");
    }

    #[test]
    fn an_assertion_and_a_safety_check_failure_are_failures_with_their_location() {
        let Class::Failed { diagnostics } = captured("fail_assert") else { panic!("{:?}", captured("fail_assert")) };
        assert_eq!(
            diagnostics,
            "Failed Checks: assertion failed: buggy_abs(x) > 0\nFile: \"src/lib.rs\", line 11, in verification::fail_assert"
        );
        let Class::Failed { diagnostics } = captured("fail_overflow") else { panic!() };
        assert!(diagnostics.contains("attempt to negate with overflow"), "{diagnostics}");
    }

    #[test]
    fn an_unwinding_failure_is_never_a_failure() {
        let Class::Unwinding { at } = captured("unwind_short") else { panic!("{:?}", captured("unwind_short")) };
        assert_eq!(at, "unwinding assertion loop 0 (src/lib.rs:3:68 in function sum_to)");
    }

    #[test]
    fn an_unsatisfiable_cover_under_a_successful_verdict_is_vacuous() {
        // The passing capture says `SAT checker: instance is UNSATISFIABLE` too: only the cover's status counts.
        assert!(pass().0.contains("instance is UNSATISFIABLE"));
        let Class::VacuousCover { cover } = captured("cover_unsat") else { panic!("{:?}", captured("cover_unsat")) };
        assert!(cover.starts_with("cover condition: x < 0") && cover.ends_with("is UNSATISFIABLE"), "{cover}");
    }

    #[test]
    fn an_unsupported_construct_wins_over_the_undetermined_assertion() {
        let Class::Unsupported { construct } = captured("unsupported") else { panic!("{:?}", captured("unsupported")) };
        assert!(construct.starts_with("TerminatorKind::InlineAsm is not currently supported by Kani"), "{construct}");
    }

    #[test]
    fn tool_and_build_failures_are_their_own_classes() {
        assert_eq!(captured("harness_not_found"), Class::HarnessNotFound);
        assert_eq!(
            captured("preprocessor_failed"),
            Class::PreprocessorFailed {
                detail: "<home>/.alire/lib/gcc/aarch64-apple-darwin23.6.0/15.0.1/include/stdint.h:11:16: fatal error: stdint.h: No such file or directory".to_string()
            }
        );
        let stdout = read("compile_error.stdout.txt").unwrap();
        let stderr = read("compile_error.stderr.txt").unwrap();
        let Class::CompileError { detail } = classify(&stdout, &stderr, None, "verification::broken") else { panic!() };
        assert!(detail.contains("error[E0308]: mismatched types"), "{detail}");
    }

    #[test]
    fn timeout_and_out_of_memory_are_read_from_the_text_or_the_sarif() {
        let (stdout, stderr, sarif) = pass();
        let h = "verification::pass";
        let text = |m: &str| classify(&format!("{stdout}\n{m}\n"), &stderr, Some(&sarif), h);
        assert_eq!(text("CBMC timed out. You may want to rerun your proof with a larger timeout"), Class::Timeout);
        assert_eq!(text("CBMC appears to have run out of memory. You may want to rerun"), Class::OutOfMemory);
        let rule = |id: &str| sarif.replace(r#""results": []"#, &format!(r#""results": [{{"ruleId": "{id}"}}]"#));
        assert_eq!(classify(&stdout, &stderr, Some(&rule("kani.cbmc.timeout")), h), Class::Timeout);
        assert_eq!(classify(&stdout, &stderr, Some(&rule("kani.cbmc.oom")), h), Class::OutOfMemory);
    }

    #[test]
    fn a_successful_verdict_fails_closed() {
        let (stdout, stderr, sarif) = pass();
        let h = "verification::pass";
        let undetermined = stdout.replacen("Status: SUCCESS", "Status: UNDETERMINED", 1);
        assert!(
            matches!(classify(&undetermined, &stderr, Some(&sarif), h), Class::Undetermined { checks } if checks.len() == 1)
        );
        let unreachable_cover = stdout.replacen("Status: SATISFIED", "Status: UNREACHABLE", 1);
        assert!(matches!(classify(&unreachable_cover, &stderr, Some(&sarif), h), Class::VacuousCover { .. }));
        assert!(matches!(classify(&stdout, &stderr, None, h), Class::Unrecognised { .. }), "no SARIF");
        assert!(matches!(classify(&stdout, &stderr, Some("{"), h), Class::Unrecognised { .. }), "bad SARIF");
        assert!(matches!(classify(&stdout, &stderr, Some(&sarif), "verification::other"), Class::Unrecognised { .. }));
        let twice = format!("{stdout}\n{stdout}");
        assert!(matches!(classify(&twice, &stderr, Some(&sarif), h), Class::Unrecognised { .. }), "two runs");
        let failure = stdout.replacen("Status: SUCCESS", "Status: FAILURE", 1);
        assert!(matches!(classify(&failure, &stderr, Some(&sarif), h), Class::Unrecognised { .. }));
    }

    #[test]
    fn empty_or_truncated_output_is_unrecognised() {
        let (stdout, stderr, sarif) = pass();
        let h = "verification::pass";
        assert!(matches!(classify("", "", None, h), Class::Unrecognised { .. }));
        let cut = &stdout[..stdout.find("VERIFICATION:-").unwrap()];
        assert!(matches!(classify(cut, &stderr, Some(&sarif), h), Class::Unrecognised { .. }));
    }
}
