//! The Kani toolchain the adapter drives: `cargo-kani` from `PATH` and the
//! bundle `cargo kani setup` unpacks (CBMC, Kani's compiler, its nightly)
//! under `KANI_HOME`, else `~/.kani`, as `kani-<version>/`. Both are located
//! before any run, the bundle first so a missing one is reported rather than
//! fetched mid-check, and `cargo kani --version` must match the pin in
//! `tools/kani.toml`: the output the parser reads is that release's. Every
//! result records `kani`, `cbmc`, and `nightly` in `tool_versions`.
use crate::ToolVersions;
use crate::argv::Command;
use crate::rust::tools::find_on_path;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// `tools/kani.toml`: the one place the Kani version and its nightly are written.
pub const PIN_TEXT: &str = include_str!("../../../../tools/kani.toml");
pub const KANI_HOME_ENV: &str = "KANI_HOME";
/// Under `HOME` when `KANI_HOME` is unset.
pub const DEFAULT_HOME: &str = ".kani";
const PROBE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub version: String,
    pub nightly: String,
}

/// The embedded pin; `tools/kani.toml` is checked by this crate's tests.
pub static PIN: LazyLock<Pin> = LazyLock::new(|| {
    let table: toml::Table = PIN_TEXT.parse().expect("tools/kani.toml is TOML");
    let field = |key: &str| table.get(key).and_then(toml::Value::as_str).expect("tools/kani.toml names it").to_string();
    Pin { version: field("version"), nightly: field("nightly") }
});

/// The reason a run cannot start without Kani; it begins with the stable
/// phrase "the Kani toolchain was not found".
pub fn hint(detail: &str) -> String {
    format!(
        "the Kani toolchain was not found ({detail}): install Kani {v} into proob's own directories, which proob looks in first and which leave your ~/.cargo/bin and ~/.kani as they are: `cargo install --locked --root $HOME/.proob/tools/rust kani-verifier --version {v} && PATH=$HOME/.proob/tools/rust/bin:$PATH KANI_HOME=$HOME/.proob/tools/kani cargo kani setup`",
        v = PIN.version
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    pub cargo_kani: PathBuf,
    pub versions: ToolVersions,
}

impl Toolchain {
    /// The environment every tool process runs in: the override, colours off.
    pub fn env(tool_env: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
        tool_env.iter().cloned().chain([(OsString::from("CARGO_TERM_COLOR"), OsString::from("never"))]).collect()
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

pub fn resolve(tool_env: &[(OsString, OsString)]) -> Resolution {
    let missing = |reason: String| Resolution::Missing { reason, versions: Vec::new() };
    let Some(cargo_kani) = find_on_path("cargo-kani", tool_env) else {
        return missing(hint("no `cargo-kani` on PATH"));
    };
    let Some(bundle) = bundle_dir(tool_env).filter(|d| d.is_dir()) else {
        return missing(hint(&format!("no Kani {} bundle; `cargo kani setup` unpacks it", PIN.version)));
    };
    let env = Toolchain::env(tool_env);
    let text = match probe(&cargo_kani, &env) {
        Ok(text) => text,
        Err(e) => return missing(format!("cargo kani --version failed: {e}")),
    };
    let (kani, cbmc) = versions(&text);
    if kani != PIN.version {
        return Resolution::Missing {
            reason: format!(
                "cargo-kani {kani} is installed but this proob drives Kani {v} exactly (its output is the contract the adapter reads): `cargo install --locked --root $HOME/.proob/tools/rust kani-verifier --version {v} && PATH=$HOME/.proob/tools/rust/bin:$PATH KANI_HOME=$HOME/.proob/tools/kani cargo kani setup` (proob looks there first, so your own install is left as it is)",
                v = PIN.version
            ),
            versions: vec![("kani".to_string(), kani)],
        };
    }
    let nightly = std::fs::read_to_string(bundle.join("rust-toolchain-version")).map(|t| nightly_of(&t)).ok();
    let versions = [("kani", Some(kani)), ("cbmc", cbmc), ("nightly", nightly)]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k.to_string(), v)))
        .collect();
    Resolution::Ready(Toolchain { cargo_kani, versions })
}

/// `$KANI_HOME/kani-<pin>`, else `$HOME/.kani/kani-<pin>`.
pub fn bundle_dir(tool_env: &[(OsString, OsString)]) -> Option<PathBuf> {
    let base = var(tool_env, KANI_HOME_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| var(tool_env, "HOME").map(|h| Path::new(&h).join(DEFAULT_HOME)))?;
    Some(base.join(format!("kani-{}", PIN.version)))
}

/// The override's value, else the process's.
pub(crate) fn var(tool_env: &[(OsString, OsString)], key: &str) -> Option<OsString> {
    tool_env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).or_else(|| std::env::var_os(key))
}

fn probe(cargo_kani: &Path, env: &[(OsString, OsString)]) -> Result<String, String> {
    let out = Command::new(cargo_kani, PROBE_TIMEOUT)
        .arg("kani")
        .arg("--version")
        .envs(env)
        .run()
        .map_err(|e| e.to_string())?;
    if !out.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `Kani Rust Verifier 0.68.0 (cargo plugin)` and `CBMC 6.11.0`.
pub fn versions(text: &str) -> (String, Option<String>) {
    let word = |prefix: &str| {
        text.lines().find_map(|l| l.strip_prefix(prefix)).and_then(|r| r.split_whitespace().next()).map(str::to_string)
    };
    (word("Kani Rust Verifier ").unwrap_or_else(|| "unknown".to_string()), word("CBMC "))
}

/// `nightly-2026-08-21-aarch64-apple-darwin` → `nightly-2026-08-21`.
fn nightly_of(text: &str) -> String {
    let t = text.trim();
    t.split('-').take(4).collect::<Vec<_>>().join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_parses() {
        assert_eq!(PIN.version, "0.68.0");
        assert_eq!(PIN.nightly, "nightly-2026-08-21");
    }

    #[test]
    fn version_lines() {
        let (kani, cbmc) = versions("Kani Rust Verifier 0.68.0 (cargo plugin)\nCBMC 6.11.0\n");
        assert_eq!((kani.as_str(), cbmc.as_deref()), ("0.68.0", Some("6.11.0")));
        assert_eq!(versions("").0, "unknown");
        assert_eq!(nightly_of("nightly-2026-08-21-aarch64-apple-darwin\n"), "nightly-2026-08-21");
    }

    #[test]
    fn nothing_on_path_is_missing_with_the_stable_hint() {
        let home = tempfile::tempdir().expect("tempdir");
        let env = vec![
            (OsString::from("PATH"), home.path().join("nothing").into_os_string()),
            (OsString::from("HOME"), home.path().as_os_str().to_os_string()),
            (OsString::from(KANI_HOME_ENV), OsString::new()),
        ];
        let Resolution::Missing { reason, .. } = resolve(&env) else { panic!("resolved") };
        assert!(reason.starts_with("the Kani toolchain was not found"), "{reason}");
        assert!(
            reason.contains("cargo install --locked --root $HOME/.proob/tools/rust kani-verifier --version 0.68.0"),
            "{reason}"
        );
        assert!(reason.contains("cargo kani setup"), "{reason}");
    }
}
