//! The `.meta` sidecar mutmut 3 writes beside each module it mutates, at
//! `mutants/<path>.meta` for the source file `<path>`. Its `exit_code_by_key`
//! maps every mutant of the module (`<module>.x_<function>__mutmut_<n>`, or
//! `<module>.xǁ<Class>ǁ<method>__mutmut_<n>` for a method) to the exit code
//! of the test run against it, `null` while unrun; the other keys (function
//! hashes, durations, type-check errors) are mutmut's cache and are not
//! read. What an exit code means is mutmut 3.8.0's own `status_by_exit_code`
//! table, copied into `status` and pinned with the version in
//! `tools/python-tools.txt`; a mutmut that changed it would be refused by
//! the adapter before any `.meta` is read.
use crate::error::ParseError;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaFile {
    pub exit_code_by_key: BTreeMap<String, Option<i64>>,
}

/// What mutmut 3.8.0 calls each exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutantStatus {
    Killed,
    Survived,
    /// No selected test ran the mutated function; mutmut never ran the mutant.
    NoTests,
    Skipped,
    Suspicious,
    Timeout,
    CaughtByTypeCheck,
    Interrupted,
    /// `null`: the mutant was generated and never tested.
    NotChecked,
    Segfault,
}

/// mutmut 3.8.0's `status_by_exit_code`, including its default arm.
pub fn status(exit_code: Option<i64>) -> MutantStatus {
    match exit_code {
        None => MutantStatus::NotChecked,
        Some(1 | 3) => MutantStatus::Killed,
        Some(0) => MutantStatus::Survived,
        Some(5 | 33) => MutantStatus::NoTests,
        Some(2) => MutantStatus::Interrupted,
        Some(34) => MutantStatus::Skipped,
        Some(36 | 24 | -24 | 152 | 255) => MutantStatus::Timeout,
        Some(37) => MutantStatus::CaughtByTypeCheck,
        Some(-11 | -9) => MutantStatus::Segfault,
        Some(_) => MutantStatus::Suspicious,
    }
}

/// How many mutants of a module ended in each status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MutmutCounts {
    pub killed: u32,
    pub survived: u32,
    pub no_tests: u32,
    pub skipped: u32,
    pub suspicious: u32,
    pub timeout: u32,
    pub caught_by_type_check: u32,
    pub interrupted: u32,
    pub not_checked: u32,
    pub segfault: u32,
}

impl MutmutCounts {
    pub fn total(&self) -> u32 {
        self.killed
            + self.survived
            + self.no_tests
            + self.skipped
            + self.suspicious
            + self.timeout
            + self.caught_by_type_check
            + self.interrupted
            + self.not_checked
            + self.segfault
    }

    /// The sum of two modules' counts.
    pub fn plus(self, o: MutmutCounts) -> MutmutCounts {
        MutmutCounts {
            killed: self.killed + o.killed,
            survived: self.survived + o.survived,
            no_tests: self.no_tests + o.no_tests,
            skipped: self.skipped + o.skipped,
            suspicious: self.suspicious + o.suspicious,
            timeout: self.timeout + o.timeout,
            caught_by_type_check: self.caught_by_type_check + o.caught_by_type_check,
            interrupted: self.interrupted + o.interrupted,
            not_checked: self.not_checked + o.not_checked,
            segfault: self.segfault + o.segfault,
        }
    }

    fn with(self, s: MutantStatus) -> MutmutCounts {
        match s {
            MutantStatus::Killed => MutmutCounts { killed: self.killed + 1, ..self },
            MutantStatus::Survived => MutmutCounts { survived: self.survived + 1, ..self },
            MutantStatus::NoTests => MutmutCounts { no_tests: self.no_tests + 1, ..self },
            MutantStatus::Skipped => MutmutCounts { skipped: self.skipped + 1, ..self },
            MutantStatus::Suspicious => MutmutCounts { suspicious: self.suspicious + 1, ..self },
            MutantStatus::Timeout => MutmutCounts { timeout: self.timeout + 1, ..self },
            MutantStatus::CaughtByTypeCheck => {
                MutmutCounts { caught_by_type_check: self.caught_by_type_check + 1, ..self }
            }
            MutantStatus::Interrupted => MutmutCounts { interrupted: self.interrupted + 1, ..self },
            MutantStatus::NotChecked => MutmutCounts { not_checked: self.not_checked + 1, ..self },
            MutantStatus::Segfault => MutmutCounts { segfault: self.segfault + 1, ..self },
        }
    }
}

#[derive(Deserialize)]
struct Raw {
    exit_code_by_key: BTreeMap<String, Option<i64>>,
}

pub fn parse_meta(json: &str) -> Result<MetaFile, ParseError> {
    let raw: Raw = serde_json::from_str(json).map_err(|e| ParseError::Schema(format!("mutmut .meta: {e}")))?;
    Ok(MetaFile { exit_code_by_key: raw.exit_code_by_key })
}

/// The module's mutants by status.
pub fn counts(meta: &MetaFile) -> MutmutCounts {
    meta.exit_code_by_key.values().fold(MutmutCounts::default(), |c, code| c.with(status(*code)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_exit_codes_and_counts_them_by_mutmuts_table() {
        let json = r#"{"exit_code_by_key": {"m.x_f__mutmut_1": 1, "m.x_f__mutmut_2": 0, "m.x_f__mutmut_3": 33, "m.x_f__mutmut_4": null,
            "m.xǁCǁg__mutmut_1": 36, "m.xǁCǁg__mutmut_2": 34, "m.xǁCǁg__mutmut_3": 99, "m.xǁCǁg__mutmut_4": 37, "m.xǁCǁg__mutmut_5": -9, "m.xǁCǁg__mutmut_6": 2},
            "hash_by_function_name": {"x_f": "abc"}, "type_check_error_by_key": {}, "durations_by_key": {}, "estimated_durations_by_key": {}}"#;
        let meta = parse_meta(json).expect("parses");
        let c = counts(&meta);
        assert_eq!(c.killed, 1);
        assert_eq!(c.survived, 1);
        assert_eq!(c.no_tests, 1);
        assert_eq!(c.not_checked, 1);
        assert_eq!(c.timeout, 1);
        assert_eq!(c.skipped, 1);
        assert_eq!(c.suspicious, 1, "an exit code outside the table is suspicious");
        assert_eq!(c.caught_by_type_check, 1);
        assert_eq!(c.segfault, 1);
        assert_eq!(c.interrupted, 1);
        assert_eq!(c.total(), 10);
        assert_eq!(c.plus(c).total(), 20);
    }

    #[test]
    fn every_timeout_and_kill_code_is_named() {
        for code in [36, 24, -24, 152, 255] {
            assert_eq!(status(Some(code)), MutantStatus::Timeout, "{code}");
        }
        assert_eq!(status(Some(3)), MutantStatus::Killed, "a pytest internal error kills");
        assert_eq!(status(Some(5)), MutantStatus::NoTests, "pytest's own no-tests exit");
        assert_eq!(status(None), MutantStatus::NotChecked);
    }

    #[test]
    fn refuses_a_file_without_the_results_map() {
        assert!(parse_meta(r#"{"hash_by_function_name": {}}"#).is_err());
        assert!(parse_meta("not json").is_err());
        assert_eq!(counts(&parse_meta(r#"{"exit_code_by_key": {}}"#).expect("parses")).total(), 0);
    }
}
