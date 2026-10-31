//! `check` for a Kani binding: the one harness through `cargo kani` with
//! `--exact` and `--sarif`, classified by `proob_facts::parsers::kani`.
//!
//! Every run builds under `<target>/proob-kani` (`CARGO_TARGET_DIR` when set,
//! else `<root>/target`), the adapter's own, with cargo's `CACHEDIR.TAG` and a
//! lock held for the run (`verus::check::lock`), so a parallel run cannot
//! replace the goto binary between the build and CBMC. Kani runs CBMC on
//! every invocation, so a cached build never replays a verdict and nothing is
//! cleaned.
use crate::argv::Command;
use crate::kani::tools::{Resolution, Toolchain, resolve};
use crate::kani::{CheckRef, KaniAdapter, parse_check_ref};
use crate::verus::check::{lock, target_base};
use crate::{AdapterError, Binding, Check, CheckOutcome, ProofMeasure, WITNESS_CAP, cap, exit_text};
use proob_facts::parsers::kani::{Class, classify};
use proob_spawn::{Output, SpawnError};
use std::path::Path;
use std::time::Duration;

/// The outer wall-clock limit on one run: the build and CBMC together.
pub const TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub const TARGET_SUBDIR: &str = "proob-kani";
const REASON_CAP: usize = 600;

/// `cargo-kani kani -p <pkg> --tests --harness <path> --exact --sarif <file> --target-dir <X>`.
pub fn plan(cargo_kani: &Path, root: &Path, target: &Path, sarif: &Path, r: &CheckRef, timeout: Duration) -> Command {
    Command::new(cargo_kani, timeout)
        .arg("kani")
        .opt("-p", &r.package)
        .arg("--tests")
        .opt("--harness", &r.harness)
        .arg("--exact")
        .own("--sarif", sarif)
        .own("--target-dir", target)
        .cwd(root)
}

pub fn run(adapter: &KaniAdapter, repo: &Path, binding: &Binding) -> Result<Check, AdapterError> {
    let r = parse_check_ref(&binding.check_ref)?;
    let root = adapter.root_in(repo);
    let tc = match resolve(adapter.tool_env()) {
        Resolution::Ready(tc) => tc,
        Resolution::Missing { reason, versions } => {
            return Ok(Check { outcome: CheckOutcome::Errored { reason }, tool_versions: versions });
        }
    };
    let target = target_base(&root, adapter.tool_env())?.join(TARGET_SUBDIR);
    let _held = lock(&target)?;
    let temp = tempfile::tempdir().map_err(|e| AdapterError::Temp { message: e.to_string() })?;
    let sarif = temp.path().join("kani.sarif");
    let lock_file = root.join("Cargo.lock");
    let before = std::fs::read(&lock_file).ok();
    let ran = plan(&tc.cargo_kani, &root, &target, &sarif, &r, TIMEOUT).envs(&Toolchain::env(adapter.tool_env())).run();
    // Kani's nightly cargo cannot take --locked and may rewrite Cargo.lock, a bound file:
    // a check must leave the user's tree as it found it.
    if let Err(e) = restore(&lock_file, before.as_deref()) {
        return Ok(Check {
            outcome: CheckOutcome::Errored {
                reason: format!("Kani changed Cargo.lock and proob could not restore it: {e}"),
            },
            tool_versions: tc.versions,
        });
    }
    let outcome = match ran {
        Ok(out) => judge(&out, std::fs::read_to_string(&sarif).ok().as_deref(), &r),
        Err(SpawnError::TimedOut { after, .. }) => CheckOutcome::Errored {
            reason: format!("Kani timed out after {} minutes, the adapter's limit on one run", after.as_secs() / 60),
        },
        Err(e) => CheckOutcome::Errored { reason: format!("cargo kani did not run: {e}") },
    };
    Ok(Check { outcome, tool_versions: tc.versions })
}

/// Puts `Cargo.lock` back as it was before the run: the same bytes, or absent.
pub fn restore(lock_file: &Path, before: Option<&[u8]>) -> std::io::Result<()> {
    let now = std::fs::read(lock_file).ok();
    match (before, now.as_deref()) {
        (Some(b), Some(n)) if b == n => Ok(()),
        (Some(b), _) => std::fs::write(lock_file, b),
        (None, Some(_)) => std::fs::remove_file(lock_file),
        (None, None) => Ok(()),
    }
}

/// The outcome of one finished run, from its output and its SARIF file.
pub fn judge(out: &Output, sarif: Option<&str>, r: &CheckRef) -> CheckOutcome {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let class = classify(&stdout, &stderr, sarif, r.harness.as_str());
    outcome(class, out.code, r)
}

/// The outcome table of the plan, row by row.
pub fn outcome(class: Class, code: Option<i32>, r: &CheckRef) -> CheckOutcome {
    let errored = |reason: String| CheckOutcome::Errored { reason: cap(&reason, REASON_CAP) };
    let harness = r.harness.as_str();
    match class {
        Class::Verified { .. } if code != Some(0) => {
            errored(format!("unrecognised Kani output: a SUCCESSFUL verdict but exit {}", exit_text(code)))
        }
        Class::Verified { checks: 0 } => errored(format!(
            "nothing was checked: Kani verified `{harness}` but proved no check; a harness must assert what the statement says"
        )),
        Class::Verified { checks } => CheckOutcome::Verified { measure: ProofMeasure::Kani { checks } },
        Class::Failed { diagnostics } => CheckOutcome::ProofFailed { diagnostics: cap(&diagnostics, WITNESS_CAP) },
        Class::Unsupported { construct } => {
            errored(format!("Kani cannot verify a construct this code reaches: {construct}"))
        }
        Class::Unwinding { at } => errored(format!(
            "the harness's unwind bound is too small for a loop: {at}; raise `#[kani::unwind(N)]` on `{harness}`"
        )),
        Class::Undetermined { checks } => {
            errored(format!("Kani could not decide {} check(s): {}", checks.len(), checks.join("; ")))
        }
        Class::Timeout => errored("Kani timed out: CBMC hit its time limit".to_string()),
        Class::OutOfMemory => errored("Kani ran out of memory".to_string()),
        Class::VacuousCover { cover } => {
            errored(format!("vacuous: the harness cannot reach a case it covers: {cover}"))
        }
        Class::PreprocessorFailed { detail } => errored(format!(
            "Kani's C preprocessor failed; a non-system compiler on PATH may shadow the system one: {detail}"
        )),
        Class::CompileError { detail } => errored(format!("the crate did not compile under Kani: {detail}")),
        Class::HarnessNotFound => errored(format!(
            "harness not found: Kani has no harness `{harness}` in package `{}`; was it renamed, moved, or put behind a cfg Kani does not set?",
            r.package.as_str()
        )),
        Class::Unrecognised { detail } => errored(format!("unrecognised Kani output: {detail}")),
    }
}
