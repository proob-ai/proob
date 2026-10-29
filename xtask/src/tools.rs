//! Where the tools that are not cargo subcommands live. `detect` reads PATH
//! and the filesystem once; everything else here is pure.
//!
//! The Verus pin is read from tools/get-verus.sh at build time, so it lives in
//! one place: that script, ci.yml's cache key, rust-toolchain.toml and the vstd
//! pin move together, and xtask follows without a copy of its own.

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::error::Error;

pub const GET_VERUS_SH: &str = include_str!("../../tools/get-verus.sh");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verus {
    /// `cargo-verus` is on PATH already (CI after tools/get-verus.sh, or a developer's shell).
    OnPath,
    /// The directory tools/get-verus.sh unpacked the pinned release into.
    Cached(PathBuf),
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub verus: Verus,
    pub verus_version: String,
    /// `claude` when it is on PATH, else `$HOME/.local/bin/claude` where install.sh puts it.
    pub claude: String,
}

pub fn detect() -> Result<Tools, Error> {
    let version = pinned_version(GET_VERUS_SH).ok_or(Error::VerusPin)?.to_owned();
    let path = env::var_os("PATH").unwrap_or_default();
    let home = env::var_os("HOME").map(PathBuf::from);
    let override_dir = env::var_os("PROOB_VERUS_DIR").map(PathBuf::from);
    let platform = platform(env::consts::OS, env::consts::ARCH);
    let cache = verus_dir(home.as_deref(), override_dir.as_deref(), platform, &version);
    Ok(Tools {
        verus: detect_verus(&path, cache),
        verus_version: version,
        claude: detect_claude(&path, home.as_deref()),
    })
}

fn detect_verus(path: &OsStr, cache: Option<PathBuf>) -> Verus {
    match (find_on_path("cargo-verus", path), cache) {
        (Some(_), _) => Verus::OnPath,
        (None, Some(dir)) if dir.join("cargo-verus").is_file() && dir.join("verus").is_file() => Verus::Cached(dir),
        (None, _) => Verus::Missing,
    }
}

fn detect_claude(path: &OsStr, home: Option<&Path>) -> String {
    match (find_on_path("claude", path), home) {
        (Some(_), _) | (None, None) => "claude".to_owned(),
        (None, Some(home)) => home.join(".local/bin/claude").to_string_lossy().into_owned(),
    }
}

pub fn find_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    env::split_paths(path).map(|dir| dir.join(name)).find(|candidate| candidate.is_file())
}

/// Where tools/get-verus.sh unpacks the release: `$PROOB_VERUS_DIR`, else
/// `$HOME/.proob-dev/verus/<version>`, then `verus-<platform>/` inside it.
pub fn verus_dir(
    home: Option<&Path>,
    override_dir: Option<&Path>,
    platform: Option<&str>,
    version: &str,
) -> Option<PathBuf> {
    let base =
        override_dir.map(Path::to_path_buf).or_else(|| home.map(|home| home.join(".proob-dev/verus").join(version)))?;
    Some(base.join(format!("verus-{}", platform?)))
}

/// The release asset suffix tools/get-verus.sh picks for this host.
pub fn platform(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("arm64-macos"),
        ("macos", "x86_64") => Some("x86-macos"),
        ("linux", "x86_64") => Some("x86-linux"),
        ("linux", "aarch64") => Some("arm64-linux"),
        _ => None,
    }
}

/// The `readonly VERUS_VERSION="…"` line of tools/get-verus.sh.
pub fn pinned_version(script: &str) -> Option<&str> {
    script
        .lines()
        .find_map(|line| line.strip_prefix("readonly VERUS_VERSION=\""))
        .and_then(|rest| rest.strip_suffix('"'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_is_read_from_the_real_script() {
        let version = pinned_version(GET_VERUS_SH).expect("tools/get-verus.sh pins VERUS_VERSION");
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(parts.len(), 5, "release/<major>.<yyyy>.<mm>.<dd>.<sha>: {version}");
        assert!(parts[..4].iter().all(|p| p.chars().all(|c| c.is_ascii_digit())), "{version}");
    }

    #[test]
    fn the_pin_parser_wants_the_exact_line() {
        assert_eq!(pinned_version("x\nreadonly VERUS_VERSION=\"0.1.2.3.abc\"\ny"), Some("0.1.2.3.abc"));
        assert_eq!(pinned_version("VERUS_VERSION=\"0.1\""), None);
        assert_eq!(pinned_version("readonly VERUS_VERSION=\"unterminated"), None);
    }

    #[test]
    fn platforms_match_get_verus_sh() {
        assert_eq!(platform("macos", "aarch64"), Some("arm64-macos"));
        assert_eq!(platform("macos", "x86_64"), Some("x86-macos"));
        assert_eq!(platform("linux", "x86_64"), Some("x86-linux"));
        assert_eq!(platform("linux", "aarch64"), Some("arm64-linux"));
        assert_eq!(platform("windows", "x86_64"), None);
    }

    #[test]
    fn the_cache_directory_follows_get_verus_sh() {
        let home = Path::new("/home/me");
        assert_eq!(
            verus_dir(Some(home), None, Some("x86-linux"), "0.1"),
            Some(PathBuf::from("/home/me/.proob-dev/verus/0.1/verus-x86-linux"))
        );
        assert_eq!(
            verus_dir(Some(home), Some(Path::new("/opt/verus")), Some("arm64-macos"), "0.1"),
            Some(PathBuf::from("/opt/verus/verus-arm64-macos"))
        );
        assert_eq!(verus_dir(None, None, Some("x86-linux"), "0.1"), None, "no HOME, no override");
        assert_eq!(verus_dir(Some(home), None, None, "0.1"), None, "unsupported platform");
    }

    #[test]
    fn claude_falls_back_to_the_installer_location() {
        let empty = OsStr::new("");
        assert_eq!(detect_claude(empty, Some(Path::new("/home/me"))), "/home/me/.local/bin/claude");
        assert_eq!(detect_claude(empty, None), "claude");
    }

    #[test]
    fn verus_is_missing_when_neither_path_nor_cache_has_it() {
        assert_eq!(detect_verus(OsStr::new(""), None), Verus::Missing);
        assert_eq!(detect_verus(OsStr::new(""), Some(PathBuf::from("/nonexistent/verus"))), Verus::Missing);
    }
}
