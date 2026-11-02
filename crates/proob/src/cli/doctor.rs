//! `proob doctor`: whether this machine can run each adapter
//! `proob/config.toml` lists, and has the room a run needs. It locates and
//! version-probes tools and never installs one; `/proob:setup` installs, with
//! the user's approval, from what this reports.
use crate::cli::error::CliError;
use proob_adapters::tla::tools::{JAR_ENV, PIN as TLA_PIN};
use proob_adapters::{Readiness, adapter_for, language_name, mutation};
use proob_facts::config::{Config, Language};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct Doctor {
    pub ok: bool,
    pub adapters: Vec<Row>,
    pub disk: Room,
    /// `PROOB_MEMORY_LIMIT_MB`, else half the physical memory; `None`: no budget.
    pub memory_budget_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub language: &'static str,
    pub root: String,
    pub ready: bool,
    pub versions: BTreeMap<String, String>,
    /// Why the adapter cannot run, with how to install what is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Room {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Every configured adapter's readiness, the disk under `repo`, and the memory budget.
pub fn run(repo: &Path, config: &Config) -> Result<Doctor, CliError> {
    let adapters = config
        .adapters
        .iter()
        .map(|a| {
            let adapter = adapter_for(a, config.witness.into()).map_err(|e| CliError::Usage(e.to_string()))?;
            let readiness = adapter.map_or_else(
                || Readiness::Missing { reason: "this proob has no adapter for it".into(), versions: Vec::new() },
                |ad| ad.readiness(repo),
            );
            let readiness = match a.language {
                Language::Tla => pinned_tlc(readiness, jar_digest()),
                _ => readiness,
            };
            Ok(row(language_name(a.language), &a.root, readiness))
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    let disk = match mutation::room(repo) {
        Ok(()) => Room { ok: true, reason: None },
        Err(reason) => Room { ok: false, reason: Some(reason) },
    };
    let ok = !adapters.is_empty() && adapters.iter().all(|r| r.ready) && disk.ok;
    Ok(Doctor { ok, adapters, disk, memory_budget_bytes: proob_spawn::memory_budget() })
}

fn row(language: &'static str, root: &str, readiness: Readiness) -> Row {
    let (ready, versions, reason) = match readiness {
        Readiness::Ready(v) => (true, v, None),
        Readiness::Missing { reason, versions } => (false, versions, Some(reason)),
    };
    Row { language, root: root.to_string(), ready, versions: versions.into_iter().collect(), reason }
}

/// The SHA-256 of the jar `PROOB_TLA2TOOLS` names, when it can be read.
fn jar_digest() -> Option<String> {
    let bytes = std::fs::read(std::env::var_os(JAR_ENV)?).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// Pure: a TLC that resolves is ready only as the pinned release, the one the
/// adapter's parser is tested against.
pub fn pinned_tlc(readiness: Readiness, digest: Option<String>) -> Readiness {
    match (readiness, digest) {
        (Readiness::Ready(versions), Some(d)) if d != TLA_PIN.sha256 => Readiness::Missing {
            reason: format!(
                "the jar {JAR_ENV} names is not TLA+ {} (its sha256 is {d}, the pin's is {}): proob's TLC parser is tested against that release; download {} and point {JAR_ENV} at it",
                TLA_PIN.version, TLA_PIN.sha256, TLA_PIN.url
            ),
            versions,
        },
        (r, _) => r,
    }
}

pub fn text(d: &Doctor) -> String {
    let mut out = String::new();
    if d.adapters.is_empty() {
        out.push_str("adapters: none in proob/config.toml; run `proob init`\n");
    }
    for r in &d.adapters {
        let versions = r.versions.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ");
        let _ = match &r.reason {
            None => writeln!(out, "{} at {}: ready ({versions})", r.language, r.root),
            Some(why) => writeln!(out, "{} at {}: not ready: {why}", r.language, r.root),
        };
    }
    let _ = match &d.disk.reason {
        None => writeln!(out, "disk: room to build"),
        Some(why) => writeln!(out, "disk: {why}"),
    };
    let _ = match d.memory_budget_bytes {
        Some(b) => {
            writeln!(out, "memory budget: {:.1} GB (PROOB_MEMORY_LIMIT_MB sets it; 0 turns it off)", b as f64 / 1e9)
        }
        None => writeln!(out, "memory budget: none"),
    };
    let problems = d.adapters.iter().filter(|r| !r.ready).count() + usize::from(!d.disk.ok);
    let _ = match (d.ok, problems) {
        (true, _) => writeln!(out, "proob doctor: ok"),
        (false, 0) => writeln!(out, "proob doctor: nothing to check"),
        (false, n) => writeln!(out, "proob doctor: {n} not ready"),
    };
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tlc_other_than_the_pin_is_not_ready_and_says_which() {
        let ready = || Readiness::Ready(vec![("java".into(), "25".into())]);
        assert_eq!(pinned_tlc(ready(), Some(TLA_PIN.sha256.clone())), ready());
        assert_eq!(pinned_tlc(ready(), None), ready());
        let Readiness::Missing { reason, versions } = pinned_tlc(ready(), Some("ab".repeat(32))) else {
            panic!("ready")
        };
        assert!(reason.contains(&TLA_PIN.url) && reason.contains(&"ab".repeat(32)), "{reason}");
        assert_eq!(versions, vec![("java".to_string(), "25".to_string())]);
        let missing = Readiness::Missing { reason: "x".into(), versions: Vec::new() };
        assert_eq!(pinned_tlc(missing.clone(), Some("ab".repeat(32))), missing);
    }

    #[test]
    fn the_text_names_each_adapter_and_counts_what_is_not_ready() {
        let d = Doctor {
            ok: false,
            adapters: vec![
                row("rust", ".", Readiness::Ready(vec![("cargo".into(), "1.99.1".into())])),
                row("tla", "specs", Readiness::Missing { reason: "TLC was not found".into(), versions: Vec::new() }),
            ],
            disk: Room { ok: true, reason: None },
            memory_budget_bytes: Some(8_000_000_000),
        };
        let t = text(&d);
        assert!(t.contains("rust at .: ready (cargo 1.99.1)"), "{t}");
        assert!(t.contains("tla at specs: not ready: TLC was not found"), "{t}");
        assert!(t.contains("memory budget: 8.0 GB"), "{t}");
        assert!(t.ends_with("proob doctor: 1 not ready\n"), "{t}");
    }
}
