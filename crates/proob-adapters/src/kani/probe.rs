//! The build probe: can Kani build this crate at all? Asked once, by `proob
//! init --kani` and `proob shadow open`, before any property is declared a
//! proof, so a crate Kani cannot build (one that needs a newer compiler than
//! Kani's nightly, or does not compile under it) never gets a declaration
//! full of proofs that each fall back.
//!
//! A trivial harness is appended to the first package's library crate root,
//! else its first binary's, in a temporary copy of the adapter root
//! (`score::Lab`: the user's tree is never touched, and the dependencies
//! build once with the score's), and run as `check` runs a harness, so the
//! reason a probe fails is the one a check would give.
use crate::argv::Checked;
use crate::kani::check::{TIMEOUT, judge};
use crate::kani::score::Lab;
use crate::kani::tools::{Resolution, resolve};
use crate::kani::{CheckRef, KaniAdapter};
use crate::rust::manifest::{CrateKind, crate_roots, packages};
use crate::{CheckOutcome, cap};
use proob_spawn::SpawnError;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The harness the probe adds at the crate root.
pub const HARNESS: &str = "proob_kani_probe";
const HARNESS_TEXT: &str = "\n#[cfg(kani)]\n#[kani::proof]\nfn proob_kani_probe() {\n    let x: u8 = kani::any();\n    assert!(u16::from(x) < 256);\n}\n";
const REASON_CAP: usize = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    Passed,
    Failed { reason: String },
}

impl Probe {
    /// `passed`, or `failed: <reason>` on one line.
    pub fn line(&self) -> String {
        match self {
            Probe::Passed => "passed".to_string(),
            Probe::Failed { reason } => {
                format!("failed: {}", cap(&reason.split_whitespace().collect::<Vec<_>>().join(" "), REASON_CAP))
            }
        }
    }

    /// The record kept in `proob/.cache`: `kani_probe = "<line>"`, TOML.
    pub fn record(&self) -> String {
        format!("kani_probe = {}\n", toml::Value::String(self.line()))
    }
}

pub fn run(adapter: &KaniAdapter, repo: &Path) -> Probe {
    let root = adapter.root_in(repo);
    let tc = match resolve(adapter.tool_env()) {
        Resolution::Ready(tc) => tc,
        Resolution::Missing { reason, .. } => return Probe::Failed { reason },
    };
    let ran = target(&root).and_then(|(package, file)| {
        let r = CheckRef { package, harness: Checked::rust_path(HARNESS)? };
        let lab = Lab::new(adapter, &tc, &root, &r).map_err(|e| e.to_string())?;
        append(&lab.copy().join(file))?;
        let (out, sarif, _) = lab.harness(TIMEOUT)?;
        Ok(match out {
            Ok(out) => judge(&out, sarif.as_deref(), &r),
            Err(SpawnError::TimedOut { after, .. }) => CheckOutcome::Errored {
                reason: format!("Kani timed out after {} minutes building the crate", after.as_secs() / 60),
            },
            Err(e) => CheckOutcome::Errored { reason: format!("cargo kani did not run: {e}") },
        })
    });
    match ran {
        Ok(CheckOutcome::Verified { .. }) => Probe::Passed,
        Ok(CheckOutcome::Errored { reason }) => Probe::Failed { reason },
        Ok(CheckOutcome::ProofFailed { diagnostics }) => Probe::Failed { reason: diagnostics },
        Ok(other) => Probe::Failed { reason: format!("unrecognised Kani outcome: {other:?}") },
        Err(reason) => Probe::Failed { reason },
    }
}

/// The package and its crate root the harness goes in (module doc), the
/// file relative to the adapter root.
// shortcut: probes one crate of a workspace; probe each package if members differ in what Kani can build.
fn target(root: &Path) -> Result<(Checked, PathBuf), String> {
    let pkgs = packages(root, Path::new("")).map_err(|e| e.to_string())?;
    let found = pkgs.iter().find_map(|pkg| {
        let roots = crate_roots(pkg).ok()?;
        let lib = roots.iter().find(|r| r.kind == CrateKind::Lib);
        let file = lib.or_else(|| roots.iter().find(|r| r.kind == CrateKind::Bin))?.file.clone();
        Some((pkg.name.clone(), file))
    });
    let (name, file) = found.ok_or("no library or binary crate under the adapter root for Kani to build")?;
    let file = file.strip_prefix(root).map_err(|e| e.to_string())?.to_path_buf();
    Ok((Checked::package(&name)?, file))
}

fn append(file: &Path) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new().append(true).open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    f.write_all(HARNESS_TEXT.as_bytes()).map_err(|e| format!("{}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_is_one_toml_string() {
        assert_eq!(Probe::Passed.record(), "kani_probe = \"passed\"\n");
        let failed = Probe::Failed { reason: "the crate did not compile under Kani:\nerror: \"x\"\\".to_string() };
        let table: toml::Table = failed.record().parse().expect("TOML");
        assert_eq!(table["kani_probe"].as_str(), Some("failed: the crate did not compile under Kani: error: \"x\"\\"));
    }

    #[test]
    fn the_harness_is_the_one_the_probe_names() {
        assert!(HARNESS_TEXT.contains(&format!("fn {HARNESS}()")));
    }
}
