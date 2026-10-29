//! The JUnit XML report cargo-nextest writes for a profile with `junit.path`
//! set: `testsuites > testsuite > testcase`, where a test case that failed
//! carries a `failure` (or `error`) child whose `message` attribute is the
//! short reason and whose body is the captured output, and a skipped one
//! carries `skipped`. nextest names each case by its test path (`name`) and
//! its binary id (`classname`).
use crate::error::ParseError;
use crate::parsers::xml::{self, Element};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JUnitReport {
    pub cases: Vec<TestCase>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    /// nextest: the binary id, such as `pkg::props`.
    pub classname: String,
    /// nextest: the test path inside the binary, such as `m::my_test`.
    pub name: String,
    pub status: CaseStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseStatus {
    Passed,
    /// The test ran and failed (`failure`) or could not complete (`error`).
    Failed {
        message: String,
        body: String,
    },
    Skipped,
}

impl JUnitReport {
    /// The case for one binary id and test path, if the report has it.
    pub fn case(&self, classname: &str, name: &str) -> Option<&TestCase> {
        self.cases.iter().find(|c| c.classname == classname && c.name == name)
    }
}

pub fn parse_junit(text: &str) -> Result<JUnitReport, ParseError> {
    let root = xml::parse(text)?;
    if root.name != "testsuites" && root.name != "testsuite" {
        return Err(ParseError::Schema(format!("junit: root element is `{}`, not `testsuites`", root.name)));
    }
    let cases = root.descendants_named("testcase").into_iter().map(case_of).collect::<Result<Vec<_>, _>>()?;
    Ok(JUnitReport { cases })
}

fn case_of(e: &Element) -> Result<TestCase, ParseError> {
    let name = e.attr("name").ok_or_else(|| ParseError::Schema("junit: testcase without a name".into()))?;
    let classname = e.attr("classname").unwrap_or_default();
    Ok(TestCase { classname: classname.to_string(), name: name.to_string(), status: status_of(e) })
}

fn status_of(e: &Element) -> CaseStatus {
    let failed = e.children_named("failure").chain(e.children_named("error")).next();
    match failed {
        Some(f) => CaseStatus::Failed { message: f.attr("message").unwrap_or_default().to_string(), body: f.text() },
        None if e.children_named("skipped").next().is_some() => CaseStatus::Skipped,
        None => CaseStatus::Passed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="2" failures="1" errors="0">
  <testsuite name="pkg::props" tests="2" failures="1">
    <testcase name="good" classname="pkg::props" time="0.01"/>
    <testcase name="bad" classname="pkg::props" time="0.01">
      <failure message="thread &apos;bad&apos; panicked" type="test failure">Test failed: x.
minimal failing input: x = 1</failure>
      <system-out>out</system-out>
    </testcase>
    <testcase name="skipped" classname="pkg::other"><skipped/></testcase>
  </testsuite>
</testsuites>"#;

    #[test]
    fn reads_passed_failed_and_skipped_cases() {
        let r = parse_junit(REPORT).expect("parses");
        assert_eq!(r.cases.len(), 3);
        assert_eq!(r.case("pkg::props", "good").map(|c| &c.status), Some(&CaseStatus::Passed));
        match &r.case("pkg::props", "bad").expect("bad").status {
            CaseStatus::Failed { message, body } => {
                assert_eq!(message, "thread 'bad' panicked");
                assert!(body.contains("minimal failing input: x = 1"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(r.case("pkg::other", "skipped").map(|c| &c.status), Some(&CaseStatus::Skipped));
        assert!(r.case("pkg::props", "missing").is_none());
    }

    #[test]
    fn refuses_a_non_junit_document() {
        assert!(parse_junit("<report/>").is_err());
        assert!(parse_junit("not xml").is_err());
    }
}
