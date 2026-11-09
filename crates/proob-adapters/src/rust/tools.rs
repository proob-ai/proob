//! The tools the Rust adapter drives, resolved from `PATH` (proob's own tool
//! directory `~/.proob/tools/rust/bin` first, when it exists) and version-probed
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

/// proob's own cargo tools: `cargo install --root` puts binaries in `bin/`
/// here, never in the user's `~/.cargo/bin`, so a pinned version replaces none of theirs.
pub const OWN_RUST_TOOLS: &str = ".proob/tools/rust";
/// The Kani bundle's home (`KANI_HOME`) when proob installed it.
pub const OWN_KANI_HOME: &str = ".proob/tools/kani";

/// The production tool environment: `PATH` with proob's tool directory first,
/// and `KANI_HOME` at proob's bundle, each only when it exists and the user
/// has not set `KANI_HOME` themselves. Empty when proob installed nothing.
pub fn own_tools_env() -> Vec<(OsString, OsString)> {
    own_tools_env_in(std::env::var_os("HOME"), std::env::var_os("PATH"), std::env::var_os("KANI_HOME").is_some())
}

fn own_tools_env_in(home: Option<OsString>, path: Option<OsString>, kani_home_set: bool) -> Vec<(OsString, OsString)> {
    let Some(home) = home.map(PathBuf::from) else { return Vec::new() };
    let bin = home.join(OWN_RUST_TOOLS).join("bin");
    let path = bin.is_dir().then(|| {
        let rest = path.map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
        std::env::join_paths(std::iter::once(bin).chain(rest)).ok()
    });
    let kani = home.join(OWN_KANI_HOME);
    let kani = (!kani_home_set && kani.is_dir()).then_some(kani.into_os_string());
    path.flatten()
        .map(|p| (OsString::from("PATH"), p))
        .into_iter()
        .chain(kani.map(|k| (OsString::from("KANI_HOME"), k)))
        .collect()
}

pub const CARGO_HINT: &str =
    "cargo was not found on PATH: install Rust with rustup (https://rustup.rs) so `cargo` is available";
pub const NEXTEST_HINT: &str = "cargo-nextest is not installed: `cargo install --locked --root $HOME/.proob/tools/rust cargo-nextest --version 0.9.150` (proob looks in $HOME/.proob/tools/rust/bin first, so your own ~/.cargo/bin is left as it is; in CI: taiki-e/install-action with tool: cargo-nextest@0.9.150)";
pub const MUTANTS_HINT: &str = "cargo-mutants is not installed: `cargo install --locked --root $HOME/.proob/tools/rust cargo-mutants --version 27.1.0` (proob looks in $HOME/.proob/tools/rust/bin first, so your own ~/.cargo/bin is left as it is; in CI: taiki-e/install-action with tool: cargo-mutants@27.1.0)";

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

#[cfg(test)]
mod tests {
    use super::*;

    /// proob's directories steer the tools only once they exist, first on
    /// `PATH`, and never over a `KANI_HOME` the user set.
    #[test]
    fn proobs_own_tool_directories_come_first_only_when_they_exist() {
        let home = tempfile::tempdir().expect("tempdir");
        let h = || Some(home.path().as_os_str().to_owned());
        assert_eq!(own_tools_env_in(h(), Some("/usr/bin".into()), false), Vec::new());
        let bin = home.path().join(OWN_RUST_TOOLS).join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        std::fs::create_dir_all(home.path().join(OWN_KANI_HOME)).expect("mkdir");
        let env = own_tools_env_in(h(), Some("/usr/bin".into()), false);
        let path = std::env::join_paths([bin.clone(), PathBuf::from("/usr/bin")]).expect("join");
        assert_eq!(
            env,
            vec![
                (OsString::from("PATH"), path),
                (OsString::from("KANI_HOME"), home.path().join(OWN_KANI_HOME).into_os_string())
            ]
        );
        assert_eq!(own_tools_env_in(h(), None, true), vec![(OsString::from("PATH"), bin.into_os_string())]);
        assert_eq!(own_tools_env_in(None, None, false), Vec::new());
    }
}
