//! `score` for a Kani binding: mutation under Kani. cargo-mutants cannot use
//! Kani as its test tool, so it only lists the scope's mutants (`--list
//! --json --diff`); each sampled mutant's diff is applied in a temporary copy
//! of the adapter root and the one harness is run there as `check` runs it.
//!
//! Mutants in harness code are dropped before sampling: in files reached
//! through a `mod` under `#[cfg(kani)]`, inside an inline `#[cfg(kani)]`
//! module, or inside a bound harness function. Mutating the proof is not
//! mutating the code under proof.
//!
//! Fates, as the Verus adapter's (`verus::score`):
//!
//! - an assertion written in the harness fails: **killed**;
//! - only a Kani safety check fails (overflow, bounds, a panic elsewhere):
//!   **safety**. The statement is not "the code does not panic", so it is no
//!   kill; it is counted as **survived**, since the evidence record has no
//!   field for it and an older reader refuses unknown fields: a mutant the
//!   harness's own assertions did not catch was not caught;
//! - `SUCCESSFUL`: **survived**;
//! - unwinding failure, undetermined checks, CBMC timeout or out of memory,
//!   the per-mutant limit, or proob's memory budget: **timeout**;
//! - anything Kani cannot check (compile error, unsupported construct,
//!   harness not found, a vacuous cover, unrecognised output): **unviable**,
//!   excluded from every denominator.
//!
//! The unmutated copy runs first (the baseline) and must verify, or nothing
//! is scored. Each mutant gets five times the baseline's time, at least
//! `verus::score::MIN_MUTANT_TIMEOUT`. Mutants beyond `max_mutants` are
//! chosen by `mutation::sample` under the property's scoring seed and the
//! rest counted as unsampled. The copy builds under `<target>/proob-kani-score`
//! under `kani::check`'s lock, held for the whole score, so dependencies
//! build once per machine and two scores never overlap; not in
//! `check`'s directory, and with every copied file's mtime set to now:
//! cargo names a path package's build by its path relative to the workspace,
//! so the copy, the real tree, and every earlier copy share one build, and a
//! copied file keeps its old mtime, which would let cargo reuse an earlier
//! mutant's build as fresh.
use crate::argv::{Checked, Command};
use crate::kani::check::{judge, out_of_memory, plan, restore};
use crate::kani::tools::{Resolution, Toolchain, resolve};
use crate::kani::{CheckRef, KaniAdapter, parse_check_ref};
use crate::mutation::sample;
use crate::rust::discover::crates;
use crate::rust::scan::{Bound, scan};
use crate::rust::score::{LIST_TIMEOUT, Outside, listing, rust_files_under};
use crate::rust::tools::{self as rust_tools, Need};
use crate::verus::check::{lock, target_base};
use crate::verus::score::{BASELINE_TIMEOUT, MIN_MUTANT_TIMEOUT, TIMEOUT_MULTIPLIER, copy_tree};
use crate::{AdapterError, Binding, CheckOutcome, Score, ScoreOutcome, Seed, ToolVersions};
use proob_facts::parsers::cargo_mutants::{Listed, parse_listing};
use proob_facts::parsers::kani::{Class, Failure, classify};
use proob_spawn::{Output, SpawnError};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Recorded in `tool_versions` under `OPERATOR_SET_KEY`.
pub const OPERATOR_SET: &str = "cargo-mutants-under-kani-v1";
pub const OPERATOR_SET_KEY: &str = "mutants-under-kani";
/// Under the adapter's target base, apart from `check`'s (module doc).
pub const TARGET_SUBDIR: &str = "proob-kani-score";

/// One run of the harness: its result, its SARIF, its time.
pub(crate) type Ran = (Result<Output, SpawnError>, Option<String>, Duration);

/// What one mutant's Kani run means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    Killed,
    /// Only a safety check failed; counted as survived (module doc).
    Safety,
    Survived,
    Timeout,
    Unviable,
}

/// Pure: the fate of a finished run's class and exit code.
pub fn fate_of(class: &Class, code: Option<i32>) -> Fate {
    match class {
        // The mutant made the assertions unreachable: the proof did not catch it.
        Class::Verified { .. } | Class::Vacuous { .. } if code == Some(0) => Fate::Survived,
        Class::Failed { failure: Failure::Assertion, .. } => Fate::Killed,
        Class::Failed { failure: Failure::Safety, .. } => Fate::Safety,
        Class::Unwinding { .. } | Class::Undetermined { .. } | Class::Timeout | Class::OutOfMemory => Fate::Timeout,
        Class::Verified { .. }
        | Class::Unsupported { .. }
        | Class::VacuousCover { .. }
        | Class::Vacuous { .. }
        | Class::PreprocessorFailed { .. }
        | Class::CompileError { .. }
        | Class::HarnessNotFound
        | Class::Unrecognised { .. } => Fate::Unviable,
    }
}

/// Pure: the fate of one run of `harness`. A process that never started
/// aborts the score (`Err`): every later mutant would fail the same way.
pub fn fate(run: Result<&Output, &SpawnError>, sarif: Option<&str>, harness: &str) -> Result<Fate, String> {
    match run {
        // A mutant that took more memory than the budget has no verdict, as one that took more time.
        Err(SpawnError::TimedOut { .. } | SpawnError::OutOfMemory { .. }) => Ok(Fate::Timeout),
        Err(e) => Err(e.to_string()),
        Ok(out) => {
            let class =
                classify(&String::from_utf8_lossy(&out.stdout), &String::from_utf8_lossy(&out.stderr), sarif, harness);
            Ok(fate_of(&class, out.code))
        }
    }
}

/// Pure: the counts of a sample, safety folded into survived.
pub fn scored(fates: &[Fate], unsampled: u32) -> ScoreOutcome {
    let count = |f: &[Fate]| fates.iter().filter(|x| f.contains(x)).count() as u32;
    ScoreOutcome::Scored {
        killed: count(&[Fate::Killed]),
        survived: count(&[Fate::Survived, Fate::Safety]),
        timeout: count(&[Fate::Timeout]),
        unviable: count(&[Fate::Unviable]),
        unsampled,
    }
}

/// Where harness code sits under the adapter root, root-relative.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarnessCode {
    /// Files reached through a `mod` under `#[cfg(kani)]`.
    pub files: BTreeSet<PathBuf>,
    /// (file, first line, last line) of each inline `#[cfg(kani)]` module.
    pub spans: Vec<(PathBuf, usize, usize)>,
    /// (file, path inside the file) of each bound harness, as cargo-mutants names functions.
    pub functions: BTreeSet<(PathBuf, String)>,
}

impl HarnessCode {
    pub fn contains(&self, m: &Listed) -> bool {
        let file = Path::new(&m.file);
        self.files.contains(file)
            || self.spans.iter().any(|(f, first, last)| f == file && (*first..=*last).contains(&m.line))
            || m.function.as_ref().is_some_and(|name| self.functions.contains(&(file.to_path_buf(), name.clone())))
    }
}

/// The harness code of the crates under the adapter root, reading `files`
/// (root-relative) for inline modules and harness functions.
pub fn harness_code(adapter: &KaniAdapter, repo: &Path, files: &[Checked]) -> Result<HarnessCode, AdapterError> {
    let root = adapter.root_in(repo);
    let under_root = |rel: &Path| rel.strip_prefix(adapter.root()).unwrap_or(rel).to_path_buf();
    let mut code = HarnessCode::default();
    for (_, krate) in crates(&root, adapter.root(), repo)? {
        code.files.extend(krate.files.iter().filter(|f| f.in_kani_tree).map(|f| under_root(&f.rel)));
    }
    for file in files {
        let path = root.join(file.as_str());
        let text = std::fs::read_to_string(&path)
            .map_err(|e| AdapterError::Io { path: path.clone(), message: e.to_string() })?;
        let scanned = scan(&text);
        let rel = PathBuf::from(file.as_str());
        code.spans.extend(scanned.kani_spans.iter().map(|(a, b)| (rel.clone(), *a, *b)));
        code.functions.extend(scanned.tokens.into_iter().filter_map(|t| match t.bound {
            Bound::Kani { function, module_path, .. } => {
                Some((rel.clone(), module_path.into_iter().chain([function]).collect::<Vec<_>>().join("::")))
            }
            _ => None,
        }));
    }
    Ok(code)
}

/// `cargo mutants --package <pkg> --file <f>… --list --json --diff`. Pure.
pub fn list_plan(cargo: &Path, root: &Path, package: &Checked, files: &[Checked]) -> Command {
    files
        .iter()
        .fold(Command::new(cargo, LIST_TIMEOUT).arg("mutants").opt("--package", package), |c, f| c.opt("--file", f))
        .arg("--list")
        .arg("--json")
        .arg("--diff")
        .cwd(root)
        .env("CARGO_TERM_COLOR", "never")
}

/// Pure: `text` with cargo-mutants' unified diff of it applied. Every
/// context and removed line must match where its hunk says, or it fails.
pub fn patch(text: &str, diff: &str) -> Result<String, String> {
    let old: Vec<&str> = text.lines().collect();
    let mut new: Vec<&str> = Vec::with_capacity(old.len());
    let (mut at, mut in_hunk) = (0usize, false);
    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@ -") {
            let start = hunk_start(header).ok_or_else(|| format!("bad hunk header `{line}`"))?;
            if start < at || start > old.len() {
                return Err(format!("hunk `{line}` is out of order or past the end of the file"));
            }
            new.extend(&old[at..start]);
            (at, in_hunk) = (start, true);
            continue;
        }
        if !in_hunk || line.starts_with('\\') {
            continue;
        }
        let (op, body) = (line.get(..1).unwrap_or("?"), line.get(1..).unwrap_or(""));
        if op == "+" {
            new.push(body);
            continue;
        }
        if !matches!(op, " " | "-" | "") || old.get(at) != Some(&body) {
            return Err(format!("line {} does not match the diff: `{line}`", at + 1));
        }
        if op != "-" {
            new.push(body);
        }
        at += 1;
    }
    new.extend(&old[at.min(old.len())..]);
    let ending = if text.ends_with('\n') { "\n" } else { "" };
    Ok(new.join("\n") + ending)
}

/// The 0-based index of the first old line a hunk header (past `@@ -`)
/// covers: `N,M` starts at line N, and `N,0` inserts after it.
fn hunk_start(header: &str) -> Option<usize> {
    let old = header.split(' ').next()?;
    let (start, count) = old.split_once(',').unwrap_or((old, "1"));
    let (start, count): (usize, usize) = (start.parse().ok()?, count.parse().ok()?);
    Some(if count == 0 { start } else { start.checked_sub(1)? })
}

pub fn run(
    adapter: &KaniAdapter,
    repo: &Path,
    binding: &Binding,
    scope_files: &[PathBuf],
) -> Result<Score, AdapterError> {
    let r = parse_check_ref(&binding.check_ref)?;
    let root = adapter.root_in(repo);
    let tc = match resolve(adapter.tool_env()) {
        Resolution::Ready(tc) => tc,
        Resolution::Missing { reason, versions } => return Ok(errored(reason, versions)),
    };
    let cargo = match rust_tools::resolve(&root, &[Need::Mutants], adapter.tool_env()) {
        rust_tools::Resolution::Ready(tools) => tools,
        rust_tools::Resolution::Missing { reason, .. } => return Ok(errored(reason, tc.versions)),
    };
    let versions: ToolVersions = tc
        .versions
        .iter()
        .cloned()
        .chain(cargo.versions.iter().filter(|(k, _)| k == "cargo-mutants").cloned())
        .chain([(OPERATOR_SET_KEY.to_string(), OPERATOR_SET.to_string())])
        .collect();
    let files = match rust_files_under(adapter.root(), scope_files) {
        Ok(files) => files,
        Err(Outside::Root(f)) => {
            let reason = format!(
                "scope file {} lies outside the adapter root `{}`; this adapter cannot mutate it",
                f.display(),
                adapter.root().display()
            );
            return Ok(errored(reason, versions));
        }
        Err(Outside::Grammar(f, rule)) => return Err(AdapterError::Unsafe { value: f.display().to_string(), rule }),
    };
    if files.is_empty() {
        return Ok(Score { outcome: ScoreOutcome::NoMutants, tool_versions: versions, jobs: None });
    }
    let code = harness_code(adapter, repo, &files)?;
    let listed = match list(&cargo.cargo, &root, &r, &files, adapter.tool_env()) {
        Ok(listed) => listed,
        Err(reason) => return Ok(errored(reason, versions)),
    };
    let kept: Vec<Listed> = listed.into_iter().filter(|m| !code.contains(m)).collect();
    if kept.is_empty() {
        return Ok(Score { outcome: ScoreOutcome::NoMutants, tool_versions: versions, jobs: None });
    }
    let names: Vec<String> = kept.iter().map(|m| m.name.clone()).collect();
    let chosen = sample(&names, adapter.max_mutants(), Seed::for_scoring(binding.id));
    let lab = Lab::new(adapter, &tc, &root, &r)?;
    let baseline = match lab.baseline() {
        Ok(took) => took,
        Err(reason) => return Ok(errored(reason, versions)),
    };
    let per_mutant = (baseline * TIMEOUT_MULTIPLIER).max(MIN_MUTANT_TIMEOUT);
    let mut fates = Vec::with_capacity(chosen.len());
    for m in chosen.iter().map(|&i| &kept[i]) {
        match lab.fate_of(m, per_mutant) {
            Ok(f) => fates.push(f),
            Err(e) => return Ok(errored(format!("mutant {} did not run: {e}", m.name), versions)),
        }
    }
    let unsampled = u32::try_from(kept.len() - chosen.len()).unwrap_or(u32::MAX);
    Ok(Score { outcome: scored(&fates, unsampled), tool_versions: versions, jobs: None })
}

/// Every mutant in the scope files, or why they could not be listed. The
/// listing runs in the real tree, so `Cargo.lock` is put back as it was.
fn list(
    cargo: &Path,
    root: &Path,
    r: &CheckRef,
    files: &[Checked],
    tool_env: &[(OsString, OsString)],
) -> Result<Vec<Listed>, String> {
    let lock_file = root.join("Cargo.lock");
    let before = std::fs::read(&lock_file).ok();
    let text = listing(list_plan(cargo, root, &r.package, files).envs(tool_env));
    restore(&lock_file, before.as_deref())
        .map_err(|e| format!("cargo mutants changed Cargo.lock and proob could not restore it: {e}"))?;
    parse_listing(&text?).map_err(|e| format!("cargo mutants --list --json could not be read: {e}"))
}

fn errored(reason: String, tool_versions: ToolVersions) -> Score {
    Score { outcome: ScoreOutcome::Errored { reason }, tool_versions, jobs: None }
}

/// A temporary copy of the adapter root where the harness runs on mutants
/// (and the build probe's harness, `probe`).
pub(crate) struct Lab<'a> {
    dir: tempfile::TempDir,
    cargo_kani: &'a Path,
    target: PathBuf,
    r: &'a CheckRef,
    env: Vec<(OsString, OsString)>,
    /// The target's lock, held for the lab's life: another lab of the same
    /// crate shares its build (module doc), so a mutant written while that
    /// lab builds could be read as fresh, or two CBMC runs overlap.
    _held: std::fs::File,
}

impl<'a> Lab<'a> {
    pub(crate) fn new(
        adapter: &KaniAdapter,
        tc: &'a Toolchain,
        root: &Path,
        r: &'a CheckRef,
    ) -> Result<Lab<'a>, AdapterError> {
        let dir = tempfile::tempdir().map_err(|e| AdapterError::Temp { message: e.to_string() })?;
        let copy = dir.path().join("crate");
        std::fs::create_dir(&copy).map_err(|e| AdapterError::Io { path: copy.clone(), message: e.to_string() })?;
        let target = target_base(root, adapter.tool_env())?.join(TARGET_SUBDIR);
        let _held = lock(&target)?;
        copy_tree(root, &copy)?;
        touch_tree(&copy, SystemTime::now())?;
        Ok(Lab { dir, cargo_kani: &tc.cargo_kani, target, r, env: Toolchain::env(adapter.tool_env()), _held })
    }

    /// The copy of the adapter root.
    pub(crate) fn copy(&self) -> PathBuf {
        self.dir.path().join("crate")
    }

    /// One run of the harness in the copy: its result, its SARIF, its time.
    pub(crate) fn harness(&self, timeout: Duration) -> Result<Ran, String> {
        let sarif = self.dir.path().join("kani.sarif");
        let _ = std::fs::remove_file(&sarif);
        let started = Instant::now();
        let out = plan(self.cargo_kani, &self.copy(), &self.target, &sarif, self.r, timeout).envs(&self.env).run();
        Ok((out, std::fs::read_to_string(&sarif).ok(), started.elapsed()))
    }

    /// The unmutated copy must verify; how long it took, or why it did not.
    fn baseline(&self) -> Result<Duration, String> {
        let (out, sarif, took) = self.harness(BASELINE_TIMEOUT).map_err(|e| format!("baseline did not run: {e}"))?;
        let why = match out {
            Ok(out) => match judge(&out, sarif.as_deref(), self.r) {
                CheckOutcome::Verified { .. } => return Ok(took),
                CheckOutcome::ProofFailed { diagnostics } => diagnostics,
                CheckOutcome::Errored { reason } => reason,
                other => format!("{other:?}"),
            },
            Err(SpawnError::TimedOut { after, .. }) => format!("Kani timed out after {} minutes", after.as_secs() / 60),
            Err(SpawnError::OutOfMemory { limit_bytes, .. }) => out_of_memory(limit_bytes),
            Err(e) => return Err(format!("baseline did not run: {e}")),
        };
        Err(format!("baseline failed: the harness does not verify before any mutation: {why}"))
    }

    /// Applies `m` in the copy, runs the harness, and puts the file back.
    fn fate_of(&self, m: &Listed, timeout: Duration) -> Result<Fate, String> {
        let path = self.copy().join(&m.file);
        let original = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mutated = patch(&original, &m.diff).map_err(|e| format!("its diff does not apply: {e}"))?;
        std::fs::write(&path, mutated).map_err(|e| format!("{}: {e}", path.display()))?;
        let ran = self.harness(timeout);
        std::fs::write(&path, &original).map_err(|e| format!("{}: {e}", path.display()))?;
        let (out, sarif, _) = ran?;
        fate(out.as_ref(), sarif.as_deref(), self.r.harness.as_str())
    }
}

/// Sets every file's mtime under `dir` to `now` (module doc).
fn touch_tree(dir: &Path, now: SystemTime) -> Result<(), AdapterError> {
    let io = |path: &Path, e: std::io::Error| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() };
    for entry in std::fs::read_dir(dir).map_err(|e| io(dir, e))? {
        let path = entry.map_err(|e| io(dir, e))?.path();
        if path.is_dir() {
            touch_tree(&path, now)?;
        } else {
            let file = std::fs::File::options().write(true).open(&path).map_err(|e| io(&path, e))?;
            file.set_modified(now).map_err(|e| io(&path, e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_applies_where_it_says_and_nowhere_else() {
        let text = "a\nb\nc\nd\n";
        let diff = "--- f.rs\n+++ replace b\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n";
        assert_eq!(patch(text, diff), Ok("a\nB\nc\nd\n".to_string()));
        let two = "--- f.rs\n+++ x\n@@ -1,1 +1,2 @@\n a\n+a2\n@@ -4,1 +5,1 @@\n-d\n+D\n";
        assert_eq!(patch(text, two), Ok("a\na2\nb\nc\nD\n".to_string()));
        assert!(patch(text, "@@ -2,1 +2,1 @@\n-x\n+y\n").is_err(), "a removed line that is not there");
        assert!(patch("a", "@@ -1,1 +1,1 @@\n-a\n+b\n\\ No newline at end of file\n").is_ok_and(|t| t == "b"));
        assert_eq!(patch(text, "@@ -2,0 +3,1 @@\n+x\n"), Ok("a\nb\nx\nc\nd\n".to_string()), "an insertion");
        assert!(patch(text, "@@ -9,1 +9,1 @@\n-x\n").is_err(), "past the end");
        assert!(patch(text, "@@ -3,1 +3,1 @@\n c\n@@ -1,1 +1,1 @@\n a\n").is_err(), "out of order");
    }

    #[test]
    fn safety_is_counted_as_survived_and_unviable_apart() {
        let fates = [Fate::Killed, Fate::Safety, Fate::Survived, Fate::Unviable, Fate::Timeout, Fate::Killed];
        assert_eq!(
            scored(&fates, 3),
            ScoreOutcome::Scored { killed: 2, survived: 2, timeout: 1, unviable: 1, unsampled: 3 }
        );
    }
}
