//! `check` for a Rust binding: one `cargo nextest run` selecting exactly the
//! bound test, with the adapter's seed and budget in proptest's environment,
//! and a tool-specific nextest profile whose only setting is a JUnit report
//! at a temporary path. The repository's own `.config/nextest.toml` stays in
//! force underneath (it is a member of the binding set for that reason); the
//! tool config only adds the profile.
use crate::argv::{Checked, Command};
use crate::rust::tools::{Need, Resolution, resolve};
use crate::rust::{CheckRef, Features, RustAdapter, parse_check_ref, proptest};
use crate::{
    AdapterError, Binding, Budget, Check, CheckOutcome, Seed, ToolVersions, Witness, WitnessPolicy, cap, exit_text,
};
use proob_facts::parsers::junit::{CaseStatus, JUnitReport, parse_junit};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Compile plus run of one test, with room for a cold build.
pub const TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// The tool-specific profile name nextest requires of tool configs.
pub const PROFILE: &str = "@tool:proob:check";
pub const CONFIG_FILE: &str = "nextest.toml";
pub const JUNIT_FILE: &str = "junit.xml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub command: Command,
    pub junit: PathBuf,
}

/// The nextest tool config: one profile writing JUnit to `junit`.
pub fn config_text(junit: &Path) -> String {
    let mut junit_table = toml::Table::new();
    junit_table.insert("path".into(), toml::Value::String(junit.to_string_lossy().into_owned()));
    let mut profile = toml::Table::new();
    profile.insert("junit".into(), toml::Value::Table(junit_table));
    let mut profiles = toml::Table::new();
    profiles.insert(PROFILE.into(), toml::Value::Table(profile));
    let mut root = toml::Table::new();
    root.insert("profile".into(), toml::Value::Table(profiles));
    format!("# Written by proob for one `run check`; never read from the repository.\n{root}")
}

/// The argv for one check, given the temporary directory the config and the
/// report live in. Pure apart from writing that config file.
pub fn plan(
    cargo: &Path,
    root: &Path,
    r: &CheckRef,
    features: &Features,
    tmp: &Path,
    budget: Budget,
    seed: Seed,
) -> Result<Plan, AdapterError> {
    let junit = tmp.join(JUNIT_FILE);
    let config = tmp.join(CONFIG_FILE);
    std::fs::write(&config, config_text(&junit))
        .map_err(|e| AdapterError::Io { path: config.clone(), message: e.to_string() })?;
    let tool_config = format!("proob:{}", config.display());
    let command = features
        .apply(Command::new(cargo, TIMEOUT).arg("nextest").arg("run"))
        .opt("--package", &r.package)
        .own("--tool-config-file", Path::new(&tool_config))
        .arg("--profile")
        .arg(PROFILE)
        .arg("--no-fail-fast")
        .opt("--filterset", &Checked::filterset(&r.binary_id, &r.test_path))
        .cwd(root)
        .env("CARGO_TERM_COLOR", "never")
        .env(proptest::RNG_SEED, seed.0.to_string())
        .env(proptest::CASES, budget.0.to_string())
        .env(proptest::DISABLE_FAILURE_PERSISTENCE, "1");
    Ok(Plan { command, junit })
}

pub fn run(
    adapter: &RustAdapter,
    repo: &Path,
    binding: &Binding,
    budget: Budget,
    seed: Seed,
) -> Result<Check, AdapterError> {
    let r = parse_check_ref(&binding.check_ref)?;
    let root = adapter.root_in(repo);
    let tools = match resolve(&root, &[Need::Nextest], adapter.tool_env()) {
        Resolution::Ready(tools) => tools,
        Resolution::Missing { reason, versions } => return Ok(errored(reason, versions)),
    };
    let tmp = tempfile::tempdir().map_err(|e| AdapterError::Temp { message: e.to_string() })?;
    let plan = plan(&tools.cargo, &root, &r, adapter.features(), tmp.path(), budget, seed)?;
    let out = match plan.command.clone().envs(adapter.tool_env()).run() {
        Ok(out) => out,
        Err(e) => return Ok(errored(format!("nextest did not run: {e}"), tools.versions)),
    };
    let report = match std::fs::read_to_string(&plan.junit) {
        Ok(text) => text,
        Err(_) => {
            let tail = cap(String::from_utf8_lossy(&out.stderr).trim(), 600);
            return Ok(errored(
                format!("nextest produced no report (exit {}); last output: {tail}", exit_text(out.code)),
                tools.versions,
            ));
        }
    };
    let outcome = match parse_junit(&report) {
        Ok(parsed) => outcome_of(&parsed, &r, budget, seed, adapter.witness()),
        Err(e) => CheckOutcome::Errored { reason: format!("nextest's JUnit report could not be read: {e}") },
    };
    Ok(Check { outcome, tool_versions: tools.versions })
}

fn errored(reason: String, tool_versions: ToolVersions) -> Check {
    Check { outcome: CheckOutcome::Errored { reason }, tool_versions }
}

/// The bound test's case in the report, mapped to an outcome.
pub fn outcome_of(
    report: &JUnitReport,
    r: &CheckRef,
    budget: Budget,
    seed: Seed,
    policy: WitnessPolicy,
) -> CheckOutcome {
    match report.case(r.binary_id.as_str(), r.test_path.as_str()).map(|c| &c.status) {
        Some(CaseStatus::Passed) => CheckOutcome::NoCounterexample { budget, replay: seed },
        Some(CaseStatus::Failed { message, body }) => CheckOutcome::Counterexample {
            witness: Witness::new(policy, &proptest::witness_text(message, body)),
            replay: seed,
        },
        Some(CaseStatus::Skipped) => {
            CheckOutcome::Errored { reason: format!("bound test `{}` was skipped by nextest", r.test_path.as_str()) }
        }
        None => CheckOutcome::Errored {
            reason: format!(
                "bound test not found: nextest ran no test `{}` in `{}`; was it renamed or removed?",
                r.test_path.as_str(),
                r.binary_id.as_str()
            ),
        },
    }
}
