//! `score` for a Verus binding: mutation-under-proof. The bound item's
//! executable body is mutated one operator at a time (`mutate`), each mutant
//! is written into a temporary copy of the adapter root and verified there
//! exactly as `check` attributes the item (`cargo verus focus` with
//! `--verify-function`, after cleaning the package), and the verdicts are
//! counted:
//!
//! - the verifier rejects the mutant: **killed** (the specification can fail);
//! - the verifier accepts it: **survived** (the specification cannot tell);
//! - rustc or Verus cannot check it at all (no JSON, or a VIR error): **unviable**,
//!   excluded from every denominator;
//! - the resource limit is exhausted, or the process times out or passes
//!   proob's memory budget: **timeout**.
//!
//! The unmutated copy is verified first (the baseline); if it does not
//! verify, nothing is scored. Mutants beyond `max_mutants` are counted as
//! unsampled and never run. The scope files play no part: the unit measured
//! is the bound item's specification against its own implementation, and the
//! copy shares the real tree's build directory so `vstd` is built once.
//!
//! Only an exec `fn` has an implementation to mutate: a binding to a
//! `proof fn` or `spec fn` scores `NoMutants`, which never satisfies the
//! kill floor; bind the exec function whose `ensures` states the property.
use crate::verus::check::{Judged, clean, judge, lock, partial_dir, plan_item, target_dir};
use crate::verus::mutate::{Mutant, OPERATOR_SET, apply, mutants};
use crate::verus::scan::{Bound, Mode, scan};
use crate::verus::tools::{Resolution, Toolchain, resolve};
use crate::verus::{CheckRef, VerusAdapter, parse_check_ref};
use crate::{AdapterError, Binding, Score, ScoreOutcome, ToolVersions};
use proob_spawn::{Output, SpawnError};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The baseline may build the crate cold.
pub const BASELINE_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// Each mutant gets this multiple of the baseline's time, at least the floor.
pub const TIMEOUT_MULTIPLIER: u32 = 5;
pub const MIN_MUTANT_TIMEOUT: Duration = Duration::from_secs(120);
const RLIMIT_MARK: &str = "rlimit";

/// What one mutant's verifier run means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    Killed,
    Survived,
    Timeout,
    Unviable,
}

/// Pure: the fate of a mutant from its run. A process that never started is
/// not a fate but an abort (`Err`), since every later mutant would fail the
/// same way and the score would mean nothing.
pub fn fate(run: Result<&Output, &SpawnError>) -> Result<Fate, String> {
    match run {
        Err(SpawnError::TimedOut { .. } | SpawnError::OutOfMemory { .. }) => Ok(Fate::Timeout),
        Err(e) => Err(e.to_string()),
        Ok(out) => Ok(match judge(out) {
            Judged::Verified => Fate::Survived,
            Judged::Rejected(diagnostics) if diagnostics.contains(RLIMIT_MARK) => Fate::Timeout,
            Judged::Rejected(_) => Fate::Killed,
            Judged::Aborted(_) | Judged::VirError(_) | Judged::ItemNotFound | Judged::NoReport(_) => Fate::Unviable,
        }),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub killed: u32,
    pub survived: u32,
    pub timeout: u32,
    pub unviable: u32,
}

pub fn tally(fates: impl IntoIterator<Item = Fate>) -> Tally {
    fates.into_iter().fold(Tally::default(), |t, f| match f {
        Fate::Killed => Tally { killed: t.killed + 1, ..t },
        Fate::Survived => Tally { survived: t.survived + 1, ..t },
        Fate::Timeout => Tally { timeout: t.timeout + 1, ..t },
        Fate::Unviable => Tally { unviable: t.unviable + 1, ..t },
    })
}

pub fn run(adapter: &VerusAdapter, repo: &Path, binding: &Binding) -> Result<Score, AdapterError> {
    let r = parse_check_ref(&binding.check_ref)?;
    let root = adapter.root_in(repo);
    let tc = match resolve(adapter.tool_env()) {
        Resolution::Ready(tc) => tc,
        Resolution::Missing { reason, versions } => return Ok(errored(reason, versions)),
    };
    let versions: ToolVersions =
        tc.versions.iter().cloned().chain([("mutants-under-proof".to_string(), OPERATOR_SET.to_string())]).collect();
    let source = repo.join(&binding.token.path);
    let text = read(&source)?;
    let item = match bound_item(&text, binding.token.line) {
        Some(item) => item,
        None => {
            return Ok(errored(
                format!("the token at {} no longer sits above a verus! item; run `proob validate`", binding.token),
                versions,
            ));
        }
    };
    if item.mode != Mode::Exec {
        return Ok(Score { outcome: ScoreOutcome::NoMutants, tool_versions: versions, jobs: None });
    }
    let all = mutants(&text, item.line);
    if all.is_empty() {
        return Ok(Score { outcome: ScoreOutcome::NoMutants, tool_versions: versions, jobs: None });
    }
    let (sample, rest) = all.split_at(all.len().min(adapter.max_mutants() as usize));
    let under_root = binding.token.path.strip_prefix(adapter.root()).unwrap_or(&binding.token.path).to_path_buf();
    let lab = Lab::new(adapter, &tc, &root, &r)?;
    let baseline = match lab.verify(&under_root, &text, BASELINE_TIMEOUT) {
        Ok((Judged::Verified, took)) => took,
        Ok((judged, _)) => {
            return Ok(errored(
                format!("baseline failed: the bound item does not verify before any mutation ({judged:?})"),
                versions,
            ));
        }
        Err(e) => return Ok(errored(format!("baseline did not run: {e}"), versions)),
    };
    let per_mutant = (baseline * TIMEOUT_MULTIPLIER).max(MIN_MUTANT_TIMEOUT);
    let mut fates = Vec::with_capacity(sample.len());
    for m in sample {
        match lab.fate_of(&under_root, &text, m, per_mutant) {
            Ok(f) => fates.push(f),
            Err(e) => return Ok(errored(format!("mutant {} did not run: {e}", m.describe()), versions)),
        }
    }
    let t = tally(fates);
    let outcome = ScoreOutcome::Scored {
        killed: t.killed,
        survived: t.survived,
        timeout: t.timeout,
        unviable: t.unviable,
        unsampled: rest.len() as u32,
    };
    Ok(Score { outcome, tool_versions: versions, jobs: None })
}

/// The item the token at `line` binds, re-read from the file as it is now.
fn bound_item(text: &str, line: usize) -> Option<crate::verus::scan::Item> {
    scan(text).tokens.into_iter().find(|t| t.line == line).and_then(|t| match t.bound {
        Bound::Item { item, .. } => Some(item),
        Bound::Problem(_) => None,
    })
}

fn errored(reason: String, tool_versions: ToolVersions) -> Score {
    Score { outcome: ScoreOutcome::Errored { reason }, tool_versions, jobs: None }
}

fn read(path: &Path) -> Result<String, AdapterError> {
    std::fs::read_to_string(path).map_err(|e| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() })
}

/// A temporary copy of the adapter root where mutants are verified.
struct Lab<'a> {
    copy: tempfile::TempDir,
    tc: &'a Toolchain,
    target: PathBuf,
    r: &'a CheckRef,
    env: Vec<(OsString, OsString)>,
    rlimit: u32,
}

impl<'a> Lab<'a> {
    fn new(adapter: &VerusAdapter, tc: &'a Toolchain, root: &Path, r: &'a CheckRef) -> Result<Lab<'a>, AdapterError> {
        let copy = tempfile::tempdir().map_err(|e| AdapterError::Temp { message: e.to_string() })?;
        copy_tree(root, copy.path())?;
        let env = tc.env(adapter.tool_env());
        Ok(Lab { copy, tc, target: target_dir(root, adapter.tool_env())?, r, env, rlimit: adapter.rlimit() })
    }

    /// Writes `text` at `under_root` in the copy and verifies the item there.
    fn verify(&self, under_root: &Path, text: &str, timeout: Duration) -> Result<(Judged, Duration), String> {
        let path = self.copy.path().join(under_root);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        let _held = lock(&self.target).map_err(|e| e.to_string())?;
        clean(self.copy.path(), &partial_dir(&self.target), self.r, &self.env);
        let started = Instant::now();
        let out =
            plan_item(self.tc, self.copy.path(), &self.target, self.r, self.rlimit, timeout).envs(&self.env).run();
        let took = started.elapsed();
        match out {
            Ok(out) => Ok((judge(&out), took)),
            Err(SpawnError::TimedOut { .. }) => Ok((Judged::NoReport("timed out".into()), took)),
            Err(e) => Err(e.to_string()),
        }
    }

    fn fate_of(&self, under_root: &Path, text: &str, m: &Mutant, timeout: Duration) -> Result<Fate, String> {
        let path = self.copy.path().join(under_root);
        std::fs::write(&path, apply(text, m)).map_err(|e| format!("{}: {e}", path.display()))?;
        let _held = lock(&self.target).map_err(|e| e.to_string())?;
        clean(self.copy.path(), &partial_dir(&self.target), self.r, &self.env);
        let out =
            plan_item(self.tc, self.copy.path(), &self.target, self.r, self.rlimit, timeout).envs(&self.env).run();
        fate(out.as_ref())
    }
}

/// Copies a tree, leaving out build output and the repository's history.
pub fn copy_tree(from: &Path, to: &Path) -> Result<(), AdapterError> {
    let io = |path: &Path, e: std::io::Error| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() };
    for entry in std::fs::read_dir(from).map_err(|e| io(from, e))? {
        let entry = entry.map_err(|e| io(from, e))?;
        let name = entry.file_name();
        if name == "target" || name == ".git" {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        if src.is_dir() {
            std::fs::create_dir_all(&dst).map_err(|e| io(&dst, e))?;
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| io(&src, e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.as_bytes().to_vec(), stderr: stderr.as_bytes().to_vec() }
    }

    const PASS: &str = "verification results:: 1 verified, 0 errors\n{\n\"verification-results\": {\"success\": true, \"verified\": 1, \"errors\": 0},\n\"verus\": {\"version\": \"v\"}\n}\n";
    const FAIL: &str = "{\n\"verification-results\": {\"success\": false, \"encountered-error\": true, \"verified\": 0, \"errors\": 1},\n\"verus\": {\"version\": \"v\"}\n}\n";
    const VIR: &str = "{\n\"verification-results\": {\"success\": false, \"encountered-error\": true, \"encountered-vir-error\": true},\n\"verus\": {\"version\": \"v\"}\n}\n";

    #[test]
    fn fates_from_the_verifiers_answers() {
        assert_eq!(fate(Ok(&out(0, PASS, ""))), Ok(Fate::Survived));
        assert_eq!(
            fate(Ok(&out(101, FAIL, "error: postcondition not satisfied\n  --> src/lib.rs:9:5\n"))),
            Ok(Fate::Killed)
        );
        assert_eq!(
            fate(Ok(&out(101, FAIL, "error: Resource limit (rlimit) exceeded\n  --> src/lib.rs:9:5\n"))),
            Ok(Fate::Timeout)
        );
        assert_eq!(
            fate(Ok(&out(101, "", "error[E0308]: mismatched types\n  --> src/lib.rs:9:5\n"))),
            Ok(Fate::Unviable)
        );
        const ABORTED: &str = "{\n\"verification-results\": {\"success\": false, \"encountered-error\": true, \"verified\": 0, \"errors\": 0},\n\"verus\": {\"version\": \"v\"}\n}\n";
        assert_eq!(
            fate(Ok(&out(101, ABORTED, "error[E0308]: mismatched types\n"))),
            Ok(Fate::Unviable),
            "rustc's rejection comes with a JSON object too"
        );
        assert_eq!(fate(Ok(&out(101, VIR, "error: cannot call function with mode exec\n"))), Ok(Fate::Unviable));
        let timeout = SpawnError::TimedOut { program: PathBuf::from("cargo-verus"), after: Duration::from_secs(1) };
        assert_eq!(fate(Err(&timeout)), Ok(Fate::Timeout));
        let oom = SpawnError::OutOfMemory {
            program: PathBuf::from("cargo-verus"),
            limit_bytes: 1 << 30,
            peak_bytes: 2 << 30,
        };
        assert_eq!(fate(Err(&oom)), Ok(Fate::Timeout), "the memory budget: no verdict, never killed");
        let start = SpawnError::Start { program: PathBuf::from("cargo-verus"), message: "no such file".into() };
        assert!(fate(Err(&start)).is_err(), "a toolchain that cannot start aborts the score");
    }

    #[test]
    fn unviable_mutants_are_counted_apart() {
        let t = tally([Fate::Killed, Fate::Unviable, Fate::Survived, Fate::Unviable, Fate::Timeout, Fate::Killed]);
        assert_eq!(t, Tally { killed: 2, survived: 1, timeout: 1, unviable: 2 });
        assert_eq!(t.killed + t.survived + t.timeout, 4, "the denominator leaves the unviable out");
    }
}
