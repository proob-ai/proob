//! Discovery: walk each crate from its root through `mod` declarations,
//! collect the bound tests, and compute every binding's set and hash.
//! Tokens above Kani harnesses are not this adapter's: discovery skips them
//! and `uncovered` reports them with what would bind them.
use crate::rust::manifest::{CrateKind, CrateRoot, Package, crate_roots, is_verus_verified, packages};
use crate::rust::scan::{Bound, Kani, ModDecl, scan};
use crate::rust::{RustAdapter, check_ref};
use crate::{AdapterError, Binding, Location, Uncovered};
use proob_core::binding_hash::binding_hash;
use proob_core::scope_hash::PathDigest;
use proob_core::types::Kind;
use proob_facts::property::file_digest;
use proob_facts::ulid;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Config files at the adapter root that shape how a bound test runs.
pub const ROOT_CONFIG_FILES: [&str; 2] = [".config/nextest.toml", ".cargo/mutants.toml"];

#[derive(Debug)]
pub(crate) struct FileNode {
    pub(crate) rel: PathBuf,
    in_test_tree: bool,
    /// Reached through a `mod` under `#[cfg(kani)]`.
    pub(crate) in_kani_tree: bool,
    children: Vec<usize>,
}

/// A token above a Kani harness.
#[derive(Debug)]
pub(crate) struct Harness {
    pub(crate) file: usize,
    pub(crate) line: usize,
    pub(crate) id: u128,
    pub(crate) marker: Kani,
    /// The harness's module path inside the crate and its name.
    pub(crate) path: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct Crate {
    pub(crate) kind: CrateKind,
    binary_id: String,
    pub(crate) files: Vec<FileNode>,
    /// (file index, token line, id, test path)
    tests: Vec<(usize, usize, u128, String)>,
    pub(crate) harnesses: Vec<Harness>,
}

pub fn discover(adapter: &RustAdapter, repo: &Path) -> Result<Vec<Binding>, AdapterError> {
    let root = adapter.root_in(repo);
    let config_files: Vec<PathBuf> = ROOT_CONFIG_FILES
        .iter()
        .filter(|f| root.join(f).is_file())
        .map(|f| adapter.repo_relative(Path::new(f)))
        .collect();
    let mut bindings: Vec<Binding> = Vec::new();
    for (_, krate) in crates(&adapter.root_in(repo), adapter.root(), repo)? {
        bindings.extend(bindings_of(repo, &krate, &config_files)?);
    }
    check_distinct(&bindings)?;
    bindings.sort_by(|a, b| (&a.token.path, a.token.line).cmp(&(&b.token.path, b.token.line)));
    Ok(bindings)
}

/// Tokens above Kani harnesses: no Rust test can bind them, and the reason
/// says what would.
pub fn uncovered(adapter: &RustAdapter, repo: &Path) -> Result<Vec<Uncovered>, AdapterError> {
    Ok(crates(&adapter.root_in(repo), adapter.root(), repo)?
        .into_iter()
        .flat_map(|(_, k)| k.harnesses.into_iter().map(move |h| (k.files[h.file].rel.clone(), h)))
        .map(|(path, h)| Uncovered {
            id: h.id,
            token: Location { path, line: h.line },
            reason: match h.marker {
                Kani::Proof => "a Kani harness; add a `kani` adapter (`proob init --kani`)".to_string(),
                Kani::Contract => "a Kani contract harness; Kani contracts are not supported yet".to_string(),
            },
        })
        .collect())
}

/// Every crate under the adapter root (absolute `root`, repo-relative
/// `root_rel`) the Rust and Kani adapters scan, walked, with its package.
pub(crate) fn crates(root: &Path, root_rel: &Path, repo: &Path) -> Result<Vec<(Package, Crate)>, AdapterError> {
    let mut out = Vec::new();
    for pkg in packages(root, root_rel)? {
        // A Verus-verified crate's lib and bin roots hold proofs, not tests:
        // the Verus adapter reads them. Its integration tests stay here.
        let verified = is_verus_verified(&pkg.manifest);
        for crate_root in crate_roots(&pkg)?.into_iter().filter(|r| !verified || r.kind == CrateKind::Test) {
            out.push((pkg.clone(), walk(repo, &crate_root)?));
        }
    }
    Ok(out)
}

pub(crate) fn check_distinct(bindings: &[Binding]) -> Result<(), AdapterError> {
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

/// Breadth-first over `mod` declarations from the crate root.
fn walk(repo: &Path, root: &CrateRoot) -> Result<Crate, AdapterError> {
    let is_test_crate = root.kind == CrateKind::Test;
    let mut krate = Crate {
        kind: root.kind,
        binary_id: root.binary_id.clone(),
        files: Vec::new(),
        tests: Vec::new(),
        harnesses: Vec::new(),
    };
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    // (file, module path, (in the cfg(test) tree, in the cfg(kani) tree), parent)
    type Queued = (PathBuf, Vec<String>, (bool, bool), Option<usize>);
    let mut queue: Vec<Queued> = vec![(root.file.clone(), Vec::new(), (is_test_crate, false), None)];
    while let Some((abs, module_path, (in_test_tree, in_kani_tree), parent)) = queue.pop() {
        if !seen.insert(abs.clone()) {
            continue;
        }
        let index = krate.files.len();
        let text = read(&abs)?;
        let scanned = scan(&text);
        let rel = repo_relative(repo, &abs);
        if let Some(p) = parent {
            krate.files[p].children.push(index);
        }
        for token in &scanned.tokens {
            match &token.bound {
                Bound::Test { id, function, module_path: inline } => {
                    let path: Vec<&str> = module_path
                        .iter()
                        .chain(inline.iter())
                        .map(String::as_str)
                        .chain([function.as_str()])
                        .collect();
                    krate.tests.push((index, token.line, *id, path.join("::")));
                }
                Bound::Kani { id, harness, function, module_path: inline } => {
                    let path = module_path.iter().chain(inline).chain([function]).cloned().collect();
                    krate.harnesses.push(Harness { file: index, line: token.line, id: *id, marker: *harness, path });
                }
                Bound::Problem(problem) => {
                    return Err(AdapterError::Token {
                        at: Location { path: rel, line: token.line },
                        problem: problem.clone(),
                    });
                }
            }
        }
        let is_root = index == 0;
        for decl in &scanned.mods {
            if let Some(child) = resolve_mod(&abs, is_root, decl) {
                let mut child_path = module_path.clone();
                child_path.extend(decl.module_path.iter().cloned());
                child_path.push(decl.name.clone());
                let trees = (in_test_tree || decl.cfg_test, in_kani_tree || decl.cfg_kani);
                queue.push((child, child_path, trees, Some(index)));
            }
        }
        krate.files.push(FileNode { rel, in_test_tree, in_kani_tree, children: Vec::new() });
    }
    Ok(krate)
}

/// The file a `mod name;` names, if it exists.
pub(crate) fn resolve_mod(file: &Path, is_crate_root: bool, decl: &ModDecl) -> Option<PathBuf> {
    let parent = file.parent()?;
    if let Some(explicit) = &decl.path_attr {
        let candidate = parent.join(explicit);
        return candidate.is_file().then_some(candidate);
    }
    let is_mod_rs = file.file_name().is_some_and(|n| n == "mod.rs");
    let base = if is_crate_root || is_mod_rs { parent.to_path_buf() } else { parent.join(file.file_stem()?) };
    let dir = decl.module_path.iter().fold(base, |d, seg| d.join(seg));
    [dir.join(format!("{}.rs", decl.name)), dir.join(&decl.name).join("mod.rs")].into_iter().find(|c| c.is_file())
}

/// The files that give a test in `file` its meaning (see the module doc).
fn binding_files(krate: &Crate, file: usize) -> Vec<usize> {
    match krate.kind {
        CrateKind::Test => (0..krate.files.len()).collect(),
        CrateKind::Lib | CrateKind::Bin => subtree(krate, file, |f| f.in_test_tree),
    }
}

/// `file` and the files reached from it through `mod` declarations whose
/// targets satisfy `member`.
pub(crate) fn subtree(krate: &Crate, file: usize, member: impl Fn(&FileNode) -> bool) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![file];
    while let Some(i) = stack.pop() {
        out.push(i);
        stack.extend(krate.files[i].children.iter().copied().filter(|c| member(&krate.files[*c])));
    }
    out
}

fn bindings_of(repo: &Path, krate: &Crate, config_files: &[PathBuf]) -> Result<Vec<Binding>, AdapterError> {
    krate
        .tests
        .iter()
        .map(|(file, line, id, test_path)| {
            let mut files: Vec<PathBuf> =
                binding_files(krate, *file).into_iter().map(|i| krate.files[i].rel.clone()).collect();
            files.extend(config_files.iter().cloned());
            files.sort();
            files.dedup();
            let binding_hash = hash_of(repo, &files)?;
            Ok(Binding {
                id: *id,
                kind: Kind::Test,
                check_ref: check_ref(&krate.binary_id, test_path),
                files,
                binding_hash,
                token: Location { path: krate.files[*file].rel.clone(), line: *line },
            })
        })
        .collect()
}

/// `proob-binding-v1` over the normalised content of repo-relative `files`.
pub fn hash_of(repo: &Path, files: &[PathBuf]) -> Result<[u8; 32], AdapterError> {
    let pairs = files
        .iter()
        .map(|rel| {
            let bytes = std::fs::read(repo.join(rel))
                .map_err(|e| AdapterError::Io { path: repo.join(rel), message: e.to_string() })?;
            Ok(PathDigest { path: rel.to_string_lossy().as_bytes().to_vec(), digest: file_digest(&bytes) })
        })
        .collect::<Result<Vec<_>, AdapterError>>()?;
    binding_hash(pairs).ok_or_else(|| AdapterError::Binding {
        check_ref: String::new(),
        message: "a binding file was listed twice".into(),
    })
}
