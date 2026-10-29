//! `proob shadow`: try proob on a repository without touching it.
//!
//! A shadow is a private `git clone --local --no-hardlinks` of the user's
//! repository at its `HEAD`, kept under the shadow home (`place`). Every
//! proob command can run there with `--shadow` (or from inside `work/`, where
//! the marker says so), and whatever proob or an agent adds there is kept as
//! `overlay.patch`, which `open` carries over to the user's next commit.
//!
//! Invariant: nothing here writes to the user's repository except `adopt`,
//! which adds one branch ref and the objects it needs, and never touches the
//! checkout. Every read of the user's repository goes through git with
//! `GIT_OPTIONAL_LOCKS=0`, so not even the index is refreshed.
pub mod adopt;
pub mod git;
pub mod overlay;
pub mod place;
pub mod report;

use crate::cli::error::CliError;
use crate::cli::workspace::Workspace;
use crate::cli::{Common, Exit, init};
use clap::Subcommand;
use git::{Step, line};
use place::{MARKER, Place};
use proob_effects::git::{Git, Repo};
use proob_facts::paths::RESULTS_DIR;
use proob_facts::results::RESULTS_FILE;
use std::path::Path;

#[derive(Subcommand)]
pub enum ShadowCmd {
    /// Create or refresh this repository's shadow, a private copy proob works in.
    Open,
    /// What the shadow tried, and the steps to make it real.
    Report,
    /// Put the shadow's declaration, tests, and workflows on a new branch of your repository (your checkout stays as it is).
    Adopt {
        /// Property ids to adopt (default: all).
        ids: Vec<String>,
        /// The branch to create (default: proob/adopt-<date>).
        #[arg(long)]
        branch: Option<String>,
    },
    /// Remove this repository's shadow.
    Clean {
        /// Remove every shadow.
        #[arg(long)]
        all: bool,
    },
    /// Print the shadow's working directory.
    Path,
}

pub fn run(common: &Common, cmd: ShadowCmd) -> Result<Exit, CliError> {
    match cmd {
        ShadowCmd::Open => open(common),
        ShadowCmd::Report => report_cmd(common),
        ShadowCmd::Adopt { ids, branch } => adopt::run(common, &ids, branch.as_deref()),
        ShadowCmd::Clean { all } => clean(common, all),
        ShadowCmd::Path => path(common),
    }
}

/// `--shadow`: the same command, against this repository's shadow.
pub fn redirect(common: &Common) -> Result<Common, CliError> {
    if !common.shadow {
        return Ok(common.clone());
    }
    let place = place::locate(common.repo.as_deref())?;
    place.require()?;
    Ok(Common { repo: Some(place.work()), shadow: false, ..common.clone() })
}

/// A directory is a shadow's `work/` when the marker is at its root.
pub fn is_shadow(root: &Path) -> bool {
    root.join(MARKER).is_file()
}

// ---------------------------------------------------------------------------
// The user's repository, read only
// ---------------------------------------------------------------------------

/// What shadow reads from the user's repository, all of it read only.
pub struct UserRepo {
    pub head: String,
    pub origin: Option<String>,
    pub default_branch: String,
    pub dirty: bool,
}

pub fn user_repo(place: &Place, common: &Common) -> Result<UserRepo, CliError> {
    let at = |args: &[&str]| Step::new(&place.repo, args);
    let head = line(&at(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])).map_err(|_| {
        CliError::Shadow("your repository has no commits yet; commit once, then run proob shadow open".into())
    })?;
    let user = Git::open(&place.repo);
    let origin = user.remote_url("origin").map_err(CliError::failed)?;
    let default_branch =
        super::default_branch(&user, common.default_branch.as_deref()).unwrap_or_else(|_| "main".to_string());
    let dirty = !line(&at(&["status", "--porcelain"]))?.is_empty();
    Ok(UserRepo { head, origin, default_branch, dirty })
}

pub fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

// ---------------------------------------------------------------------------
// open
// ---------------------------------------------------------------------------

/// Ignored in `work`'s own `.git/info/exclude`: the marker, proob's cache, and
/// what test and mutation tools leave behind, so none of it joins the overlay.
const EXCLUDE: &str = "\n# proob shadow\n/.proob-shadow\n/proob/.cache/\n__pycache__/\n.hypothesis/\n.pytest_cache/\n/mutants/\n/mutants.out*/\n/target/\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opened {
    Created,
    Refreshed,
    Current,
}

fn open(common: &Common) -> Result<Exit, CliError> {
    let place = place::locate(common.repo.as_deref())?;
    let user = user_repo(&place, common)?;
    let opened = refresh(&place, &user)?;
    let n = property_count(&place.work());
    println!("{}", opened_line(&place, &user, opened, n));
    if user.dirty {
        println!("shadow: uncommitted changes in your repository are not included; commit them to try them");
    }
    Ok(Exit::Pass)
}

fn property_count(work: &Path) -> usize {
    Workspace::open(Some(work)).and_then(|ws| ws.property_paths()).map_or(0, |p| p.len())
}

fn opened_line(place: &Place, user: &UserRepo, opened: Opened, n: usize) -> String {
    let props = format!("{n} propert{}", if n == 1 { "y" } else { "ies" });
    let what = match (opened, n) {
        (Opened::Created, 0) => "nothing tried yet".to_string(),
        (Opened::Current, _) => format!("up to date, {props}"),
        (Opened::Created | Opened::Refreshed, _) => format!("{props} carried over"),
    };
    format!("shadow: ready at {} (your last commit {}, {what})", place::shown(&place.work()), short(&user.head))
}

/// Captures the overlay of an existing shadow; re-clones when the user's
/// `HEAD` moved (or there is no shadow), carrying the overlay over.
fn refresh(place: &Place, user: &UserRepo) -> Result<Opened, CliError> {
    let existing = if place.exists() { Some(place.base()?) } else { None };
    if let Some(base) = &existing {
        overlay::capture(place, base)?;
        if *base == user.head {
            return Ok(Opened::Current);
        }
    }
    build(place, user)?;
    Ok(if existing.is_some() { Opened::Refreshed } else { Opened::Created })
}

/// The clone, its remote made unpushable, and its default branch named.
pub fn clone_plan(repo: &Path, fresh: &Path, user: &UserRepo) -> Vec<Step> {
    let (repo_s, fresh_s) = (repo.display().to_string(), fresh.display().to_string());
    let parent = fresh.parent().unwrap_or(fresh);
    let origin = user.origin.as_deref().map(|url| Step::new(fresh, &["remote", "set-url", "origin", url]));
    let default_ref = format!("refs/remotes/origin/{}", user.default_branch);
    std::iter::once(Step::new(parent, &["clone", "--quiet", "--local", "--no-hardlinks", "--", &repo_s, &fresh_s]))
        .chain(origin)
        .chain([
            Step::new(fresh, &["remote", "set-url", "--push", "origin", "no-push"]),
            Step::new(fresh, &["symbolic-ref", "refs/remotes/origin/HEAD", &default_ref]),
            Step::new(fresh, &["checkout", "--quiet", "--detach", &user.head]),
        ])
        .collect()
}

/// A new `work/` at the user's `HEAD`, swapped in only once it is complete:
/// on any failure the old one is left exactly as it was.
fn build(place: &Place, user: &UserRepo) -> Result<(), CliError> {
    let fresh = place.dir.join("work.new");
    remove_dir(&fresh)?;
    std::fs::create_dir_all(&place.dir)
        .map_err(|e| CliError::Shadow(format!("cannot create {}: {e}", place.dir.display())))?;
    let made = git::run_all(&clone_plan(&place.repo, &fresh, user)).and_then(|()| prepare(place, &fresh, user));
    if let Err(e) = made {
        let _ = remove_dir(&fresh);
        return Err(e);
    }
    swap(place, &fresh)?;
    place::write(&place.base_file(), format!("{}\n", user.head).as_bytes())?;
    overlay::capture(place, &user.head).map(drop)
}

fn prepare(place: &Place, fresh: &Path, user: &UserRepo) -> Result<(), CliError> {
    let exclude = fresh.join(".git/info/exclude");
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    place::write(&exclude, format!("{existing}{EXCLUDE}").as_bytes())?;
    place::write(&fresh.join(MARKER), place::marker_text(&place.repo).as_bytes())?;
    carry_over(place, fresh)?;
    init::run(&Workspace::open(Some(fresh))?, &user.default_branch).map(drop)
}

/// The overlay applied to the new clone, and the local results kept.
fn carry_over(place: &Place, fresh: &Path) -> Result<(), CliError> {
    let patch = place.overlay();
    if std::fs::metadata(&patch).is_ok_and(|m| m.len() > 0) {
        let (check, apply) = overlay::apply_plan(fresh, &patch);
        git::run(&check).map_err(|e| conflict(&check, &e))?;
        git::run_all(&[apply])?;
    }
    let results = Path::new(RESULTS_DIR).join(RESULTS_FILE);
    match std::fs::read(place.work().join(&results)) {
        Ok(bytes) => place::write(&fresh.join(&results), &bytes),
        Err(_) => Ok(()),
    }
}

fn conflict(step: &Step, e: &proob_effects::error::GitError) -> CliError {
    match git::stderr_of(e).map(overlay::conflicts) {
        Some(files) if !files.is_empty() => CliError::Shadow(format!(
            "your latest commit changes {}, which the shadow also changed; the shadow is kept as it was; undo that change or run proob shadow clean to start over",
            files.join(", ")
        )),
        _ => git::failed(step, e),
    }
}

fn swap(place: &Place, fresh: &Path) -> Result<(), CliError> {
    let (work, old) = (place.work(), place.dir.join("work.old"));
    remove_dir(&old)?;
    let rename = |from: &Path, to: &Path| {
        std::fs::rename(from, to)
            .map_err(|e| CliError::Shadow(format!("cannot move {} to {}: {e}", from.display(), to.display())))
    };
    if work.exists() {
        rename(&work, &old)?;
    }
    rename(fresh, &work)?;
    remove_dir(&old)
}

fn remove_dir(dir: &Path) -> Result<(), CliError> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(CliError::Shadow(format!("cannot remove {}: {e}", dir.display())))
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// report, clean, path
// ---------------------------------------------------------------------------

/// Today's default adopt branch.
pub fn default_adopt_branch(now: u64) -> String {
    let date = proob_facts::date::Date::from_secs(now).map_or_else(|| "today".to_string(), |d| d.to_string());
    format!("proob/adopt-{date}")
}

/// `name` is an executable file in a `PATH` directory.
pub fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

pub fn next(place: &Place, user: &UserRepo) -> report::Next {
    report::Next {
        branch: default_adopt_branch(crate::cli::workspace::clock()),
        default_branch: user.default_branch.clone(),
        origin: user.origin.clone(),
        gh: on_path("gh"),
        adopted: place.adopted(),
    }
}

fn report_cmd(common: &Common) -> Result<Exit, CliError> {
    let place = place::locate(common.repo.as_deref())?;
    place.require()?;
    let base = place.base()?;
    overlay::capture(&place, &base)?;
    let user = user_repo(&place, common)?;
    let rows = report::rows(&place.work())?;
    let next = next(&place, &user);
    let text = report::text(&rows, &next);
    let json = serde_json::to_string_pretty(&report::json(&rows, &next, &place.repo, &place.work(), &base))
        .map_err(CliError::failed)?;
    place::write(&place.report_md(), text.as_bytes())?;
    place::write(&place.report_json(), format!("{json}\n").as_bytes())?;
    if common.json {
        println!("{json}");
    } else {
        print!("{text}");
    }
    Ok(if report::counts(&rows).fails > 0 { Exit::Block } else { Exit::Pass })
}

/// The session hook's line for a repository that has a shadow, from the
/// shadow alone (nothing is captured or written).
pub fn hook_line(dir: &Path) -> Option<String> {
    let place = place::locate(Some(dir)).ok().filter(Place::exists)?;
    let c = report::counts(&report::rows(&place.work()).ok()?);
    let tried = match c.tried {
        0 => "a shadow, nothing tried yet".to_string(),
        _ => report::tally(&c).join(", "),
    };
    let tail = match place.adopted() {
        None => "nothing in this repository was changed".to_string(),
        Some(b) => format!("adopt made branch {b}; nothing else in this repository was changed"),
    };
    Some(format!("proob (shadow): {tried} · /proob:try to see them · {tail}"))
}

fn clean(common: &Common, all: bool) -> Result<Exit, CliError> {
    let dirs = if all {
        let home = place::home()?;
        let entries = std::fs::read_dir(&home)
            .map(|e| e.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_else(|_| Vec::new());
        entries
            .into_iter()
            .filter(|d: &std::path::PathBuf| d.join("work").join(MARKER).is_file() || d.join("overlay.patch").is_file())
            .collect()
    } else {
        let place = place::locate(common.repo.as_deref())?;
        if place.dir.exists() { vec![place.dir] } else { Vec::new() }
    };
    if dirs.is_empty() {
        println!(
            "shadow: nothing to remove; {}",
            if all { "there are no shadows" } else { "this repository has no shadow" }
        );
    }
    for d in &dirs {
        let kept =
            std::fs::read_to_string(d.join("adopted")).ok().map(|b| b.trim().to_string()).filter(|b| !b.is_empty());
        remove_dir(d)?;
        let note = kept.map_or_else(
            || "your repository was not touched".to_string(),
            |b| format!("branch {b} that adopt made stays in your repository"),
        );
        println!("shadow: removed {} ({note})", place::shown(d));
    }
    Ok(Exit::Pass)
}

fn path(common: &Common) -> Result<Exit, CliError> {
    let place = place::locate(common.repo.as_deref())?;
    place.require()?;
    println!("{}", place.work().display());
    Ok(Exit::Pass)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clone_never_pushes_and_starts_at_the_users_head() {
        let user = UserRepo {
            head: "abc".into(),
            origin: Some("git@github.com:a/b.git".into()),
            default_branch: "main".into(),
            dirty: false,
        };
        let plan = clone_plan(Path::new("/r"), Path::new("/s/work.new"), &user);
        let argv: Vec<String> = plan.iter().map(|s| s.args.join(" ")).collect();
        assert_eq!(argv[0], "clone --quiet --local --no-hardlinks -- /r /s/work.new");
        assert!(argv.contains(&"remote set-url --push origin no-push".to_string()));
        assert!(argv.contains(&"remote set-url origin git@github.com:a/b.git".to_string()));
        assert_eq!(argv.last().map(String::as_str), Some("checkout --quiet --detach abc"));
    }

    #[test]
    fn the_adopt_branch_is_dated() {
        assert_eq!(default_adopt_branch(1_793_145_600), "proob/adopt-2026-10-28");
    }
}
