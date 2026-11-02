//! The tools the Rust adapter drives, resolved from `PATH` and version-probed
//! before any run, so every result records what produced it and a missing
//! tool is a closed `Errored` with an install hint rather than a stack trace.
use crate::ToolVersions;
use crate::argv::Command;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Nextest,
    Mutants,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub cargo: PathBuf,
    pub versions: ToolVersions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ready(Tools),
    /// Why a run cannot start, and what was learnt before that.
    Missing {
        reason: String,
        versions: ToolVersions,
    },
}

pub const CARGO_HINT: &str =
    "cargo was not found on PATH: install Rust with rustup (https://rustup.rs) so `cargo` is available";
pub const NEXTEST_HINT: &str = "cargo-nextest is not installed: `cargo install --locked cargo-nextest --version 0.9.150` (in CI: taiki-e/install-action with tool: cargo-nextest@0.9.150)";
pub const MUTANTS_HINT: &str = "cargo-mutants is not installed: `cargo install --locked cargo-mutants --version 27.1.0` (in CI: taiki-e/install-action with tool: cargo-mutants@27.1.0)";

/// `cargo` from `PATH` (the override's when `tool_env` sets one), then
/// `cargo --version` and the subcommands in `needs`.
pub fn resolve(root: &Path, needs: &[Need], tool_env: &[(OsString, OsString)]) -> Resolution {
    let Some(cargo) = find_on_path("cargo", tool_env) else {
        return Resolution::Missing { reason: CARGO_HINT.to_string(), versions: Vec::new() };
    };
    let mut versions = Vec::new();
    match probe(&cargo, root, &[], "cargo", tool_env) {
        Ok(v) => versions.push(v),
        Err(e) => return Resolution::Missing { reason: format!("cargo --version failed: {e}"), versions },
    }
    for need in needs {
        let (sub, name, hint) = match need {
            Need::Nextest => ("nextest", "cargo-nextest", NEXTEST_HINT),
            Need::Mutants => ("mutants", "cargo-mutants", MUTANTS_HINT),
        };
        match probe(&cargo, root, &[sub], name, tool_env) {
            Ok(v) => versions.push(v),
            Err(_) => return Resolution::Missing { reason: hint.to_string(), versions },
        }
    }
    versions.extend(proptest_version(root));
    Resolution::Ready(Tools { cargo, versions })
}

/// `name` (or `name.exe`) on the override's `PATH`, else the process's.
pub(crate) fn find_on_path(name: &str, tool_env: &[(OsString, OsString)]) -> Option<PathBuf> {
    let path =
        tool_env.iter().find(|(k, _)| k == "PATH").map(|(_, v)| v.clone()).or_else(|| std::env::var_os("PATH"))?;
    std::env::split_paths(&path)
        .flat_map(|dir| [dir.join(name), dir.join(format!("{name}.exe"))])
        .find(|candidate| candidate.is_file())
}

/// `cargo [sub] --version`; the second word of the first line is the version.
fn probe(
    cargo: &Path,
    root: &Path,
    sub: &[&'static str],
    name: &str,
    tool_env: &[(OsString, OsString)],
) -> Result<(String, String), String> {
    let mut command = Command::new(cargo, PROBE_TIMEOUT).cwd(root).envs(tool_env).env("CARGO_TERM_COLOR", "never");
    for s in sub {
        command = command.arg(s);
    }
    let out = command.arg("--version").run().map_err(|e| e.to_string())?;
    if !out.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let version = text.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("unknown");
    Ok((name.to_string(), version.to_string()))
}

/// proptest's version from the root's `Cargo.lock`, when it is a dependency.
fn proptest_version(root: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(root.join("Cargo.lock")).ok()?;
    let lock: toml::Table = text.parse().ok()?;
    lock.get("package")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("proptest"))
        .and_then(|p| p.get("version")?.as_str().map(|v| ("proptest".to_string(), v.to_string())))
}
