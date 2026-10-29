//! Discovery through the adapter registry: the bindings that `status`,
//! `check`, `hash`, `validate`, and `run` read, and the "no adapter" answer
//! when `proob/config.toml` lists none this proob has.
//!
//! Discovery is a fact about the working tree, so it runs before GitHub is
//! consulted and its errors (a malformed token, two tests bound to one
//! property) stop a command with the file and line; `validate` reports the
//! same errors as problems.
use crate::cli::backend::{self, Bindings};
use crate::cli::error::CliError;
use crate::cli::workspace::Workspace;
use proob_adapters::{Adapter, AdapterError, Binding, Uncovered, adapters_for};
use proob_core::types::PropertyId;
use proob_effects::facts::GovernedSet;
use proob_effects::gather::BindingSet;
use proob_facts::config::Config;
use proob_facts::ulid;
use std::collections::BTreeMap;

/// The adapters a config names and this proob has.
pub fn registry(config: &Config) -> Result<Vec<Box<dyn Adapter>>, CliError> {
    adapters_for(config).map_err(|e| CliError::Usage(e.to_string()))
}

/// What discovery found: each bound property with the adapter that bound it.
pub struct Discovered {
    pub adapters: Vec<Box<dyn Adapter>>,
    pub by_id: BTreeMap<PropertyId, (usize, Binding)>,
    /// Tokens an adapter saw where no configured adapter checks, for ids no
    /// adapter bound: these properties render `Not checked (no adapter)`.
    pub uncovered: BTreeMap<PropertyId, Uncovered>,
}

impl Discovered {
    /// The binding sets as the pipeline reads them.
    pub fn bindings(&self) -> Bindings {
        let by_id: BTreeMap<PropertyId, BindingSet> = self
            .by_id
            .iter()
            .map(|(id, (_, b))| {
                (*id, BindingSet { files: b.files.iter().map(|f| f.to_string_lossy().into_owned()).collect() })
            })
            .collect();
        let governed = GovernedSet::new(by_id.values().flat_map(|b| b.files.iter().cloned()));
        Bindings { by_id, governed, no_adapter: self.uncovered.keys().copied().collect() }
    }

    pub fn binding(&self, id: PropertyId) -> Option<(&dyn Adapter, &Binding)> {
        self.by_id.get(&id).map(|(i, b)| (self.adapters[*i].as_ref(), b))
    }
}

/// Every binding under every configured adapter; `None` when the config
/// lists no adapter this proob has, so every property renders
/// `Not checked (no adapter)`.
pub fn discover(ws: &Workspace, config: &Config) -> Result<Option<Discovered>, CliError> {
    let adapters = registry(config)?;
    if adapters.is_empty() {
        return Ok(None);
    }
    let mut by_id: BTreeMap<PropertyId, (usize, Binding)> = BTreeMap::new();
    for (i, adapter) in adapters.iter().enumerate() {
        for b in adapter.discover(&ws.root).map_err(adapter_error)? {
            if let Some((_, first)) = by_id.get(&b.id) {
                return Err(CliError::invalid(
                    b.token.to_string(),
                    format!(
                        "binding token `proob:{}` is already bound at {}; a property binds one check",
                        ulid::encode(b.id),
                        first.token
                    ),
                ));
            }
            by_id.insert(b.id, (i, b));
        }
    }
    let mut uncovered: BTreeMap<PropertyId, Uncovered> = BTreeMap::new();
    for adapter in &adapters {
        for u in adapter.uncovered(&ws.root).map_err(adapter_error)?.into_iter().filter(|u| !by_id.contains_key(&u.id))
        {
            uncovered.entry(u.id).or_insert(u);
        }
    }
    Ok(Some(Discovered { adapters, by_id, uncovered }))
}

/// The binding sets for the pipeline: the fixture's when a test supplies
/// them, else real discovery.
pub fn bindings(ws: &Workspace, config: &Config) -> Result<Option<Bindings>, CliError> {
    if backend::fixture_has_bindings() {
        return Ok(None);
    }
    Ok(discover(ws, config)?.map(|d| d.bindings()))
}

pub fn adapter_error(e: AdapterError) -> CliError {
    match e {
        AdapterError::Token { at, problem } => {
            CliError::invalid(at.to_string(), format!("{problem}; run `proob validate`"))
        }
        AdapterError::Duplicate { id, first, second } => CliError::invalid(
            second.to_string(),
            format!("binding token `proob:{id}` is already bound at {first}; a property binds one check"),
        ),
        other => CliError::failed(format!("adapter: {other}")),
    }
}
