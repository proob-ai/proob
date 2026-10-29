//! `proob hash <id>`: the three hashes approvals and evidence are about, for
//! the property's current content and the current tree.
use crate::cli::backend::Backend;
use crate::cli::error::CliError;
use crate::cli::workspace::{LoadedProperty, Workspace};
use proob_core::binding_hash::binding_hash;
use proob_core::scope_hash::{ScopeHash, scope_hash};
use proob_effects::gather::{digests_of, resolve_scope};
use proob_facts::convert;
use proob_facts::hex;
use proob_facts::property::content_hash_of;
use serde::Serialize;

#[derive(Serialize)]
pub struct Hashes {
    pub id: String,
    pub path: String,
    pub algo: Algos,
    pub content_hash: String,
    /// `None` when no adapter binds the property.
    pub binding_hash: Option<String>,
    pub binding_files: Vec<String>,
    /// `None` when the scope matches nothing.
    pub scope_hash: Option<String>,
    pub scope_files: Vec<String>,
}

#[derive(Serialize)]
pub struct Algos {
    pub content: &'static str,
    pub binding: &'static str,
    pub scope: &'static str,
}

/// The property whose ULID text is `id`, case-insensitively.
pub fn find<'a>(properties: &'a [LoadedProperty], id: &str) -> Result<&'a LoadedProperty, CliError> {
    let wanted =
        proob_facts::ulid::decode(id).map_err(|e| CliError::Usage(format!("{id} is not a property id: {e}")))?;
    properties
        .iter()
        .find(|p| p.id == wanted)
        .ok_or_else(|| CliError::Usage(format!("no property with id {id} under proob/properties/")))
}

pub fn hashes(
    ws: &Workspace,
    backend: &Backend<'_>,
    p: &LoadedProperty,
    waiver_max_days: u32,
) -> Result<Hashes, CliError> {
    let property = convert::property(&p.file, ws.now, waiver_max_days).map_err(|e| CliError::invalid(&p.path, e))?;
    let scope = resolve_scope(backend.repo, &property.scope).map_err(CliError::failed)?;
    let scope_files: Vec<String> = scope.iter().map(|pd| String::from_utf8_lossy(&pd.path).into_owned()).collect();
    let scope_hash = match scope_hash(scope) {
        ScopeHash::Ok(h) => Some(hex::encode(&h)),
        ScopeHash::DuplicatePath => {
            return Err(CliError::failed(format!("{}: a scope file was resolved twice", p.path)));
        }
    }
    .filter(|_| !scope_files.is_empty());
    let binding = backend.bindings.and_then(|b| b.by_id.get(&p.id));
    let binding_files: Vec<String> = binding.map(|b| b.files.clone()).unwrap_or_default();
    let binding_hash = match binding {
        Some(set) => {
            let pairs = digests_of(backend.repo, &set.files).map_err(CliError::failed)?;
            Some(
                binding_hash(pairs)
                    .map(|h| hex::encode(&h))
                    .ok_or_else(|| CliError::failed(format!("{}: a binding file was listed twice", p.path)))?,
            )
        }
        None => None,
    };
    Ok(Hashes {
        id: p.file.id.clone(),
        path: p.path.clone(),
        algo: Algos { content: "proob-content-v1", binding: "proob-binding-v1", scope: "proob-scope-v1" },
        content_hash: hex::encode(&content_hash_of(&p.text)),
        binding_hash,
        binding_files,
        scope_hash,
        scope_files,
    })
}

pub fn text(h: &Hashes) -> String {
    let binding = h.binding_hash.as_deref().unwrap_or("unknown (no adapter has reported a binding set)");
    let scope = match &h.scope_hash {
        Some(s) => format!("{s} over {} file{}", h.scope_files.len(), if h.scope_files.len() == 1 { "" } else { "s" }),
        None => "none (scope matches nothing)".to_string(),
    };
    format!(
        "property: {} ({})\ncontent hash ({}): {}\nbinding hash ({}): {binding}\nscope hash ({}): {scope}\n",
        h.path, h.id, h.algo.content, h.content_hash, h.algo.binding, h.algo.scope
    )
}
