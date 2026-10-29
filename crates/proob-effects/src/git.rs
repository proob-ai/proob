//! The repository as the effects read it: history through `git` (shelled out
//! through proob-spawn, never linked) and the working tree through the file
//! system, behind the `Repo` trait so facts can be assembled from fixtures.
//!
//! Every git answer is parsed strictly and anything else fails closed as
//! `GitError::Unparsable`. Object ids are `[u8; 32]`, left-aligned: a SHA-1
//! id fills 20 bytes and the rest are zero (`proob_facts::hex`), and
//! `encode_oid` turns one back into the 40 characters git expects. Paths are
//! passed after `--`; a revision that starts with `-` is refused before git
//! sees it. Full history is required: `fetch-depth: 0` in CI.
use crate::error::GitError;
use proob_core::types::{Oid, Secs};
use proob_facts::hex;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What the facts layer asks of a repository.
pub trait Repo {
    /// The commit a revision names.
    fn head(&self, rev: &str) -> Result<Oid, GitError>;
    /// The last first-parent commit on `rev` that touched any of `paths`;
    /// `None` when none did (the file is not on that branch).
    fn introducing_commit(&self, rev: &str, paths: &[&str]) -> Result<Option<Oid>, GitError>;
    /// Up to `limit` first-parent commits on `rev` touching any of `paths`, newest first.
    fn touching_commits(&self, rev: &str, paths: &[&str], limit: usize) -> Result<Vec<Oid>, GitError>;
    /// First-parent commits on `rev` after `since` touching any of `paths`,
    /// newest first, with their commit times.
    fn commits_since(&self, since: &Oid, rev: &str, paths: &[&str]) -> Result<Vec<(Oid, Secs)>, GitError>;
    /// A commit's committer time.
    fn commit_time(&self, oid: &Oid) -> Result<Secs, GitError>;
    /// The blob id of `path` at `rev`; `None` when the revision or the path
    /// does not exist there.
    fn blob_at(&self, rev: &str, path: &str) -> Result<Option<Oid>, GitError>;
    /// Tracked files of the working tree, root-relative.
    fn ls_files(&self) -> Result<Vec<String>, GitError>;
    /// Paths that differ between `base`'s merge base with `head` and `head`.
    fn changed_paths(&self, base: &str, head: &str) -> Result<Vec<String>, GitError>;
    /// A working-tree file's bytes; `None` when it does not exist.
    fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>, GitError>;
    /// The bytes of `path` as committed at `rev`; `None` when the revision or
    /// the path does not exist there.
    fn file_at(&self, rev: &str, path: &str) -> Result<Option<Vec<u8>>, GitError>;
    /// The branch `origin/HEAD` points at (`main` for `refs/remotes/origin/main`);
    /// `None` when the remote's HEAD is not recorded locally.
    fn default_branch(&self) -> Result<Option<String>, GitError>;
    /// The fetch URL of a remote; `None` when no such remote is configured.
    fn remote_url(&self, remote: &str) -> Result<Option<String>, GitError>;
}

/// `git` in a working tree.
#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
    program: PathBuf,
    timeout: Duration,
}

impl Git {
    pub fn open(root: &Path) -> Git {
        Git { root: root.to_path_buf(), program: PathBuf::from("git"), timeout: Duration::from_secs(120) }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Runs git and returns stdout on success.
    fn run(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
        let out = self.run_any(args)?;
        if out.success() {
            Ok(out.stdout)
        } else {
            Err(GitError::Failed {
                command: command_line(args),
                code: out.code,
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            })
        }
    }

    /// Runs git and returns whatever happened.
    fn run_any(&self, args: &[&str]) -> Result<proob_spawn::Output, GitError> {
        self.run_env(args, &[])
    }

    fn run_env(&self, args: &[&str], extra: &[(&str, &str)]) -> Result<proob_spawn::Output, GitError> {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        let fixed = [("GIT_TERMINAL_PROMPT", "0"), ("GIT_OPTIONAL_LOCKS", "0"), ("LC_ALL", "C")];
        let env: Vec<(OsString, OsString)> =
            fixed.iter().chain(extra).map(|(k, v)| (OsString::from(k), OsString::from(v))).collect();
        proob_spawn::run(&self.program, &argv, Some(&self.root), &env, self.timeout).map_err(GitError::Spawn)
    }

    /// Any git command, run in this directory with `env` added; stdout on
    /// success. For shadow mode, which clones, applies patches, and builds a
    /// commit with plumbing; the caller plans every argument. Not part of
    /// `Repo`: fixtures never write.
    pub fn exec(&self, args: &[&str], env: &[(&str, &str)]) -> Result<Vec<u8>, GitError> {
        let out = self.run_env(args, env)?;
        if out.success() {
            Ok(out.stdout)
        } else {
            Err(GitError::Failed {
                command: command_line(args),
                code: out.code,
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            })
        }
    }

    /// `git fetch <remote> <ref>`: brings a base branch into a checkout that
    /// lacks it. Not part of `Repo`: fixtures never fetch, and the judge and
    /// execute jobs reach it only when `fetch-depth: 0` was not honoured.
    pub fn fetch(&self, remote: &str, git_ref: &str) -> Result<(), GitError> {
        self.run(&["fetch", "--quiet", checked_rev(remote)?, checked_rev(git_ref)?]).map(|_| ())
    }

    /// Paths the working tree changes against `base`'s merge base with
    /// `HEAD`: committed, staged, unstaged, and untracked (not ignored), sorted.
    /// Not part of `Repo`: only the informational uncovered list reads it.
    pub fn worktree_changed_paths(&self, base: &str) -> Result<Vec<String>, GitError> {
        let mb_args = ["merge-base", checked_rev(base)?, "HEAD"];
        let mb = String::from_utf8_lossy(&self.run(&mb_args)?).trim().to_string();
        parse_oid(&mb).ok_or_else(|| unparsable(&mb_args, &mb))?;
        let mut paths = nul_separated(&self.run(&["diff", "--name-only", "-z", &mb])?);
        paths.extend(nul_separated(&self.run(&["ls-files", "-z", "--others", "--exclude-standard"])?));
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    /// Lines of `git log` with a `%H` or `%H %ct` format, parsed strictly.
    fn log(&self, args: &[&str]) -> Result<Vec<(Oid, Option<Secs>)>, GitError> {
        let out = self.run(args)?;
        let text = String::from_utf8_lossy(&out);
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| parse_log_line(line).ok_or_else(|| unparsable(args, line)))
            .collect()
    }
}

fn command_line(args: &[&str]) -> String {
    std::iter::once("git").chain(args.iter().copied()).collect::<Vec<_>>().join(" ")
}

fn unparsable(args: &[&str], output: &str) -> GitError {
    GitError::Unparsable { command: command_line(args), output: output.to_string() }
}

fn parse_oid(text: &str) -> Option<Oid> {
    hex::decode_oid(text.trim()).ok()
}

/// `<sha>` or `<sha> <secs>`.
fn parse_log_line(line: &str) -> Option<(Oid, Option<Secs>)> {
    let mut parts = line.split_whitespace();
    let oid = parse_oid(parts.next()?)?;
    let time = match parts.next() {
        Some(t) => Some(t.parse().ok()?),
        None => None,
    };
    Some((oid, time))
}

/// A revision must not look like an option.
fn checked_rev(rev: &str) -> Result<&str, GitError> {
    if rev.starts_with('-') || rev.is_empty() {
        Err(GitError::UnsafeRevision { rev: rev.to_string() })
    } else {
        Ok(rev)
    }
}

/// NUL-separated paths; non-UTF-8 names are skipped (a scope glob could not
/// have matched them anyway: the glob alphabet is ASCII).
fn nul_separated(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .filter_map(|p| std::str::from_utf8(p).ok().map(str::to_string))
        .collect()
}

fn with_paths<'a>(mut args: Vec<&'a str>, paths: &[&'a str]) -> Vec<&'a str> {
    args.push("--");
    args.extend_from_slice(paths);
    args
}

impl Repo for Git {
    fn head(&self, rev: &str) -> Result<Oid, GitError> {
        let spec = format!("{}^{{commit}}", checked_rev(rev)?);
        let args = ["rev-parse", "--verify", "--quiet", spec.as_str()];
        let out = self.run(&args)?;
        let text = String::from_utf8_lossy(&out);
        parse_oid(&text).ok_or_else(|| unparsable(&args, &text))
    }

    fn introducing_commit(&self, rev: &str, paths: &[&str]) -> Result<Option<Oid>, GitError> {
        Ok(self.touching_commits(rev, paths, 1)?.into_iter().next())
    }

    fn touching_commits(&self, rev: &str, paths: &[&str], limit: usize) -> Result<Vec<Oid>, GitError> {
        let n = limit.to_string();
        let args = with_paths(vec!["log", "--first-parent", "--format=%H", "-n", &n, checked_rev(rev)?], paths);
        Ok(self.log(&args)?.into_iter().map(|(oid, _)| oid).collect())
    }

    fn commits_since(&self, since: &Oid, rev: &str, paths: &[&str]) -> Result<Vec<(Oid, Secs)>, GitError> {
        let range = format!("{}..{}", hex::encode_oid(since), checked_rev(rev)?);
        let args = with_paths(vec!["log", "--first-parent", "--format=%H %ct", &range], paths);
        self.log(&args)?
            .into_iter()
            .map(|(oid, t)| t.map(|t| (oid, t)).ok_or_else(|| unparsable(&args, "missing commit time")))
            .collect()
    }

    fn commit_time(&self, oid: &Oid) -> Result<Secs, GitError> {
        let sha = hex::encode_oid(oid);
        let args = ["show", "-s", "--format=%ct", &sha];
        let out = self.run(&args)?;
        let text = String::from_utf8_lossy(&out);
        text.trim().parse().map_err(|_| unparsable(&args, &text))
    }

    fn blob_at(&self, rev: &str, path: &str) -> Result<Option<Oid>, GitError> {
        let spec = format!("{}:{path}", checked_rev(rev)?);
        let args = ["rev-parse", "--verify", "--quiet", spec.as_str()];
        let out = self.run_any(&args)?;
        if !out.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&out.stdout);
        parse_oid(&text).map(Some).ok_or_else(|| unparsable(&args, &text))
    }

    fn ls_files(&self) -> Result<Vec<String>, GitError> {
        Ok(nul_separated(&self.run(&["ls-files", "-z", "--cached"])?))
    }

    fn changed_paths(&self, base: &str, head: &str) -> Result<Vec<String>, GitError> {
        let range = format!("{}...{}", checked_rev(base)?, checked_rev(head)?);
        Ok(nul_separated(&self.run(&["diff", "--name-only", "-z", &range])?))
    }

    fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        let full = self.root.join(path);
        match std::fs::read(&full) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(GitError::Read { path: full, message: e.to_string() }),
        }
    }

    fn file_at(&self, rev: &str, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        if self.blob_at(rev, path)?.is_none() {
            return Ok(None);
        }
        let spec = format!("{}:{path}", checked_rev(rev)?);
        self.run(&["cat-file", "blob", spec.as_str()]).map(Some)
    }

    fn default_branch(&self) -> Result<Option<String>, GitError> {
        let args = ["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"];
        let out = self.run_any(&args)?;
        if !out.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let short = text.trim();
        match short.strip_prefix("origin/") {
            Some(name) if !name.is_empty() => Ok(Some(name.to_string())),
            _ => Err(unparsable(&args, short)),
        }
    }

    fn remote_url(&self, remote: &str) -> Result<Option<String>, GitError> {
        let key = format!("remote.{remote}.url");
        let out = self.run_any(&["config", "--get", key.as_str()])?;
        if !out.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(if text.is_empty() { None } else { Some(text) })
    }
}
