//! The repository a command runs in: its root, its clock, and the proob files
//! in its working tree, read once and parsed through the facts crate so every
//! error names the file and the field.
use crate::cli::error::CliError;
use proob_core::types::{PropertyId, Secs};
use proob_facts::config::{Config, parse_config};
use proob_facts::paths::{CONFIG, PROOB_DIR, PROPERTIES_DIR};
use proob_facts::property::{PropertyFile, parse_property};
use proob_facts::ulid;
use std::path::{Path, PathBuf};

/// Test hook: a fixed clock, in seconds since the epoch, so goldens are stable.
pub const NOW_ENV: &str = "PROOB_NOW";

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub now: Secs,
}

/// One property file of the working tree.
pub struct LoadedProperty {
    /// Root-relative, such as `proob/properties/x.toml`.
    pub path: String,
    pub text: String,
    pub file: PropertyFile,
    pub id: PropertyId,
}

/// `proob/config.toml` as found, or absent (the built-in defaults then apply).
pub enum ConfigSource {
    File(Config),
    Missing,
}

impl ConfigSource {
    pub fn candidate(&self) -> Config {
        match self {
            ConfigSource::File(c) => c.clone(),
            ConfigSource::Missing => Config::defaults(),
        }
    }
}

impl Workspace {
    /// `repo` or the current directory; it must exist.
    pub fn open(repo: Option<&Path>) -> Result<Workspace, CliError> {
        let root = match repo {
            Some(path) => path.to_path_buf(),
            None => std::env::current_dir()
                .map_err(|e| CliError::failed(format!("cannot read the current directory: {e}")))?,
        };
        if !root.is_dir() {
            return Err(CliError::Usage(format!("{} is not a directory", root.display())));
        }
        Ok(Workspace { root, now: clock() })
    }

    pub fn has_proob_dir(&self) -> bool {
        self.root.join(PROOB_DIR).is_dir()
    }

    /// The error for a repository without a `proob/` directory.
    pub fn require_proob_dir(&self) -> Result<(), CliError> {
        if self.has_proob_dir() {
            Ok(())
        } else {
            Err(CliError::Usage(format!("no proob/ directory in {}; run `proob init` to start", self.root.display())))
        }
    }

    /// Every `proob/properties/*.toml`, by name; a file that does not parse
    /// is an error naming it.
    pub fn load_properties(&self) -> Result<Vec<LoadedProperty>, CliError> {
        self.property_paths()?.into_iter().map(|path| self.load_property(&path)).collect()
    }

    /// Root-relative paths of the property files, sorted.
    pub fn property_paths(&self) -> Result<Vec<String>, CliError> {
        let dir = self.root.join(PROPERTIES_DIR);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(CliError::failed(format!("cannot read {}: {e}", dir.display()))),
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.ends_with(".toml") && !n.starts_with('.'))
            .collect();
        names.sort();
        Ok(names.into_iter().map(|n| format!("{PROPERTIES_DIR}/{n}")).collect())
    }

    fn load_property(&self, path: &str) -> Result<LoadedProperty, CliError> {
        let text = self.read_text(path)?;
        let file = parse_property(&text).map_err(|e| CliError::invalid(path, e))?;
        let id = ulid::decode(&file.id).map_err(|e| CliError::invalid(path, format!("id: not a ULID: {e}")))?;
        Ok(LoadedProperty { path: path.to_string(), text, file, id })
    }

    pub fn load_config(&self) -> Result<ConfigSource, CliError> {
        match self.read_optional(CONFIG)? {
            Some(text) => parse_config_text(&text, CONFIG).map(ConfigSource::File),
            None => Ok(ConfigSource::Missing),
        }
    }

    pub fn read_text(&self, path: &str) -> Result<String, CliError> {
        std::fs::read_to_string(self.root.join(path)).map_err(|e| CliError::failed(format!("cannot read {path}: {e}")))
    }

    pub fn read_optional(&self, path: &str) -> Result<Option<String>, CliError> {
        match std::fs::read_to_string(self.root.join(path)) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(CliError::failed(format!("cannot read {path}: {e}"))),
        }
    }
}

/// Parses config text, naming `label` in the error.
pub fn parse_config_text(text: &str, label: &str) -> Result<Config, CliError> {
    parse_config(text).map_err(|e| CliError::invalid(label, e))
}

/// The clock: `PROOB_NOW` when set (tests and goldens), else the platform's.
/// A clock before the epoch reads as the epoch.
pub fn clock() -> Secs {
    std::env::var(NOW_ENV).ok().and_then(|t| t.trim().parse().ok()).unwrap_or_else(|| {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
    })
}
