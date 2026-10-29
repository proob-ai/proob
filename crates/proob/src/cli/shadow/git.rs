//! Git for shadow mode, planned as data and run in one place.
//!
//! A `Step` is one git invocation: the directory it runs in, its argv, and
//! the environment it adds. The plans (`clone`, `capture`, the commit adopt
//! builds) are pure functions returning steps; `run` is the only function in
//! shadow mode that starts git, through `proob_effects::git::Git::exec`.
use crate::cli::error::CliError;
use proob_effects::error::GitError;
use proob_effects::git::Git;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub dir: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl Step {
    pub fn new(dir: &Path, args: &[&str]) -> Step {
        Step { dir: dir.to_path_buf(), args: args.iter().map(|a| (*a).to_string()).collect(), env: Vec::new() }
    }

    /// The same step with `key=value` added to its environment.
    pub fn env(mut self, key: &str, value: &str) -> Step {
        self.env.push((key.to_string(), value.to_string()));
        self
    }
}

/// Runs one step; its stdout, or git's own words on failure.
pub fn run(step: &Step) -> Result<Vec<u8>, GitError> {
    let args: Vec<&str> = step.args.iter().map(String::as_str).collect();
    let env: Vec<(&str, &str)> = step.env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    Git::open(&step.dir).exec(&args, &env)
}

/// Runs one step and trims its stdout to a line.
pub fn line(step: &Step) -> Result<String, CliError> {
    run(step).map(|out| String::from_utf8_lossy(&out).trim().to_string()).map_err(|e| failed(step, &e))
}

/// Runs every step in order, stopping at the first failure.
pub fn run_all(steps: &[Step]) -> Result<(), CliError> {
    steps.iter().try_for_each(|s| run(s).map(drop).map_err(|e| failed(s, &e)))
}

/// A git failure as one `shadow:` line: what git said, first line only.
pub fn failed(step: &Step, e: &GitError) -> CliError {
    let said = match e {
        GitError::Failed { stderr, .. } => {
            stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
        }
        other => other.to_string(),
    };
    CliError::Shadow(format!("git {} failed: {said}", step.args.first().map_or("", String::as_str)))
}

/// git's stderr when a step failed with an exit code, for callers that read it.
pub fn stderr_of(e: &GitError) -> Option<&str> {
    match e {
        GitError::Failed { stderr, .. } => Some(stderr),
        _ => None,
    }
}
