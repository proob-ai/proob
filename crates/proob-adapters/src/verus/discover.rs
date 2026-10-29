//! Discovery: the verified crates under the adapter root, the items their
//! tokens bind, and each crate's binding set (see the module doc of `verus`).
//! Tokens in crates that are not verified are reported as `uncovered`, so
//! the property renders `Not checked (no adapter)` rather than silently
//! `Not checked (no check)`.
use crate::rust::discover::{hash_of, resolve_mod};
use crate::rust::manifest::{CrateKind, Package, crate_roots, is_verus_verified, package_at, packages, read_manifest};
use crate::rust::scan as rust_scan;
use crate::verus::scan::{Bound, scan};
use crate::verus::{VerusAdapter, check_ref};
use crate::{AdapterError, Binding, Location, Uncovered};
use proob_core::types::Kind;
use proob_facts::ulid;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use toml::Table;

pub const TOOLCHAIN_FILES: [&str; 2] = ["rust-toolchain.toml", "rust-toolchain"];

/// A source file a crate compiles: where it is, repo-relative, and the
/// module path of the file (empty for the crate root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub abs: PathBuf,
    pub rel: PathBuf,
    pub module_path: Vec<String>,
}

/// The packages under the adapter root whose manifests say `verify = true`.
pub fn verified_packages(adapter: &VerusAdapter, repo: &Path) -> Result<Vec<Package>, AdapterError> {
    Ok(packages(&adapter.root_in(repo), adapter.root())?
        .into_iter()
        .filter(|p| is_verus_verified(&p.manifest))
        .collect())
}

pub fn discover(adapter: &VerusAdapter, repo: &Path) -> Result<Vec<Binding>, AdapterError> {
    let root = adapter.root_in(repo);
    let root_manifest = read_manifest(&root.join("Cargo.toml"))?;
    let toolchain = toolchain_file(adapter, repo);
    let mut bindings = Vec::new();
    for pkg in verified_packages(adapter, repo)? {
        let sources = verified_sources(repo, &pkg)?;
        let files = binding_files(adapter, repo, &root_manifest, &pkg, &sources, toolchain.as_deref())?;
        let binding_hash = hash_of(repo, &files)?;
        for file in &sources {
            for token in scan(&read(&file.abs)?).tokens {
                let at = Location { path: file.rel.clone(), line: token.line };
                match token.bound {
                    Bound::Item { id, item } => {
                        let module_path: Vec<String> =
                            file.module_path.iter().chain(item.module_path.iter()).cloned().collect();
                        bindings.push(Binding {
                            id,
                            kind: Kind::Proof,
                            check_ref: check_ref(&pkg.name, &module_path, &item.name),
                            files: files.clone(),
                            binding_hash,
                            token: at,
                        });
                    }
                    Bound::Problem(problem) => return Err(AdapterError::Token { at, problem }),
                }
            }
        }
    }
    check_distinct(&bindings)?;
    bindings.sort_by(|a, b| (&a.token.path, a.token.line).cmp(&(&b.token.path, b.token.line)));
    Ok(bindings)
}

/// Tokens in crates under the root that are not verified: nothing here can
/// bind them, and the reason says what would.
pub fn uncovered(adapter: &VerusAdapter, repo: &Path) -> Result<Vec<Uncovered>, AdapterError> {
    let mut found = Vec::new();
    for pkg in packages(&adapter.root_in(repo), adapter.root())?.into_iter().filter(|p| !is_verus_verified(&p.manifest))
    {
        let reason = format!(
            "`{}` is not a verified crate (no `[package.metadata.verus] verify = true` in {}), so the Verus adapter cannot bind a proof there",
            pkg.name,
            pkg.rel.join("Cargo.toml").display()
        );
        for root in crate_roots(&pkg)? {
            for file in walk(repo, &root.file)? {
                let text = read(&file.abs)?;
                found.extend(token_lines(&text).map(|(line, id)| Uncovered {
                    id,
                    token: Location { path: file.rel.clone(), line },
                    reason: reason.clone(),
                }));
            }
        }
    }
    Ok(found)
}

/// Comment-only `// proob:<ULID>` lines that decode, with their 1-based line.
fn token_lines(text: &str) -> impl Iterator<Item = (usize, u128)> + '_ {
    text.lines().enumerate().filter_map(|(i, line)| {
        let rest = line.trim().strip_prefix("//")?;
        if rest.starts_with('/') || rest.starts_with('!') {
            return None;
        }
        ulid::decode(rest.trim_start().strip_prefix("proob:")?.trim_end()).ok().map(|id| (i + 1, id))
    })
}

fn check_distinct(bindings: &[Binding]) -> Result<(), AdapterError> {
    let mut seen: BTreeMap<u128, &Location> = BTreeMap::new();
    for b in bindings {
        if let Some(first) = seen.insert(b.id, &b.token) {
            return Err(AdapterError::Duplicate {
                id: ulid::encode(b.id),
                first: first.clone(),
                second: b.token.clone(),
            });
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<String, AdapterError> {
    std::fs::read_to_string(path).map_err(|e| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() })
}

fn repo_relative(repo: &Path, abs: &Path) -> PathBuf {
    abs.strip_prefix(repo).map(Path::to_path_buf).unwrap_or_else(|_| abs.to_path_buf())
}

/// The files `cargo verus` verifies: the lib and bin roots and what their
/// `mod` declarations reach. Integration tests are plain Rust and not here.
pub fn verified_sources(repo: &Path, pkg: &Package) -> Result<Vec<SourceFile>, AdapterError> {
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut out = Vec::new();
    for root in crate_roots(pkg)?.into_iter().filter(|r| r.kind != CrateKind::Test) {
        out.extend(walk(repo, &root.file)?.into_iter().filter(|f| seen.insert(f.rel.clone())));
    }
    Ok(out)
}

/// Breadth-first over `mod` declarations from a crate root file.
fn walk(repo: &Path, root_file: &Path) -> Result<Vec<SourceFile>, AdapterError> {
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut out = Vec::new();
    let mut queue: Vec<(PathBuf, Vec<String>)> = vec![(root_file.to_path_buf(), Vec::new())];
    while let Some((abs, module_path)) = queue.pop() {
        if !seen.insert(abs.clone()) {
            continue;
        }
        let text = read(&abs)?;
        let is_root = out.is_empty();
        for decl in &rust_scan::scan(&text).mods {
            if let Some(child) = resolve_mod(&abs, is_root, decl) {
                let child_path: Vec<String> =
                    module_path.iter().chain(decl.module_path.iter()).cloned().chain([decl.name.clone()]).collect();
                queue.push((child, child_path));
            }
        }
        out.push(SourceFile { rel: repo_relative(repo, &abs), abs, module_path });
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

/// `rust-toolchain.toml` (or `rust-toolchain`) at the adapter root, else at
/// the repository root, repo-relative.
fn toolchain_file(adapter: &VerusAdapter, repo: &Path) -> Option<PathBuf> {
    TOOLCHAIN_FILES
        .iter()
        .map(|f| adapter.repo_relative(Path::new(f)))
        .chain(TOOLCHAIN_FILES.iter().map(PathBuf::from))
        .find(|rel| repo.join(rel).is_file())
}

/// The binding set of a verified crate (see the module doc of `verus`),
/// repo-relative, sorted, deduplicated.
fn binding_files(
    adapter: &VerusAdapter,
    repo: &Path,
    root_manifest: &Table,
    pkg: &Package,
    sources: &[SourceFile],
    toolchain: Option<&Path>,
) -> Result<Vec<PathBuf>, AdapterError> {
    let root = adapter.root_in(repo);
    let mut files: Vec<PathBuf> = sources.iter().map(|f| f.rel.clone()).collect();
    files.push(pkg.rel.join("Cargo.toml"));
    if root_manifest.contains_key("workspace") {
        files.push(adapter.repo_relative(Path::new("Cargo.toml")));
    }
    files.extend(toolchain.map(Path::to_path_buf));
    let mut seen: BTreeSet<PathBuf> = BTreeSet::from([pkg.dir.clone()]);
    for dep in verified_path_deps(repo, &root, root_manifest, pkg, &mut seen)? {
        files.extend(verified_sources(repo, &dep)?.into_iter().map(|f| f.rel));
        files.push(dep.rel.join("Cargo.toml"));
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Path dependencies that are themselves verified crates, transitively: the
/// trusted base a proof rests on.
fn verified_path_deps(
    repo: &Path,
    root: &Path,
    root_manifest: &Table,
    pkg: &Package,
    seen: &mut BTreeSet<PathBuf>,
) -> Result<Vec<Package>, AdapterError> {
    let mut out = Vec::new();
    let deps = pkg.manifest.get("dependencies").and_then(toml::Value::as_table).cloned().unwrap_or_default();
    for (name, spec) in &deps {
        let Some(dir) = dep_dir(root, root_manifest, &pkg.dir, name, spec) else { continue };
        if !dir.join("Cargo.toml").is_file() || !seen.insert(dir.clone()) {
            continue;
        }
        let dep = package_at(&dir, &repo_relative(repo, &dir))?;
        if is_verus_verified(&dep.manifest) {
            out.extend(verified_path_deps(repo, root, root_manifest, &dep, seen)?);
            out.push(dep);
        }
    }
    Ok(out)
}

/// The directory of a `path = "…"` dependency, or of a `workspace = true`
/// dependency whose `[workspace.dependencies]` entry has a path.
fn dep_dir(root: &Path, root_manifest: &Table, pkg_dir: &Path, name: &str, spec: &toml::Value) -> Option<PathBuf> {
    if let Some(path) = spec.get("path").and_then(toml::Value::as_str) {
        return Some(pkg_dir.join(path));
    }
    if spec.get("workspace").and_then(toml::Value::as_bool) != Some(true) {
        return None;
    }
    let path = root_manifest.get("workspace")?.get("dependencies")?.get(name)?.get("path")?.as_str()?;
    Some(root.join(path))
}
