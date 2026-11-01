//! `score` for a TLA+ binding: TLC with only the bound property, on spec
//! mutants (`mutate`) in the check's temporary copy. The scope files are
//! code the model describes and never runs, so they are not mutated.
//!
//! Fates:
//!
//! - the bound property is violated: **killed**;
//! - TLC completes with no error: **survived**;
//! - the per-mutant limit, proob's memory budget, or a Java heap exhausted:
//!   **timeout**;
//! - a parse or semantic error, a deadlock (it says nothing about the
//!   property), or anything unrecognised: **unviable**, excluded from every
//!   denominator.
//!
//! The unmutated spec runs first (the baseline) and must report no
//! violation, or nothing is scored. Each mutant gets five times the
//! baseline's time, at least `MIN_MUTANT_TIMEOUT`. Mutants
//! beyond `max_mutants` are chosen by `mutation::sample` under the
//! property's scoring seed and the rest counted as unsampled.
use crate::mutation::sample;
use crate::tla::check::{Lab, judge, out_of_memory, versions_of};
use crate::tla::discover::cfg_of;
use crate::tla::mutate::{OPERATOR_SET, mutants};
use crate::tla::tools::{Resolution, Tools, resolve};
use crate::tla::{TlaAdapter, cfg, parse_check_ref};
use crate::verus::score::{BASELINE_TIMEOUT, TIMEOUT_MULTIPLIER};
use crate::{AdapterError, Binding, CheckOutcome, Score, ScoreOutcome, Seed, ToolVersions};
use proob_facts::parsers::tlc::{Class, classify};
use proob_spawn::{Output, SpawnError};
use std::path::Path;
use std::time::{Duration, Instant};

/// Recorded in `tool_versions` under `OPERATOR_SET_KEY`.
pub const OPERATOR_SET_KEY: &str = "spec-mutants";
/// The least a mutant gets: a JVM start and a parse, with room. A mutant
/// that unbounds the state space (`+ 1` to `- 1` on a counter) runs until
/// this, so it is far below the cargo adapters' minimum.
pub const MIN_MUTANT_TIMEOUT: Duration = Duration::from_secs(20);

/// What one mutant's TLC run means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    Killed,
    Survived,
    Timeout,
    Unviable,
}

/// Pure: the fate of a finished run's class and exit code, checking `name`.
pub fn fate_of(class: &Class, code: Option<i32>, name: &str) -> Fate {
    match class {
        Class::InvariantViolated { invariant, .. } if invariant == name && code != Some(0) => Fate::Killed,
        Class::TemporalViolated { .. } if code != Some(0) => Fate::Killed,
        Class::NoError if code == Some(0) => Fate::Survived,
        Class::OutOfMemory => Fate::Timeout,
        Class::NoError
        | Class::InvariantViolated { .. }
        | Class::TemporalViolated { .. }
        | Class::Deadlock { .. }
        | Class::ParseError { .. }
        | Class::SemanticError { .. }
        | Class::Unrecognised { .. } => Fate::Unviable,
    }
}

/// Pure: the fate of one run. A process that never started aborts the
/// score (`Err`): every later mutant would fail the same way.
pub fn fate(run: Result<&Output, &SpawnError>, name: &str) -> Result<Fate, String> {
    match run {
        Err(SpawnError::TimedOut { .. } | SpawnError::OutOfMemory { .. }) => Ok(Fate::Timeout),
        Err(e) => Err(e.to_string()),
        Ok(out) => {
            let class = classify(&String::from_utf8_lossy(&out.stdout), &String::from_utf8_lossy(&out.stderr));
            Ok(fate_of(&class, out.code, name))
        }
    }
}

/// Pure: the counts of a sample.
pub fn scored(fates: &[Fate], unsampled: u32) -> ScoreOutcome {
    let count = |f: Fate| fates.iter().filter(|x| **x == f).count() as u32;
    ScoreOutcome::Scored {
        killed: count(Fate::Killed),
        survived: count(Fate::Survived),
        timeout: count(Fate::Timeout),
        unviable: count(Fate::Unviable),
        unsampled,
    }
}

/// The names `mutate` starts from (`NEXT`, `SPECIFICATION`) and those it
/// leaves alone (`INIT`, else `Init`, and the bound name), from the cfg.
pub fn roots(cfg_text: &str, name: &str) -> (Vec<String>, Vec<String>) {
    let init = cfg::names(cfg_text, &["INIT"]);
    let init = if init.is_empty() { vec!["Init".to_string()] } else { init };
    (cfg::names(cfg_text, &["NEXT", "SPECIFICATION"]), init.into_iter().chain([name.to_string()]).collect())
}

pub fn run(adapter: &TlaAdapter, repo: &Path, binding: &Binding) -> Result<Score, AdapterError> {
    let r = parse_check_ref(&binding.check_ref)?;
    let operator_set = (OPERATOR_SET_KEY.to_string(), OPERATOR_SET.to_string());
    let tools = match resolve(adapter.tool_env()) {
        Resolution::Ready(tools) => tools,
        Resolution::Missing { reason, versions } => {
            return Ok(errored(reason, versions.into_iter().chain([operator_set]).collect()));
        }
    };
    let root = adapter.root_in(repo);
    let module = Path::new(r.module.as_str());
    let text = read(&root.join(module))?;
    let (from, excluded) = roots(&read(&root.join(cfg_of(module)))?, r.name.as_str());
    let all = mutants(&text, r.module.as_str(), &from, &excluded);
    let versions = |v: ToolVersions| v.into_iter().chain([operator_set.clone()]).collect::<ToolVersions>();
    if all.is_empty() {
        return Ok(Score { outcome: ScoreOutcome::NoMutants, tool_versions: versions(tools.versions), jobs: None });
    }
    let names: Vec<String> = all.iter().map(|m| m.name.clone()).collect();
    let chosen = sample(&names, adapter.max_mutants(), Seed::for_scoring(binding.id));
    let lab = Lab::new(adapter, repo, binding, &r)?;
    let (took, baseline_versions) = match baseline(adapter, &lab, &tools, r.name.as_str()) {
        Ok(ok) => ok,
        Err(reason) => return Ok(errored(reason, versions(tools.versions))),
    };
    let per_mutant = (took * TIMEOUT_MULTIPLIER).max(MIN_MUTANT_TIMEOUT);
    let io = |e: std::io::Error| AdapterError::Io { path: lab.spec.clone(), message: e.to_string() };
    let mut fates = Vec::with_capacity(chosen.len());
    for m in chosen.iter().map(|&i| &all[i]) {
        std::fs::write(&lab.spec, m.apply(&text)).map_err(io)?;
        match fate(lab.run(adapter, &tools, per_mutant).as_ref(), r.name.as_str()) {
            Ok(f) => fates.push(f),
            Err(e) => return Ok(errored(format!("mutant {} did not run: {e}", m.name), versions(baseline_versions))),
        }
    }
    let unsampled = u32::try_from(all.len() - chosen.len()).unwrap_or(u32::MAX);
    Ok(Score { outcome: scored(&fates, unsampled), tool_versions: versions(baseline_versions), jobs: None })
}

/// The unmutated spec must hold; how long it took and the versions it
/// printed, or why it did not.
fn baseline(adapter: &TlaAdapter, lab: &Lab, tools: &Tools, name: &str) -> Result<(Duration, ToolVersions), String> {
    let started = Instant::now();
    let why = match lab.run(adapter, tools, BASELINE_TIMEOUT) {
        Ok(out) => match judge(&out, name) {
            CheckOutcome::NoViolation => return Ok((started.elapsed(), versions_of(tools, &out))),
            CheckOutcome::Violation { trace } => format!("violation: {trace}"),
            CheckOutcome::Errored { reason } => reason,
            other => format!("{other:?}"),
        },
        Err(SpawnError::TimedOut { after, .. }) => format!("TLC timed out after {} minutes", after.as_secs() / 60),
        Err(SpawnError::OutOfMemory { limit_bytes, .. }) => out_of_memory(limit_bytes),
        Err(e) => format!("TLC did not run: {e}"),
    };
    Err(format!("baseline failed: the spec does not hold before any mutation: {why}"))
}

fn errored(reason: String, tool_versions: ToolVersions) -> Score {
    Score { outcome: ScoreOutcome::Errored { reason }, tool_versions, jobs: None }
}

fn read(path: &Path) -> Result<String, AdapterError> {
    std::fs::read_to_string(path).map_err(|e| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fates_by_class() {
        let violated = Class::InvariantViolated { invariant: "Inv".into(), trace: String::new() };
        assert_eq!(fate_of(&violated, Some(12), "Inv"), Fate::Killed);
        assert_eq!(fate_of(&violated, Some(12), "Other"), Fate::Unviable, "not the bound invariant");
        assert_eq!(fate_of(&Class::TemporalViolated { trace: String::new() }, Some(13), "P"), Fate::Killed);
        assert_eq!(fate_of(&Class::NoError, Some(0), "Inv"), Fate::Survived);
        assert_eq!(fate_of(&Class::NoError, Some(1), "Inv"), Fate::Unviable);
        assert_eq!(fate_of(&Class::Deadlock { trace: String::new() }, Some(11), "Inv"), Fate::Unviable);
        assert_eq!(fate_of(&Class::ParseError { detail: String::new() }, Some(150), "Inv"), Fate::Unviable);
        assert_eq!(fate_of(&Class::OutOfMemory, Some(1), "Inv"), Fate::Timeout);
        let timed_out = SpawnError::TimedOut { program: "java".into(), after: Duration::from_secs(1) };
        assert_eq!(fate(Err(&timed_out), "Inv"), Ok(Fate::Timeout));
        let oom = SpawnError::OutOfMemory { program: "java".into(), limit_bytes: 1, peak_bytes: 2 };
        assert_eq!(fate(Err(&oom), "Inv"), Ok(Fate::Timeout));
        assert!(fate(Err(&SpawnError::Start { program: "java".into(), message: "gone".into() }), "Inv").is_err());
    }

    #[test]
    fn roots_and_exclusions_come_from_the_cfg() {
        let (from, excluded) = roots("SPECIFICATION Spec\nINVARIANT Inv\n", "Inv");
        assert_eq!((from, excluded), (vec!["Spec".to_string()], vec!["Init".to_string(), "Inv".to_string()]));
        let (from, excluded) = roots("INIT Start\nNEXT Step\n", "Inv");
        assert_eq!((from, excluded), (vec!["Step".to_string()], vec!["Start".to_string(), "Inv".to_string()]));
    }
}
