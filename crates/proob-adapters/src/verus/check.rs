//! `check` for a Verus binding: the whole crate through `cargo verus verify`
//! under `--no-cheating` with the fixed `--rlimit`, then, when it verifies,
//! the bound item alone through `cargo verus focus` with
//! `--verify-only-module` (or `--verify-root`) and `--verify-function`, so
//! the outcome names the item. Both read `--output-json` from stdout and the
//! diagnostics from stderr (`proob_facts::parsers::verus`).
//!
//! # The build directory
//!
//! Every run builds under `<target>/proob-verus` (`CARGO_TARGET_DIR` when
//! set, else `<root>/target`): the adapter's own, so nothing another cargo
//! invocation cached can be replayed as a verdict. Cargo replays a cached
//! build's diagnostics but never the verifier's stdout, and it fingerprints
//! none of the arguments forwarded to Verus, so an unchanged crate would
//! yield no JSON at all and a `focus` run with other arguments would be
//! replayed as this one's result. `plan_clean` therefore removes the one
//! package's artifacts before every run (`verify` builds under `debug`,
//! `focus` under `verus-partial`); the dependencies, `vstd` among them, stay
//! built, so the cost is the crate's own rebuild. Cargo also keys the crate's
//! artifacts without the tree's path and calls a tree of identical content
//! fresh, so a second proob run (a copy of the same tree, a parallel test)
//! that built between a clean and its run would make cargo replay a build
//! whose stdout, the verdict, it never replays; `lock` holds the build
//! directory for each clean-and-run pair (cargo's own lock serialises single
//! invocations, not pairs). `lock` also writes cargo's `CACHEDIR.TAG` into
//! the directory it creates: cargo refuses to clean a build directory
//! without one, and a refused clean is exactly the silent replay above.
use crate::argv::{Checked, Command};
use crate::verus::tools::{Resolution, Toolchain, resolve};
use crate::verus::{CheckRef, VerusAdapter, parse_check_ref};
use crate::{AdapterError, Binding, Check, CheckOutcome, ToolVersions, WITNESS_CAP, cap, exit_text};
use proob_facts::parsers::verus::{Results, diagnostics, parse_output, summarise};
use proob_spawn::Output;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A cold build of `vstd` under Verus, then the crate.
pub const CRATE_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// The item alone, in a warm build directory.
pub const ITEM_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub const TARGET_SUBDIR: &str = "proob-verus";
pub const PARTIAL_SUBDIR: &str = "verus-partial";
pub const LOCK_FILE: &str = ".proob-adapter.lock";
const NOT_FOUND_MARK: &str = "specified by --verify-function";

/// `<target>/proob-verus`, absolute: `CARGO_TARGET_DIR` (the override's,
/// else the process's) when set, else `<root>/target`.
pub fn target_dir(root: &Path, tool_env: &[(OsString, OsString)]) -> Result<PathBuf, AdapterError> {
    let root =
        root.canonicalize().map_err(|e| AdapterError::Io { path: root.to_path_buf(), message: e.to_string() })?;
    let configured = tool_env
        .iter()
        .find(|(k, _)| k == "CARGO_TARGET_DIR")
        .map(|(_, v)| PathBuf::from(v))
        .or_else(|| std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from));
    let base = match configured {
        Some(t) if t.is_absolute() => t,
        Some(t) => root.join(t),
        None => root.join("target"),
    };
    Ok(base.join(TARGET_SUBDIR))
}

/// The verus arguments every run forwards after `--`.
fn verus_args(command: Command, rlimit: u32) -> Command {
    command
        .separator()
        .arg("--no-cheating")
        .tail_opt("--rlimit", &Checked::number(u64::from(rlimit)))
        .arg("--output-json")
}

/// The whole crate: `cargo-verus verus verify --locked -p <pkg>
/// --fwd-verus-args-to roots --target-dir <X> -- --no-cheating --rlimit N --output-json`.
pub fn plan_crate(tc: &Toolchain, root: &Path, target: &Path, r: &CheckRef, rlimit: u32) -> Command {
    let command = Command::new(tc.cargo_verus(), CRATE_TIMEOUT)
        .arg("verus")
        .arg("verify")
        .arg("--locked")
        .opt("-p", &r.package)
        .arg("--fwd-verus-args-to")
        .arg("roots")
        .own("--target-dir", target)
        .cwd(root);
    verus_args(command, rlimit)
}

/// The bound item: `cargo-verus verus focus --locked -p <pkg> --target-dir <X>
/// -- --no-cheating --rlimit N --output-json (--verify-only-module M |
/// --verify-root) --verify-function F`.
pub fn plan_item(tc: &Toolchain, root: &Path, target: &Path, r: &CheckRef, rlimit: u32, timeout: Duration) -> Command {
    let command = Command::new(tc.cargo_verus(), timeout)
        .arg("verus")
        .arg("focus")
        .arg("--locked")
        .opt("-p", &r.package)
        .own("--target-dir", target)
        .cwd(root);
    let command = verus_args(command, rlimit);
    let command = match &r.module {
        Some(module) => command.tail_opt("--verify-only-module", module),
        None => command.arg("--verify-root"),
    };
    command.tail_opt("--verify-function", &r.function)
}

/// Where `focus` builds: `<X>/verus-partial`.
pub fn partial_dir(target: &Path) -> PathBuf {
    target.join(PARTIAL_SUBDIR)
}

/// `cargo clean --locked -p <pkg> --target-dir <dir>`: the package's
/// artifacts in one build directory, so the next run there really runs.
pub fn plan_clean(cargo: &Path, root: &Path, dir: &Path, r: &CheckRef) -> Command {
    Command::new(cargo, ITEM_TIMEOUT)
        .arg("clean")
        .arg("--locked")
        .opt("-p", &r.package)
        .own("--target-dir", dir)
        .cwd(root)
}

/// The cache directory tag cargo writes into a build directory it creates,
/// and refuses to `clean` without (it aborts rather than delete a directory
/// it did not make). The lock file below is created before cargo's first
/// build there, so the tag is written here too; its signature is the one
/// cargo checks (https://bford.info/cachedir/).
pub const CACHEDIR_TAG: &str = "CACHEDIR.TAG";
const CACHEDIR_TAG_TEXT: &str = "Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by proob's Verus adapter.\n# For information about cache directory tags see https://bford.info/cachedir/\n";

/// An exclusive lock on the build directory, held while a clean and the run
/// that follows it happen; released when dropped. Creating the directory
/// also tags it, or every later clean would be refused and an unchanged
/// crate replayed from cache with no verdict.
pub fn lock(target: &Path) -> Result<std::fs::File, AdapterError> {
    let io = |path: &Path, e: std::io::Error| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() };
    std::fs::create_dir_all(target).map_err(|e| io(target, e))?;
    let tag = target.join(CACHEDIR_TAG);
    if !tag.is_file() {
        std::fs::write(&tag, CACHEDIR_TAG_TEXT).map_err(|e| io(&tag, e))?;
    }
    let path = target.join(LOCK_FILE);
    let file =
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).map_err(|e| io(&path, e))?;
    file.lock().map_err(|e| io(&path, e))?;
    Ok(file)
}

/// Runs the clean when `cargo` is on the path; a clean that finds nothing
/// to remove (nothing built yet) is not a failed check.
pub fn clean(root: &Path, dir: &Path, r: &CheckRef, env: &[(OsString, OsString)]) {
    if let Some(cargo) = crate::rust::tools::find_on_path("cargo", env) {
        let _ = plan_clean(&cargo, root, dir, r).envs(env).run();
    }
}

/// What one verifier run says, read from its JSON and its stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judged {
    Verified,
    /// The SMT ran and rejected a proof. The text is the diagnostics.
    Rejected(String),
    /// rustc rejected the crate before any proof was judged. The text is
    /// the diagnostics. Not a failed proof: a check is `Errored`, a mutant
    /// unviable.
    Aborted(String),
    /// Verification never reached the SMT: a construct `--no-cheating`
    /// forbids, or a mode or type error in ghost code. For a check this is
    /// the crate's failure; for a mutant it means the mutant is unviable.
    VirError(String),
    /// `--verify-function` named an item the verifier has no function for.
    ItemNotFound,
    /// No JSON object: rustc failed, or the run did not finish. The text is
    /// the exit and the diagnostics or the last output.
    NoReport(String),
}

pub fn judge(out: &Output) -> Judged {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diags = diagnostics_text(&stderr);
    match parse_output(&String::from_utf8_lossy(&out.stdout)) {
        Ok(report) => match report.results {
            Results::Success { .. } => Judged::Verified,
            Results::Failed { .. } => Judged::Rejected(diags),
            Results::Aborted { .. } => Judged::Aborted(diags),
            Results::VirError if stderr.contains(NOT_FOUND_MARK) => Judged::ItemNotFound,
            Results::VirError => Judged::VirError(diags),
        },
        Err(_) => Judged::NoReport(format!("exit {}: {diags}", exit_text(out.code))),
    }
}

/// The diagnostics on stderr as one capped line, or its tail when there are none.
pub fn diagnostics_text(stderr: &str) -> String {
    let d = diagnostics(stderr);
    if d.is_empty() { cap(stderr.trim(), 600) } else { cap(&summarise(&d), WITNESS_CAP) }
}

pub fn run(adapter: &VerusAdapter, repo: &Path, binding: &Binding) -> Result<Check, AdapterError> {
    let r = parse_check_ref(&binding.check_ref)?;
    let root = adapter.root_in(repo);
    let tc = match resolve(adapter.tool_env()) {
        Resolution::Ready(tc) => tc,
        Resolution::Missing { reason, versions } => return Ok(errored(reason, versions)),
    };
    let target = target_dir(&root, adapter.tool_env())?;
    let env = tc.env(adapter.tool_env());
    let _held = lock(&target)?;
    clean(&root, &target, &r, &env);
    let crate_out = match plan_crate(&tc, &root, &target, &r, adapter.rlimit()).envs(&env).run() {
        Ok(out) => out,
        Err(e) => return Ok(errored(format!("cargo verus verify did not run: {e}"), tc.versions)),
    };
    let outcome = match judge(&crate_out) {
        Judged::Verified => attribute(adapter, &tc, &root, &target, &r, &env),
        Judged::Rejected(diagnostics) | Judged::VirError(diagnostics) => CheckOutcome::ProofFailed { diagnostics },
        Judged::Aborted(text) => {
            CheckOutcome::Errored { reason: format!("cargo verus verify stopped before any proof was judged: {text}") }
        }
        Judged::ItemNotFound => CheckOutcome::Errored {
            reason: "cargo verus verify named a function it should not have; report this".into(),
        },
        Judged::NoReport(text) => {
            CheckOutcome::Errored { reason: format!("cargo verus verify did not report a verdict ({text})") }
        }
    };
    Ok(Check { outcome, tool_versions: tc.versions })
}

/// The crate verified; now the item, so the result names it.
fn attribute(
    adapter: &VerusAdapter,
    tc: &Toolchain,
    root: &Path,
    target: &Path,
    r: &CheckRef,
    env: &[(OsString, OsString)],
) -> CheckOutcome {
    let item =
        format!("{}{}", r.module.as_ref().map_or(String::new(), |m| format!("{}::", m.as_str())), r.function.as_str());
    clean(root, &partial_dir(target), r, env);
    match plan_item(tc, root, target, r, adapter.rlimit(), ITEM_TIMEOUT).envs(env).run() {
        Ok(out) => match judge(&out) {
            Judged::Verified => CheckOutcome::Verified { rlimit: adapter.rlimit() },
            Judged::Rejected(diagnostics) | Judged::VirError(diagnostics) => CheckOutcome::ProofFailed { diagnostics },
            Judged::Aborted(text) => CheckOutcome::Errored {
                reason: format!("cargo verus focus stopped before `{item}` was judged: {text}"),
            },
            Judged::ItemNotFound => CheckOutcome::Errored {
                reason: format!(
                    "bound item not found: the verifier has no function `{item}`; was it renamed, moved, or taken out of the verus! block?"
                ),
            },
            Judged::NoReport(text) => CheckOutcome::Errored {
                reason: format!("cargo verus focus did not report a verdict for `{item}` ({text})"),
            },
        },
        Err(e) => CheckOutcome::Errored { reason: format!("cargo verus focus did not run: {e}") },
    }
}

fn errored(reason: String, tool_versions: ToolVersions) -> Check {
    Check { outcome: CheckOutcome::Errored { reason }, tool_versions }
}
