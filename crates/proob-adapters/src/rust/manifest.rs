//! Packages and crate roots from `Cargo.toml`, read statically: no `cargo
//! metadata`, so discovery needs no toolchain and never starts a process.
//!
//! A root manifest is a package, a workspace (`members` globs with one `*`
//! segment at most, minus `exclude`), or both. Per package: the lib root
//! (`[lib] path` or `src/lib.rs`), bin roots (`src/main.rs`, `src/bin/*.rs`,
//! `src/bin/*/main.rs`, `[[bin]]` entries), and test roots (`tests/*.rs`,
//! `tests/*/main.rs`, `[[test]]` entries), honouring `autobins` and
//! `autotests`. Binary ids follow nextest: `pkg` for the lib, `pkg::bin/name`
//! for a bin, `pkg::name` for an integration test.
use crate::AdapterError;
use std::path::{Path, PathBuf};
use toml::Table;

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub name: String,
    /// Absolute directory.
    pub dir: PathBuf,
    /// Repo-relative directory (empty for the repository root).
    pub rel: PathBuf,
    pub manifest: Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateKind {
    Lib,
    Bin,
    Test,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateRoot {
    pub binary_id: String,
    pub kind: CrateKind,
    /// Absolute path of the crate's root file.
    pub file: PathBuf,
}

pub(crate) fn read_manifest(path: &Path) -> Result<Table, AdapterError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() })?;
    text.parse::<Table>().map_err(|e| AdapterError::Manifest { path: path.to_path_buf(), message: e.to_string() })
}

fn package_name(manifest: &Table) -> Option<String> {
    manifest.get("package")?.get("name")?.as_str().map(str::to_string)
}

fn strings(manifest: &Table, table: &str, key: &str) -> Vec<String> {
    manifest
        .get(table)
        .and_then(|t| t.get(key))
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// `[package.metadata.verus] verify = true`: `cargo verus` verifies the
/// crate's lib and bin targets, which makes them the Verus adapter's; its
/// integration tests are plain Rust and stay the Rust adapter's.
pub fn is_verus_verified(manifest: &Table) -> bool {
    manifest
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("verus"))
        .and_then(|v| v.get("verify"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

/// The package whose manifest is `dir/Cargo.toml`, with repo-relative `rel`.
pub fn package_at(dir: &Path, rel: &Path) -> Result<Package, AdapterError> {
    let manifest = read_manifest(&dir.join("Cargo.toml"))?;
    let name = package_name(&manifest).ok_or_else(|| AdapterError::Manifest {
        path: dir.join("Cargo.toml"),
        message: "no [package] name".to_string(),
    })?;
    Ok(Package { name, dir: dir.to_path_buf(), rel: rel.to_path_buf(), manifest })
}

/// Every package under `root` (absolute, repo-relative `root_rel`), sorted by directory.
pub fn packages(root: &Path, root_rel: &Path) -> Result<Vec<Package>, AdapterError> {
    let manifest = read_manifest(&root.join("Cargo.toml"))?;
    let mut dirs: Vec<PathBuf> = Vec::new();
    if package_name(&manifest).is_some() {
        dirs.push(PathBuf::new());
    }
    let excluded = strings(&manifest, "workspace", "exclude");
    for member in strings(&manifest, "workspace", "members") {
        dirs.extend(expand_member(root, &member)?.into_iter().filter(|d| !excluded.iter().any(|e| Path::new(e) == d)));
    }
    dirs.sort();
    dirs.dedup();
    dirs.into_iter()
        .map(|rel| {
            let dir = root.join(&rel);
            let manifest =
                if rel.as_os_str().is_empty() { Ok(manifest.clone()) } else { read_manifest(&dir.join("Cargo.toml")) }?;
            let name = package_name(&manifest).ok_or_else(|| AdapterError::Manifest {
                path: dir.join("Cargo.toml"),
                message: "no [package] name".to_string(),
            })?;
            Ok(Package { name, rel: root_rel.join(&rel), dir, manifest })
        })
        .collect()
}

/// A member pattern expanded against the file system: `*` matches within one
/// segment; only directories holding a `Cargo.toml` count.
fn expand_member(root: &Path, pattern: &str) -> Result<Vec<PathBuf>, AdapterError> {
    let mut found = vec![PathBuf::new()];
    for segment in pattern.split('/').filter(|s| !s.is_empty() && *s != ".") {
        let mut next = Vec::new();
        for base in &found {
            if segment.contains('*') {
                next.extend(
                    subdirs(&root.join(base))?
                        .into_iter()
                        .filter(|n| segment_matches(segment, n))
                        .map(|n| base.join(n)),
                );
            } else {
                next.push(base.join(segment));
            }
        }
        found = next;
    }
    Ok(found.into_iter().filter(|d| root.join(d).join("Cargo.toml").is_file()).collect())
}

fn subdirs(dir: &Path) -> Result<Vec<String>, AdapterError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(AdapterError::Io { path: dir.to_path_buf(), message: e.to_string() }),
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

/// `*` within one segment, any number of times.
fn segment_matches(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let Some((first, rest)) = parts.split_first() else { return pattern == name };
    if !name.starts_with(first) {
        return false;
    }
    let mut at = first.len();
    for (i, part) in rest.iter().enumerate() {
        let last = i + 1 == rest.len();
        if last {
            return name.len() >= at && name[at..].ends_with(part);
        }
        match name[at..].find(part) {
            Some(i) => at += i + part.len(),
            None => return false,
        }
    }
    true
}

/// The crate roots of a package, deduplicated by file.
pub fn crate_roots(pkg: &Package) -> Result<Vec<CrateRoot>, AdapterError> {
    let mut roots = Vec::new();
    roots.extend(lib_root(pkg));
    roots.extend(bin_roots(pkg)?);
    roots.extend(test_roots(pkg)?);
    let mut seen = std::collections::BTreeSet::new();
    Ok(roots.into_iter().filter(|r| seen.insert(r.file.clone())).collect())
}

fn target_path(entry: &toml::Value, default: PathBuf) -> PathBuf {
    entry.get("path").and_then(|p| p.as_str()).map_or(default, PathBuf::from)
}

fn target_name(entry: &toml::Value) -> Option<String> {
    entry.get("name").and_then(|n| n.as_str()).map(str::to_string)
}

fn auto(pkg: &Package, key: &str) -> bool {
    pkg.manifest.get("package").and_then(|p| p.get(key)).and_then(|v| v.as_bool()).unwrap_or(true)
}

fn lib_root(pkg: &Package) -> Option<CrateRoot> {
    let path = pkg
        .manifest
        .get("lib")
        .map_or_else(|| PathBuf::from("src/lib.rs"), |l| target_path(l, PathBuf::from("src/lib.rs")));
    let file = pkg.dir.join(path);
    file.is_file().then(|| CrateRoot { binary_id: pkg.name.clone(), kind: CrateKind::Lib, file })
}

fn bin_roots(pkg: &Package) -> Result<Vec<CrateRoot>, AdapterError> {
    let mut inferred: Vec<(String, PathBuf)> = Vec::new();
    if auto(pkg, "autobins") {
        let main = pkg.dir.join("src/main.rs");
        if main.is_file() {
            inferred.push((pkg.name.clone(), main));
        }
        inferred.extend(auto_targets(&pkg.dir.join("src/bin"))?);
    }
    let declared = declared_targets(pkg, "bin", "src/bin");
    Ok(overriding(declared, inferred)
        .into_iter()
        .filter(|(_, f)| f.is_file())
        .map(|(name, file)| CrateRoot { binary_id: format!("{}::bin/{name}", pkg.name), kind: CrateKind::Bin, file })
        .collect())
}

fn test_roots(pkg: &Package) -> Result<Vec<CrateRoot>, AdapterError> {
    let inferred = if auto(pkg, "autotests") { auto_targets(&pkg.dir.join("tests"))? } else { Vec::new() };
    let declared = declared_targets(pkg, "test", "tests");
    Ok(overriding(declared, inferred)
        .into_iter()
        .filter(|(_, f)| f.is_file())
        .map(|(name, file)| CrateRoot { binary_id: format!("{}::{name}", pkg.name), kind: CrateKind::Test, file })
        .collect())
}

/// The `[[<table>]]` targets, a missing `path` defaulting to `<dir>/<name>.rs`.
fn declared_targets(pkg: &Package, table: &str, dir: &str) -> Vec<(String, PathBuf)> {
    pkg.manifest
        .get(table)
        .and_then(|b| b.as_array())
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = target_name(entry)?;
            let file = pkg.dir.join(target_path(entry, pkg.dir.join(format!("{dir}/{name}.rs"))));
            Some((name, file))
        })
        .collect()
}

/// The declared targets, then the inferred ones no declared target names by
/// path: as in cargo, `[[test]] name = "tests" path = "tests/integration/main.rs"`
/// replaces the inferred `integration`, so its binary is `tests`.
fn overriding(declared: Vec<(String, PathBuf)>, inferred: Vec<(String, PathBuf)>) -> Vec<(String, PathBuf)> {
    let kept: Vec<(String, PathBuf)> =
        inferred.into_iter().filter(|(_, file)| !declared.iter().any(|(_, d)| d == file)).collect();
    declared.into_iter().chain(kept).collect()
}

/// `<dir>/<name>.rs` and `<dir>/<name>/main.rs`, sorted by name.
fn auto_targets(dir: &Path) -> Result<Vec<(String, PathBuf)>, AdapterError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(AdapterError::Io { path: dir.to_path_buf(), message: e.to_string() }),
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().into_string().ok()?;
            if path.is_dir() && path.join("main.rs").is_file() {
                Some((name, path.join("main.rs")))
            } else if path.is_file() {
                name.strip_suffix(".rs").map(|stem| (stem.to_string(), path.clone()))
            } else {
                None
            }
        })
        .collect();
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_target_replaces_the_inferred_one_at_its_path() {
        let p = |s: &str| PathBuf::from(s);
        let declared = vec![("tests".to_string(), p("tests/integration/main.rs"))];
        let inferred = vec![
            ("integration".to_string(), p("tests/integration/main.rs")),
            ("other".to_string(), p("tests/other.rs")),
        ];
        assert_eq!(
            overriding(declared, inferred),
            vec![("tests".to_string(), p("tests/integration/main.rs")), ("other".to_string(), p("tests/other.rs"))]
        );
    }

    #[test]
    fn segment_globs() {
        assert!(segment_matches("*", "anything"));
        assert!(segment_matches("proob-*", "proob-core"));
        assert!(!segment_matches("proob-*", "core"));
        assert!(segment_matches("*-core", "proob-core"));
        assert!(segment_matches("a*b*c", "axxbyyc"));
        assert!(!segment_matches("a*b*c", "axxbyy"));
        assert!(segment_matches("exact", "exact"));
    }
}
