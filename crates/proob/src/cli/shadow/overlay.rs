//! proob's additions in a shadow, as one binary-safe patch.
//!
//! The overlay is everything in `work/` that differs from the commit it was
//! cloned at, untracked files included: a scratch index (a copy of `work`'s
//! own, so git's stat cache stays valid) takes `add -A`, and `diff --cached
//! --binary` against the base is the patch. `.gitignore` and
//! `.claude/settings.json` are left out: `proob init` rewrites them on every
//! open and adopt merges them into the user's current versions, so a user's
//! edit to either never makes the overlay conflict.
use crate::cli::error::CliError;
use crate::cli::init::{GITIGNORE, SETTINGS};
use crate::cli::shadow::git::{self, Step};
use crate::cli::shadow::place::{self, Place};
use std::collections::BTreeSet;
use std::path::Path;

/// Paths under shadow control are the whole tree minus the regenerated files.
fn pathspec() -> Vec<String> {
    vec![".".to_string(), format!(":(exclude){GITIGNORE}"), format!(":(exclude){SETTINGS}")]
}

/// The steps that stage `work` into the scratch index at `index`, then the
/// patch and the name list against `base`.
pub fn plan(work: &Path, index: &Path, base: &str) -> (Step, Step, Step) {
    let spec = pathspec();
    let with = |head: &[&str]| {
        let args: Vec<&str> =
            head.iter().copied().chain(std::iter::once("--")).chain(spec.iter().map(String::as_str)).collect();
        Step::new(work, &args).env("GIT_INDEX_FILE", &index.display().to_string())
    };
    (
        with(&["add", "--all"]),
        with(&["diff", "--cached", "--binary", "--no-renames", "--no-ext-diff", base]),
        with(&["diff", "--cached", "--name-only", "-z", "--no-renames", base]),
    )
}

/// Reads the overlay from `work`, writes it to `overlay.patch`, and returns
/// the paths it touches.
pub fn capture(place: &Place, base: &str) -> Result<Vec<String>, CliError> {
    let work = place.work();
    let index = place.dir.join("overlay.index");
    let copied = std::fs::copy(work.join(".git/index"), &index).map(drop);
    let (add, patch, names) = plan(&work, &index, base);
    let result = copied
        .map_err(|e| CliError::Shadow(format!("cannot copy the shadow's index: {e}")))
        .and_then(|()| git::run_all(&[add]))
        .and_then(|()| git::run(&patch).map_err(|e| git::failed(&patch, &e)))
        .and_then(|bytes| git::run(&names).map(|n| (bytes, n)).map_err(|e| git::failed(&names, &e)));
    let _ = std::fs::remove_file(&index);
    let (patch, names) = result?;
    place::write(&place.overlay(), &patch)?;
    Ok(nul_separated(&names))
}

fn nul_separated(bytes: &[u8]) -> Vec<String> {
    bytes.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect()
}

/// The steps that check, then apply, `patch` in `work`.
pub fn apply_plan(work: &Path, patch: &Path) -> (Step, Step) {
    let p = patch.display().to_string();
    (
        Step::new(work, &["apply", "--check", "--binary", "--whitespace=nowarn", &p]),
        Step::new(work, &["apply", "--binary", "--whitespace=nowarn", &p]),
    )
}

/// The files `git apply` named as not applying, from its stderr.
pub fn conflicts(stderr: &str) -> Vec<String> {
    let found: BTreeSet<String> = stderr
        .lines()
        .filter_map(|l| l.trim().strip_prefix("error: "))
        .filter_map(|rest| {
            rest.strip_prefix("patch failed: ")
                .and_then(|p| p.rsplit_once(':').map(|(path, _)| path))
                .or_else(|| rest.strip_suffix(": patch does not apply"))
                .or_else(|| rest.strip_suffix(": already exists in working directory"))
                .or_else(|| rest.strip_suffix(": does not exist in index"))
                .or_else(|| rest.strip_suffix(": No such file or directory"))
                .map(str::to_string)
        })
        .collect();
    found.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflicts_are_read_from_git_apply_stderr() {
        let stderr = "error: patch failed: tests/test_props.py:3\nerror: tests/test_props.py: patch does not apply\nerror: proob/config.toml: already exists in working directory\n";
        assert_eq!(conflicts(stderr), ["proob/config.toml", "tests/test_props.py"]);
        assert!(conflicts("fatal: something else\n").is_empty());
    }

    #[test]
    fn the_overlay_leaves_out_the_files_init_regenerates() {
        let (add, diff, _) = plan(Path::new("/w"), Path::new("/i"), "abc");
        assert!(add.args.contains(&":(exclude).gitignore".to_string()));
        assert!(diff.args.contains(&":(exclude).claude/settings.json".to_string()));
        assert_eq!(diff.env, [("GIT_INDEX_FILE".to_string(), "/i".to_string())]);
    }
}
