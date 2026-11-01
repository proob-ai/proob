//! Discovery: every `.tla` file under the adapter root (skipping the
//! directories the Python adapter skips) is scanned for tokens, and each
//! bound definition gets the binding set of the module doc.
use crate::python::discover::files_with;
use crate::rust::discover::{check_distinct, hash_of};
use crate::tla::scan::{Bound, imports, masked, tokens};
use crate::tla::{TlaAdapter, check_ref};
use crate::{AdapterError, Binding, Location, TokenProblem};
use proob_core::types::Kind;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub fn discover(adapter: &TlaAdapter, repo: &Path) -> Result<Vec<Binding>, AdapterError> {
    let root = adapter.root_in(repo);
    let mut bindings = Vec::new();
    for rel in files_with(&root, "tla")? {
        for token in tokens(&read(&root.join(&rel))?) {
            let at = Location { path: adapter.repo_relative(&rel), line: token.line };
            match token.bound {
                Bound::Problem(problem) => return Err(AdapterError::Token { at, problem }),
                Bound::Definition { id, name } => {
                    let files = binding_set(&root, &rel)
                        .map_err(|problem| AdapterError::Token { at: at.clone(), problem })?
                        .iter()
                        .map(|f| adapter.repo_relative(f))
                        .collect::<Vec<_>>();
                    bindings.push(Binding {
                        id,
                        kind: Kind::Model,
                        check_ref: check_ref(&rel, &name),
                        binding_hash: hash_of(repo, &files)?,
                        files,
                        token: at,
                    });
                }
            }
        }
    }
    check_distinct(&bindings)?;
    bindings.sort_by(|a, b| (&a.token.path, a.token.line).cmp(&(&b.token.path, b.token.line)));
    Ok(bindings)
}

/// The module's `.cfg`, root-relative.
pub fn cfg_of(module: &Path) -> PathBuf {
    module.with_extension("cfg")
}

/// The module (root-relative), its `.cfg`, and the modules beside it that
/// it reaches through `EXTENDS` and `INSTANCE`, sorted. A missing `.cfg` is
/// the token's problem.
pub fn binding_set(root: &Path, module: &Path) -> Result<Vec<PathBuf>, TokenProblem> {
    let cfg = cfg_of(module);
    if !root.join(&cfg).is_file() {
        return Err(TokenProblem::NoTlaConfig { config: cfg.display().to_string() });
    }
    let mut files: BTreeSet<PathBuf> = [cfg].into();
    let mut todo = vec![module.to_path_buf()];
    while let Some(m) = todo.pop() {
        if !files.insert(m.clone()) {
            continue;
        }
        let text = std::fs::read_to_string(root.join(&m)).unwrap_or_default();
        let dir = m.parent().unwrap_or(Path::new(""));
        todo.extend(
            imports(&masked(&text))
                .iter()
                .map(|name| dir.join(format!("{name}.tla")))
                .filter(|p| root.join(p).is_file()),
        );
    }
    Ok(files.into_iter().collect())
}

fn read(path: &Path) -> Result<String, AdapterError> {
    std::fs::read_to_string(path).map_err(|e| AdapterError::Io { path: path.to_path_buf(), message: e.to_string() })
}
