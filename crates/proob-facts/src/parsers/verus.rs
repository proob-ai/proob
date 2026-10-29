//! Verus's `--output-json`, as `cargo verus verify` prints it, and the
//! rustc-style diagnostics on stderr that locate a failure.
//!
//! stdout is not pure JSON: every verified dependency crate prints a plain
//! `verification results:: N verified, M errors` line first, and the object
//! for the root crate starts at the first line that is exactly `{`
//! (`tests/fixtures/verus/README.md`). Its `verification-results` carries
//! `encountered-error`, `encountered-vir-error`, and, unless a VIR error
//! stopped verification before the SMT ran, the `verified` and `errors`
//! counts; a whole-crate run adds `success`, a partial one (`--verify-root`,
//! `--verify-only-module`) leaves it out, so success is read as "no error
//! encountered and no error counted" when the key is absent. `verus` carries
//! the version, commit, toolchain, and platform. A failing function's `func-details` entry carries no note, so
//! the failing location is read from stderr: an `error: <message>` line
//! followed by ` --> <file>:<line>:<column>`. The same `verus` object is what
//! `verus --version --output-json` prints on its own.
use crate::error::ParseError;
use serde::Deserialize;

/// The verifier's verdict on the root crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Results {
    /// `success: true`: every function in scope verified.
    Success { verified: u32, errors: u32 },
    /// The SMT ran and at least one function failed (`encountered-error`
    /// with `errors` counted).
    Failed { verified: u32, errors: u32 },
    /// `encountered-error` with no proof error counted: rustc rejected the
    /// crate (its diagnostics are on stderr) and no obligation was judged.
    Aborted { verified: u32 },
    /// Verification never reached the SMT: a mode error, a type error inside
    /// ghost code, or a construct `--no-cheating` forbids. No counts exist.
    VirError,
}

/// What `verus` says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerusInfo {
    pub version: String,
    pub commit: Option<String>,
    pub toolchain: Option<String>,
    /// `<os>-<arch>`, when reported.
    pub platform: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub results: Results,
    pub verus: VerusInfo,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "verification-results")]
    results: RawResults,
    verus: RawVerus,
}

#[derive(Deserialize)]
struct RawResults {
    #[serde(default)]
    success: Option<bool>,
    #[serde(rename = "encountered-error", default)]
    encountered_error: bool,
    #[serde(rename = "encountered-vir-error", default)]
    vir_error: bool,
    #[serde(default)]
    verified: Option<u32>,
    #[serde(default)]
    errors: Option<u32>,
}

#[derive(Deserialize)]
struct VersionOnly {
    verus: RawVerus,
}

#[derive(Deserialize)]
struct RawVerus {
    version: String,
    #[serde(default)]
    commit: Option<String>,
    #[serde(default)]
    toolchain: Option<String>,
    #[serde(default)]
    platform: Option<RawPlatform>,
}

#[derive(Deserialize)]
struct RawPlatform {
    os: String,
    arch: String,
}

impl From<RawVerus> for VerusInfo {
    fn from(v: RawVerus) -> Self {
        VerusInfo {
            version: v.version,
            commit: v.commit,
            toolchain: v.toolchain,
            platform: v.platform.map(|p| format!("{}-{}", p.os, p.arch)),
        }
    }
}

/// The JSON object in a `cargo verus verify --output-json` stdout: from the
/// first line that is exactly `{` to the end. `None` when no object was
/// printed (rustc failed before Verus ran, or the flag was not forwarded).
pub fn json_object(stdout: &str) -> Option<&str> {
    stdout
        .lines()
        .scan(0usize, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some((start, line))
        })
        .find(|(_, line)| line.trim_end_matches('\r') == "{")
        .map(|(start, _)| stdout[start..].trim_end())
}

pub fn parse_output(stdout: &str) -> Result<Report, ParseError> {
    let object = json_object(stdout)
        .ok_or_else(|| ParseError::Schema("verus printed no JSON object (no line that is exactly `{`)".into()))?;
    let raw: Raw = serde_json::from_str(object).map_err(|e| ParseError::Schema(format!("verus --output-json: {e}")))?;
    Ok(Report { results: results_of(&raw.results), verus: raw.verus.into() })
}

fn results_of(r: &RawResults) -> Results {
    let (verified, errors) = (r.verified.unwrap_or(0), r.errors.unwrap_or(0));
    let success = r.success.unwrap_or(!r.encountered_error && errors == 0);
    match (r.vir_error, success, errors) {
        (true, _, _) => Results::VirError,
        (false, true, _) => Results::Success { verified, errors },
        (false, false, 0) => Results::Aborted { verified },
        (false, false, _) => Results::Failed { verified, errors },
    }
}

/// `verus --version --output-json`.
pub fn parse_version(json: &str) -> Result<VerusInfo, ParseError> {
    let raw: VersionOnly = serde_json::from_str(json.trim())
        .map_err(|e| ParseError::Schema(format!("verus --version --output-json: {e}")))?;
    Ok(raw.verus.into())
}

/// One rustc-style error from stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    /// `file:line:column` from the ` --> ` line that follows, when there is one.
    pub location: Option<String>,
}

/// Every `error…: <message>` on stderr with its location, except the
/// trailing summaries (`could not compile`, `aborting due to`).
pub fn diagnostics(stderr: &str) -> Vec<Diagnostic> {
    let lines: Vec<&str> = stderr.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| error_message(line).map(|m| (i, m)))
        .filter(|(_, m)| !m.starts_with("could not compile") && !m.starts_with("aborting due to"))
        .map(|(i, message)| Diagnostic { message, location: location_after(&lines[i + 1..]) })
        .collect()
}

fn error_message(line: &str) -> Option<String> {
    let rest = line.strip_prefix("error")?;
    let rest = rest.strip_prefix('[').map_or(rest, |r| r.split_once(']').map_or(r, |(_, after)| after));
    rest.strip_prefix(':').map(|m| m.trim().to_string())
}

fn location_after(following: &[&str]) -> Option<String> {
    following
        .iter()
        .take_while(|l| !l.starts_with("error"))
        .find_map(|l| l.trim_start().strip_prefix("--> ").map(|loc| loc.trim().to_string()))
}

/// Diagnostics as one line: `assertion failed (src/lib.rs:7:12); …`.
pub fn summarise(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|d| match &d.location {
            Some(loc) => format!("{} ({loc})", d.message),
            None => d.message.clone(),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/verus").join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn the_object_is_cut_after_the_dependency_lines() {
        let out = fixture("pass.cargo-verus.stdout.txt");
        assert!(out.starts_with("verification results::"));
        let report = parse_output(&out).expect("parses");
        assert_eq!(report.results, Results::Success { verified: 11, errors: 0 });
        assert_eq!(report.verus.version, "0.2026.10.26.d17fbd0");
        assert_eq!(report.verus.commit.as_deref(), Some("d17fbd0c58abf0d5e86825ea64b96aba72b1b31f"));
        assert_eq!(report.verus.platform.as_deref(), Some("macos-aarch64"));
        assert!(report.verus.toolchain.as_deref().is_some_and(|t| t.starts_with("1.99.1")));
    }

    #[test]
    fn a_failed_assertion_and_a_no_cheating_rejection_are_distinct() {
        let failed = parse_output(&fixture("fail-assert.json")).expect("parses");
        assert_eq!(failed.results, Results::Failed { verified: 10, errors: 1 });
        let vir = parse_output(&fixture("fail-assume-no-cheating.json")).expect("parses");
        assert_eq!(vir.results, Results::VirError);
    }

    #[test]
    fn a_rustc_error_is_aborted_not_a_failed_proof() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/verus-crate/captures/error.stdout.txt");
        let out = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(parse_output(&out).expect("parses").results, Results::Aborted { verified: 0 });
    }

    #[test]
    fn a_partial_run_has_no_success_key() {
        // Verus pretty-prints, so the object's `{` is a line of its own.
        let partial = "{\n\"func-details\":{},\"verification-results\":{\"encountered-error\":false,\"encountered-vir-error\":false,\"verified\":1,\"errors\":0,\"is-verifying-entire-crate\":false},\"verus\":{\"version\":\"v\"}}";
        assert_eq!(parse_output(partial).expect("parses").results, Results::Success { verified: 1, errors: 0 });
        let failed = "{\n\"verification-results\":{\"encountered-error\":true,\"encountered-vir-error\":false,\"verified\":0,\"errors\":1,\"is-verifying-entire-crate\":false},\"verus\":{\"version\":\"v\"}}";
        assert_eq!(parse_output(failed).expect("parses").results, Results::Failed { verified: 0, errors: 1 });
    }

    #[test]
    fn no_object_is_an_error_not_a_verdict() {
        assert!(parse_output("    Checking x\nerror[E0308]: mismatched types\n").is_err());
        assert!(parse_output("").is_err());
        assert!(json_object("a\n{\n}\n").is_some());
        assert!(json_object("a {\n}").is_none(), "the line must be exactly `{{`");
    }

    #[test]
    fn diagnostics_pair_messages_with_locations() {
        let d = diagnostics(&fixture("fail-assert.stderr.txt"));
        assert_eq!(
            d,
            vec![Diagnostic {
                message: "assertion failed".into(),
                location: Some("crates/proob-core/src/scope_hash.rs:174:12".into())
            }]
        );
        assert_eq!(summarise(&d), "assertion failed (crates/proob-core/src/scope_hash.rs:174:12)");
        let d = diagnostics(&fixture("fail-assume-no-cheating.stderr.txt"));
        assert_eq!(d[0].message, "`assume`/`admit` not allowed with `--no-cheating`");
        let rustc = diagnostics(
            "error[E0308]: mismatched types\n  --> src/lib.rs:9:17\n   |\nerror: aborting due to 1 previous error\n",
        );
        assert_eq!(
            rustc,
            vec![Diagnostic { message: "mismatched types".into(), location: Some("src/lib.rs:9:17".into()) }]
        );
    }

    #[test]
    fn the_version_object_parses_on_its_own() {
        let v = parse_version(r#"{"verus":{"profile":"release","version":"0.2026.10.26.d17fbd0","platform":{"os":"linux","arch":"x86_64"},"toolchain":"1.99.1","commit":"abc"}}"#).expect("parses");
        assert_eq!(
            (v.version.as_str(), v.commit.as_deref(), v.platform.as_deref()),
            ("0.2026.10.26.d17fbd0", Some("abc"), Some("linux-x86_64"))
        );
    }
}
