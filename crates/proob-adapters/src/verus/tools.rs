//! The Verus toolchain the adapter drives: `verus`, `cargo-verus`, and `z3`
//! in one directory, located before any run and version-probed, so every
//! result records what verified it and a missing toolchain is a closed
//! `Errored` with a hint rather than a stack trace.
//!
//! Resolution order: `PROOB_VERUS_DIR` (the directory itself, or
//! tools/get-verus.sh's destination, which holds `verus-<platform>/`), then
//! the directory of `verus` on `PATH` when `cargo-verus` sits beside it, then
//! the newest release under `~/.proob-dev/verus/<version>/verus-<platform>/`,
//! where get-verus.sh caches what it fetched. `cargo` itself comes from
//! `PATH`; `cargo-verus` runs it. Every child process gets the toolchain
//! directory prepended to `PATH`, which is how `cargo-verus` finds `verus`
//! and `verus` finds `z3`.
use crate::ToolVersions;
use crate::argv::Command;
use crate::rust::tools::find_on_path;
use proob_facts::parsers::verus::parse_version;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const VERUS_DIR_ENV: &str = "PROOB_VERUS_DIR";
/// Under `HOME`: tools/get-verus.sh's cache.
pub const CACHE_DIR: &str = ".proob-dev/verus";
const PROBE_TIMEOUT: Duration = Duration::from_secs(120);

pub const VERUS_HINT: &str = "the Verus toolchain was not found: set PROOB_VERUS_DIR to a directory holding `verus`, `cargo-verus`, and `z3`, \
                              put one on PATH, or run tools/get-verus.sh (it caches releases under ~/.proob-dev/verus)";
pub const CARGO_HINT: &str =
    "cargo was not found on PATH: install Rust with rustup (https://rustup.rs) so `cargo` is available";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    pub dir: PathBuf,
    pub versions: ToolVersions,
}

impl Toolchain {
    pub fn cargo_verus(&self) -> PathBuf {
        self.dir.join(exe("cargo-verus"))
    }

    /// The environment every tool process runs in: the override, then
    /// `PATH` with the toolchain first, colours off.
    pub fn env(&self, tool_env: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
        let existing = var(tool_env, "PATH").unwrap_or_default();
        let joined = std::env::join_paths([self.dir.clone()].into_iter().chain(std::env::split_paths(&existing)))
            .unwrap_or_else(|_| existing.clone());
        tool_env
            .iter()
            .filter(|(k, _)| k != "PATH")
            .cloned()
            .chain([(OsString::from("PATH"), joined), (OsString::from("CARGO_TERM_COLOR"), OsString::from("never"))])
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ready(Toolchain),
    /// Why a run cannot start, and what was learnt before that.
    Missing {
        reason: String,
        versions: ToolVersions,
    },
}

/// The toolchain, with `cargo`, `verus`, and `z3` version-probed.
pub fn resolve(tool_env: &[(OsString, OsString)]) -> Resolution {
    let Some(dir) = locate(tool_env) else {
        return Resolution::Missing { reason: VERUS_HINT.to_string(), versions: Vec::new() };
    };
    let tc = Toolchain { dir, versions: Vec::new() };
    let env = tc.env(tool_env);
    let mut versions = Vec::new();
    let Some(cargo) = find_on_path("cargo", &env) else {
        return Resolution::Missing { reason: CARGO_HINT.to_string(), versions };
    };
    match probe(&cargo, &["--version"], &env).map(|text| second_word(&text)) {
        Ok(v) => versions.push(("cargo".to_string(), v)),
        Err(e) => return Resolution::Missing { reason: format!("cargo --version failed: {e}"), versions },
    }
    match probe(&tc.dir.join(exe("verus")), &["--version", "--output-json"], &env)
        .and_then(|text| parse_version(&text).map_err(|e| e.to_string()))
    {
        Ok(info) => {
            versions.push(("verus".to_string(), info.version));
            versions.extend(info.commit.map(|c| ("verus-commit".to_string(), c)));
        }
        Err(e) => {
            return Resolution::Missing {
                reason: format!("{} --version failed: {e}", tc.dir.join("verus").display()),
                versions,
            };
        }
    }
    let z3 = Some(tc.dir.join(exe("z3"))).filter(|p| p.is_file()).or_else(|| find_on_path("z3", &env));
    if let Some(z3) = z3
        && let Ok(text) = probe(&z3, &["--version"], &env)
    {
        versions.push(("z3".to_string(), z3_version(&text)));
    }
    Resolution::Ready(Toolchain { dir: tc.dir, versions })
}

/// The toolchain directory, by the resolution order above.
pub fn locate(tool_env: &[(OsString, OsString)]) -> Option<PathBuf> {
    var(tool_env, VERUS_DIR_ENV)
        .filter(|v| !v.is_empty())
        .and_then(|v| under(Path::new(&v)))
        .or_else(|| {
            find_on_path("verus", tool_env)
                .and_then(|v| v.parent().map(Path::to_path_buf))
                .filter(|d| holds_toolchain(d))
        })
        .or_else(|| var(tool_env, "HOME").and_then(|home| cached(&Path::new(&home).join(CACHE_DIR))))
}

/// The override's value, else the process's.
fn var(tool_env: &[(OsString, OsString)], key: &str) -> Option<OsString> {
    tool_env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).or_else(|| std::env::var_os(key))
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

fn holds_toolchain(dir: &Path) -> bool {
    dir.join(exe("verus")).is_file() && dir.join(exe("cargo-verus")).is_file()
}

/// `dir` itself, or its `verus-<platform>` subdirectory, when it holds the toolchain.
fn under(dir: &Path) -> Option<PathBuf> {
    if holds_toolchain(dir) {
        return Some(dir.to_path_buf());
    }
    platform_dirs(dir).into_iter().next_back()
}

/// `verus-*` subdirectories of `dir` holding the toolchain, sorted.
fn platform_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("verus-")) && holds_toolchain(p)
        })
        .collect();
    found.sort();
    found
}

/// The newest cached release: `<cache>/<version>/verus-<platform>`.
fn cached(cache: &Path) -> Option<PathBuf> {
    let mut versions: Vec<PathBuf> = std::fs::read_dir(cache)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    versions.sort();
    versions.into_iter().rev().find_map(|v| platform_dirs(&v).into_iter().next_back())
}

fn probe(program: &Path, args: &[&'static str], env: &[(OsString, OsString)]) -> Result<String, String> {
    let mut command = Command::new(program, PROBE_TIMEOUT).envs(env);
    for a in args {
        command = command.arg(a);
    }
    let out = command.run().map_err(|e| e.to_string())?;
    if !out.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn second_word(text: &str) -> String {
    text.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("unknown").to_string()
}

/// `Z3 version 4.16.0 - 64 bit` → `4.16.0`.
fn z3_version(text: &str) -> String {
    text.lines().next().and_then(|l| l.split_whitespace().nth(2)).unwrap_or("unknown").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_words() {
        assert_eq!(second_word("cargo 1.99.1 (5f94df478 2026-08-27)\n"), "1.99.1");
        assert_eq!(z3_version("Z3 version 4.16.0 - 64 bit\n"), "4.16.0");
        assert_eq!(z3_version(""), "unknown");
    }

    #[test]
    fn an_empty_override_finds_nothing_in_an_empty_home() {
        let home = tempfile::tempdir().expect("tempdir");
        let env = vec![
            (OsString::from(VERUS_DIR_ENV), OsString::new()),
            (OsString::from("PATH"), home.path().join("nothing").into_os_string()),
            (OsString::from("HOME"), home.path().as_os_str().to_os_string()),
        ];
        assert_eq!(locate(&env), None);
        assert!(matches!(resolve(&env), Resolution::Missing { reason, .. } if reason == VERUS_HINT));
    }
}
