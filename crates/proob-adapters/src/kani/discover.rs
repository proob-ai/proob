//! Discovery: the Kani harnesses the Rust scanner finds in library and binary
//! crates under the adapter root, each bound as a proof with its binding set
//! (see the module doc of `kani`). Contract harnesses and harnesses in
//! integration tests are reported `uncovered` with why.
use crate::kani::{KaniAdapter, check_ref};
use crate::rust::discover::{check_distinct, crates, hash_of, subtree};
use crate::rust::manifest::{CrateKind, read_manifest};
use crate::rust::scan::Kani;
use crate::{AdapterError, Binding, Location, Uncovered};
use proob_core::types::Kind;
use std::path::{Path, PathBuf};

pub fn discover(adapter: &KaniAdapter, repo: &Path) -> Result<Vec<Binding>, AdapterError> {
    let root = adapter.root_in(repo);
    let shared = shared_files(adapter, repo)?;
    let mut bindings = Vec::new();
    for (pkg, krate) in crates(&root, adapter.root(), repo)?.iter().filter(|(_, k)| k.kind != CrateKind::Test) {
        for h in krate.harnesses.iter().filter(|h| h.marker == Kani::Proof) {
            let mut files: Vec<PathBuf> =
                subtree(krate, h.file, |f| f.in_kani_tree).into_iter().map(|i| krate.files[i].rel.clone()).collect();
            files.push(pkg.rel.join("Cargo.toml"));
            files.extend(shared.iter().cloned());
            files.sort();
            files.dedup();
            bindings.push(Binding {
                id: h.id,
                kind: Kind::Proof,
                check_ref: check_ref(&pkg.name, &h.path),
                binding_hash: hash_of(repo, &files)?,
                files,
                token: Location { path: krate.files[h.file].rel.clone(), line: h.line },
            });
        }
    }
    check_distinct(&bindings)?;
    bindings.sort_by(|a, b| (&a.token.path, a.token.line).cmp(&(&b.token.path, b.token.line)));
    Ok(bindings)
}

/// Harness tokens this adapter sees and binds nothing to, with why.
pub fn uncovered(adapter: &KaniAdapter, repo: &Path) -> Result<Vec<Uncovered>, AdapterError> {
    let mut found = Vec::new();
    for (_, krate) in crates(&adapter.root_in(repo), adapter.root(), repo)? {
        for h in &krate.harnesses {
            let reason = match (h.marker, krate.kind) {
                (Kani::Contract, _) => "a Kani contract harness; Kani contracts are not supported yet",
                (Kani::Proof, CrateKind::Test) => {
                    "a Kani harness in an integration test; the kani adapter binds harnesses in a library or binary crate (move it into a `#[cfg(kani)]` module under src/)"
                }
                (Kani::Proof, _) => continue,
            };
            found.push(Uncovered {
                id: h.id,
                token: Location { path: krate.files[h.file].rel.clone(), line: h.line },
                reason: reason.to_string(),
            });
        }
    }
    Ok(found)
}

/// The workspace root `Cargo.toml` when the adapter root is a workspace, and
/// `Cargo.lock` at the adapter root when there is one, repo-relative.
fn shared_files(adapter: &KaniAdapter, repo: &Path) -> Result<Vec<PathBuf>, AdapterError> {
    let root = adapter.root_in(repo);
    let mut files = Vec::new();
    if read_manifest(&root.join("Cargo.toml"))?.contains_key("workspace") {
        files.push(adapter.repo_relative(Path::new("Cargo.toml")));
    }
    if root.join("Cargo.lock").is_file() {
        files.push(adapter.repo_relative(Path::new("Cargo.lock")));
    }
    Ok(files)
}
