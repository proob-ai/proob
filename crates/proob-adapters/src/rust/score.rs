//! `score` for a Rust binding: `cargo mutants` over the property's scope
//! files with nextest as the test tool and the one-test filterset forwarded
//! after cargo-mutants' own `--`, so the bound test is the only test that can
//! kill a mutant. Counts come from `mutants.out/outcomes.json`; the baseline
//! must pass or nothing is scored.
//!
//! # Sample and jobs
//!
//! `cargo mutants --list --json` first lists every mutant the run's own
//! filters (`--workspace`, `--test-package`, `--file`, and the tree's
//! `.cargo/mutants.toml`) select. When more than `max_mutants` are listed,
//! `mutation::sample` chooses that many by name and the run gets one `--re`
//! per chosen name, the name escaped and anchored, so cargo-mutants runs
//! exactly those (its `--re` matches the `--list` names, line and column
//! included). `unsampled` is the listed count minus the run's
//! `total_mutants`, so a name listed twice is run twice and counted once
//! less as unsampled. Every count, and so the kill floor, is the sample's.
//! The run takes `--jobs` (`mutation::Limits::jobs_for` on the temporary
//! directory, where cargo-mutants copies and builds the tree once per job),
//! with a copy's size measured as `copy_bytes`: the tree as cargo-mutants
//! copies it (no `target/`, `.git`, `mutants.out*`) plus the root's
//! `target/debug`, the build a check of the same tree already left there.
//! With no `target/debug` there is nothing to measure the build by, and
//! `mutation::BYTES_PER_JOB` stands in.
use crate::argv::{Checked, Command};
use crate::mutation::sample;
use crate::rust::tools::{Need, Resolution, resolve};
use crate::rust::{CheckRef, Features, RustAdapter, parse_check_ref, proptest};
use crate::{AdapterError, Binding, Budget, Score, ScoreOutcome, Seed, ToolVersions, cap, exit_text};
use proob_facts::parsers::cargo_mutants::{Baseline, MutantsOutcome, parse_list, parse_outcomes};
use proob_spawn::Output;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A mutation run builds once per mutant; hours, not minutes.
pub const TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);
/// cargo-mutants' per-mutant test timeout, as a multiple of the baseline's.
pub const TIMEOUT_MULTIPLIER: &str = "5";
pub const OUTCOMES_FILE: &str = "mutants.out/outcomes.json";
/// Listing mutants parses the tree and builds nothing.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// cargo-mutants exit codes that still leave a complete `outcomes.json`.
const EXIT_SUCCESS: i32 = 0;
const EXIT_FOUND_PROBLEMS: i32 = 2;
const EXIT_TIMEOUT: i32 = 3;
const EXIT_BASELINE_FAILED: i32 = 4;

/// The options the listing and the run share, so both see the same mutants. Pure.
fn selecting(command: Command, r: &CheckRef, features: &Features, files: &[Checked]) -> Command {
    let command = features.apply(command.arg("mutants").arg("--workspace").opt("--test-package", &r.package));
    files.iter().fold(command, |c, file| c.opt("--file", file))
}

/// The argv that lists the scope's mutants. Pure.
pub fn list_plan(cargo: &Path, root: &Path, r: &CheckRef, features: &Features, files: &[Checked]) -> Command {
    selecting(Command::new(cargo, LIST_TIMEOUT), r, features, files)
        .arg("--list")
        .arg("--json")
        .cwd(root)
        .env("CARGO_TERM_COLOR", "never")
}

/// The argv for one score: `chosen` holds one `--re` per sampled mutant, or
/// nothing when every listed mutant runs. Pure.
#[allow(clippy::too_many_arguments)]
pub fn plan(
    cargo: &Path,
    root: &Path,
    r: &CheckRef,
    features: &Features,
    out_dir: &Path,
    files: &[Checked],
    chosen: &[Checked],
    jobs: u32,
    seed: Seed,
    budget: Budget,
) -> Command {
    let command = selecting(Command::new(cargo, TIMEOUT), r, features, files)
        .arg("--test-tool")
        .arg("nextest")
        .arg("--no-shuffle")
        .arg("--cap-lints")
        .arg("true")
        .arg("--timeout-multiplier")
        .arg(TIMEOUT_MULTIPLIER)
        .opt("--jobs", &Checked::number(u64::from(jobs)))
        .own("--output", out_dir);
    chosen
        .iter()
        .fold(command, |c, re| c.opt("--re", re))
        .tail_opt("-E", &Checked::filterset(&r.binary_id, &r.test_path))
        .cwd(root)
        .env("CARGO_TERM_COLOR", "never")
        .env(proptest::RNG_SEED, seed.0.to_string())
        .env(proptest::CASES, budget.0.to_string())
        .env(proptest::DISABLE_FAILURE_PERSISTENCE, "1")
}

/// Every mutant name the run's filters select, or why they could not be listed.
fn list(
    cargo: &Path,
    root: &Path,
    r: &CheckRef,
    features: &Features,
    files: &[Checked],
    tool_env: &[(OsString, OsString)],
) -> Result<Vec<String>, String> {
    let out = list_plan(cargo, root, r, features, files)
        .envs(tool_env)
        .run()
        .map_err(|e| format!("cargo mutants --list did not run: {e}"))?;
    if !out.success() {
        return Err(format!(
            "cargo mutants --list failed (exit {}); last output: {}",
            exit_text(out.code),
            cap(String::from_utf8_lossy(&out.stderr).trim(), 600)
        ));
    }
    parse_list(&String::from_utf8_lossy(&out.stdout))
        .map_err(|e| format!("cargo mutants --list --json could not be read: {e}"))
}

/// One `--re` per sampled name; none when the sample is everything.
pub fn selectors(listed: &[String], max_mutants: u32, seed: Seed) -> Vec<Checked> {
    let chosen = sample(listed, max_mutants, seed);
    if chosen.len() == listed.len() {
        Vec::new()
    } else {
        chosen.into_iter().map(|i| Checked::exact_regex(&listed[i])).collect()
    }
}

/// Scope files as root-relative checked paths; only Rust sources can be
/// mutated, and a file outside the adapter root cannot be reached.
pub fn mutable_files(adapter: &RustAdapter, scope_files: &[PathBuf]) -> Result<Vec<Checked>, Outside> {
    scope_files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "rs"))
        .map(|f| {
            let under_root = f.strip_prefix(adapter.root()).map_err(|_| Outside::Root(f.clone()))?;
            Checked::path(under_root).map_err(|rule| Outside::Grammar(f.clone(), rule))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outside {
    Root(PathBuf),
    Grammar(PathBuf, String),
}

pub fn run(
    adapter: &RustAdapter,
    repo: &Path,
    binding: &Binding,
    scope_files: &[PathBuf],
) -> Result<Score, AdapterError> {
    let r = parse_check_ref(&binding.check_ref)?;
    let root = adapter.root_in(repo);
    let tools = match resolve(&root, &[Need::Nextest, Need::Mutants], adapter.tool_env()) {
        Resolution::Ready(tools) => tools,
        Resolution::Missing { reason, versions } => return Ok(errored(reason, versions)),
    };
    let files = match mutable_files(adapter, scope_files) {
        Ok(files) => files,
        Err(Outside::Root(f)) => {
            let reason = format!(
                "scope file {} lies outside the adapter root `{}`; this adapter cannot mutate it",
                f.display(),
                adapter.root().display()
            );
            return Ok(errored(reason, tools.versions));
        }
        Err(Outside::Grammar(f, rule)) => return Err(AdapterError::Unsafe { value: f.display().to_string(), rule }),
    };
    if files.is_empty() {
        return Ok(Score { outcome: ScoreOutcome::NoMutants, tool_versions: tools.versions, jobs: None });
    }
    let listed = match list(&tools.cargo, &root, &r, adapter.features(), &files, adapter.tool_env()) {
        Ok(listed) => listed,
        Err(reason) => return Ok(errored(reason, tools.versions)),
    };
    if listed.is_empty() {
        return Ok(Score { outcome: ScoreOutcome::NoMutants, tool_versions: tools.versions, jobs: None });
    }
    let seed = Seed::for_scoring(binding.id);
    let chosen = selectors(&listed, adapter.limits().max_mutants, seed);
    let tmp = tempfile::tempdir().map_err(|e| AdapterError::Temp { message: e.to_string() })?;
    let jobs = adapter.limits().jobs_for(&std::env::temp_dir(), copy_bytes(&root));
    let command = plan(
        &tools.cargo,
        &root,
        &r,
        adapter.features(),
        tmp.path(),
        &files,
        &chosen,
        jobs.count(),
        seed,
        Budget::DEFAULT,
    )
    .envs(adapter.tool_env());
    let out = match command.run() {
        Ok(out) => out,
        Err(e) => return Ok(errored(format!("cargo-mutants did not run: {e}"), tools.versions)),
    };
    let outcomes = std::fs::read_to_string(tmp.path().join(OUTCOMES_FILE)).ok().map(|text| parse_outcomes(&text));
    let in_scope = u32::try_from(listed.len()).unwrap_or(u32::MAX);
    Ok(Score { outcome: outcome_of(&out, outcomes, in_scope), tool_versions: tools.versions, jobs: Some(jobs) })
}

/// Names cargo-mutants leaves out of a copy of the tree.
const NOT_COPIED: &[&str] = &["target", ".git", "mutants.out", "mutants.out.old"];

/// The bytes one cargo-mutants job takes: the tree it copies plus the
/// build it makes, read as the root's `target/debug`. `None` with no
/// `target/debug` to measure by.
// shortcut: the build is the existing `target/debug` (file lengths, symlinks not followed), which over-counts other test builds and misses a `CARGO_TARGET_DIR` elsewhere; ask cargo for the target dir if that misleads.
pub fn copy_bytes(root: &Path) -> Option<u64> {
    let build = root.join("target").join("debug");
    build.is_dir().then(|| tree_bytes(root, NOT_COPIED).saturating_add(tree_bytes(&build, &[])))
}

/// The summed file lengths under `dir`, skipping the top-level names in `skip`.
fn tree_bytes(dir: &Path, skip: &[&str]) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .filter(|e| !skip.iter().any(|s| e.file_name() == *s))
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => tree_bytes(&e.path(), &[]),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .fold(0, u64::saturating_add)
}

fn errored(reason: String, tool_versions: ToolVersions) -> Score {
    Score { outcome: ScoreOutcome::Errored { reason }, tool_versions, jobs: None }
}

/// Exit code and `outcomes.json` to an outcome, with `in_scope` mutants
/// listed before the run; anything incomplete is `Errored`.
pub fn outcome_of(
    out: &Output,
    outcomes: Option<Result<MutantsOutcome, proob_facts::error::ParseError>>,
    in_scope: u32,
) -> ScoreOutcome {
    let tail = || cap(String::from_utf8_lossy(&out.stderr).trim(), 600);
    match (out.code, outcomes) {
        (Some(EXIT_BASELINE_FAILED), _) => ScoreOutcome::Errored {
            reason: "baseline failed: the bound test does not pass before any mutation, so nothing can be scored"
                .into(),
        },
        (Some(EXIT_SUCCESS | EXIT_FOUND_PROBLEMS | EXIT_TIMEOUT), Some(Ok(o))) => counts(o, in_scope),
        (_, Some(Err(e))) => {
            ScoreOutcome::Errored { reason: format!("cargo-mutants' outcomes.json could not be read: {e}") }
        }
        (code, _) => ScoreOutcome::Errored {
            reason: format!("cargo-mutants failed (exit {}); last output: {}", exit_text(code), tail()),
        },
    }
}

fn counts(o: MutantsOutcome, in_scope: u32) -> ScoreOutcome {
    match (o.baseline, o.total_mutants) {
        (Baseline::Failed, _) => ScoreOutcome::Errored {
            reason: "baseline failed: the bound test does not pass before any mutation, so nothing can be scored"
                .into(),
        },
        (_, 0) => ScoreOutcome::NoMutants,
        _ => ScoreOutcome::Scored {
            killed: o.caught,
            survived: o.missed,
            timeout: o.timeout,
            unviable: o.unviable,
            unsampled: in_scope.saturating_sub(o.total_mutants),
        },
    }
}
