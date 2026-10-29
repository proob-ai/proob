//! Where a repository's shadow lives, and which repository a directory's
//! shadow belongs to.
//!
//! `$PROOB_SHADOW_HOME`, else `~/.proob/shadow/`, holds one directory per
//! repository, `<name>-<first 8 hex of sha256(absolute root)>/`: `work/` (the
//! private clone), `overlay.patch`, `base`, `report.md`, `report.json`, and
//! `adopted` once adopt has made a branch. `work/.proob-shadow` names the
//! real repository, so a command run inside `work/` finds its way back.
use crate::cli::error::CliError;
use crate::cli::shadow::git::{Step, line};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const HOME_ENV: &str = "PROOB_SHADOW_HOME";
/// At the root of `work/`; its last line is the real repository's root.
pub const MARKER: &str = ".proob-shadow";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The user's repository, canonical.
    pub repo: PathBuf,
    /// This repository's directory under the shadow home.
    pub dir: PathBuf,
}

impl Place {
    pub fn work(&self) -> PathBuf {
        self.dir.join("work")
    }
    pub fn overlay(&self) -> PathBuf {
        self.dir.join("overlay.patch")
    }
    pub fn base_file(&self) -> PathBuf {
        self.dir.join("base")
    }
    pub fn report_md(&self) -> PathBuf {
        self.dir.join("report.md")
    }
    pub fn report_json(&self) -> PathBuf {
        self.dir.join("report.json")
    }
    pub fn adopted_file(&self) -> PathBuf {
        self.dir.join("adopted")
    }

    /// A shadow exists when its marker does.
    pub fn exists(&self) -> bool {
        self.work().join(MARKER).is_file()
    }

    pub fn require(&self) -> Result<(), CliError> {
        if self.exists() {
            Ok(())
        } else {
            Err(CliError::Shadow("no shadow yet for this repository; run /proob:try or proob shadow open".into()))
        }
    }

    /// The commit `work/` was cloned at.
    pub fn base(&self) -> Result<String, CliError> {
        read(&self.base_file()).map(|t| t.trim().to_string())
    }

    /// The branch adopt made, when it has.
    pub fn adopted(&self) -> Option<String> {
        std::fs::read_to_string(self.adopted_file()).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
    }
}

/// The marker's text for `repo`.
pub fn marker_text(repo: &Path) -> String {
    format!("# proob shadow: a private copy; nothing here is part of the repository below.\n{}\n", repo.display())
}

fn marker_repo(text: &str) -> Option<PathBuf> {
    text.lines().map(str::trim).rfind(|l| !l.is_empty() && !l.starts_with('#')).map(PathBuf::from)
}

/// `<name>-<first 8 hex of sha256(root)>`.
pub fn dir_name(root: &Path) -> String {
    let name = root.file_name().map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().into_owned());
    let digest = Sha256::digest(root.as_os_str().as_encoded_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{name}-{hex}")
}

/// The shadow home, with symbolic links resolved (`/var` is `/private/var` on
/// macOS, and test runners resolve the root they run in).
pub fn home() -> Result<PathBuf, CliError> {
    match (std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()), std::env::var_os("HOME").filter(|v| !v.is_empty())) {
        (Some(dir), _) => Ok(canonical(&PathBuf::from(dir))),
        (None, Some(home)) => Ok(canonical(&PathBuf::from(home).join(".proob/shadow"))),
        (None, None) => Err(CliError::Shadow(format!("cannot tell where to keep shadows: set {HOME_ENV} or HOME"))),
    }
}

/// `path` resolved through its deepest existing ancestor; nothing is created.
fn canonical(path: &Path) -> PathBuf {
    path.ancestors()
        .find_map(|a| {
            std::fs::canonicalize(a).ok().map(|c| path.strip_prefix(a).map_or_else(|_| c.clone(), |rest| c.join(rest)))
        })
        .unwrap_or_else(|| path.to_path_buf())
}

/// The shadow of the repository containing `start` (or the current
/// directory); from inside a shadow's `work/`, the shadow of its repository.
pub fn locate(start: Option<&Path>) -> Result<Place, CliError> {
    let dir = match start {
        Some(d) => d.to_path_buf(),
        None => {
            std::env::current_dir().map_err(|e| CliError::Shadow(format!("cannot read the current directory: {e}")))?
        }
    };
    let top = toplevel(&dir)?;
    let repo = match std::fs::read_to_string(top.join(MARKER)).ok().as_deref().and_then(marker_repo) {
        Some(real) => real,
        None => top,
    };
    Ok(Place { dir: home()?.join(dir_name(&repo)), repo })
}

fn toplevel(dir: &Path) -> Result<PathBuf, CliError> {
    let not_git = || {
        CliError::Shadow(format!(
            "{} is not a git repository; run this inside your repository or pass --repo <path>",
            dir.display()
        ))
    };
    if !dir.is_dir() {
        return Err(not_git());
    }
    let top = line(&Step::new(dir, &["rev-parse", "--show-toplevel"])).map_err(|_| not_git())?;
    std::fs::canonicalize(&top).map_err(|e| CliError::Shadow(format!("cannot resolve {top}: {e}")))
}

/// `path` with the home directory written `~`, as people read it.
pub fn shown(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from).and_then(|h| path.strip_prefix(&h).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

pub fn read(path: &Path) -> Result<String, CliError> {
    std::fs::read_to_string(path).map_err(|e| CliError::Shadow(format!("cannot read {}: {e}", path.display())))
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CliError::Shadow(format!("cannot create {}: {e}", dir.display())))?;
    }
    std::fs::write(path, bytes).map_err(|e| CliError::Shadow(format!("cannot write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_directory_name_is_the_repository_name_and_eight_hex_of_its_path() {
        let name = dir_name(Path::new("/home/a/payments"));
        assert!(name.starts_with("payments-"), "{name}");
        assert_eq!(name.len(), "payments-".len() + 8);
        assert_ne!(name, dir_name(Path::new("/home/b/payments")));
    }

    #[test]
    fn the_marker_names_the_repository_on_its_last_line() {
        assert_eq!(marker_repo(&marker_text(Path::new("/r/x"))), Some(PathBuf::from("/r/x")));
        assert_eq!(marker_repo("# only a comment\n"), None);
    }
}
