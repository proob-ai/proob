//! `proob shadow adopt [<id>...] [--branch <name>]`: the one command that
//! writes to the user's repository.
//!
//! It prints what it will do, then builds one commit in the shadow with a
//! scratch index (the user's `HEAD` tree, plus the chosen properties, their
//! bound test files, `proob/config.toml`, the two workflows, and `.gitignore`
//! and `.claude/settings.json` merged into the user's current versions), and
//! fetches it into the user's repository as a new branch. The user's
//! checkout, index, and current branch are never touched: the only writes
//! are the new ref and the objects it needs. It pushes nowhere and opens
//! nothing; it prints the commands that do.
use crate::cli::adapters::{Discovered, discover};
use crate::cli::error::CliError;
use crate::cli::init::{GITIGNORE, SETTINGS, gitignore_text, merged_settings};
use crate::cli::shadow::git::{self, Step, line};
use crate::cli::shadow::place::{self, Place};
use crate::cli::shadow::{UserRepo, default_adopt_branch, on_path, overlay, short, user_repo};
use crate::cli::workspace::{LoadedProperty, Workspace, clock};
use crate::cli::{Common, Exit};
use proob_effects::git::{Git, Repo};
use proob_facts::paths::{CONFIG, EVIDENCE_WORKFLOW, GATE_WORKFLOW};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Where an adopted file's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// The file as it is in the shadow's `work/`.
    Work,
    /// Merged into the user's current version (`.gitignore`, settings).
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub content: Content,
    /// Absent from the user's `HEAD`.
    pub new: bool,
}

pub fn run(common: &Common, ids: &[String], branch: Option<&str>) -> Result<Exit, CliError> {
    let place = place::locate(common.repo.as_deref())?;
    place.require()?;
    let user = user_repo(&place, common)?;
    refuse_in_progress(&place.repo)?;
    let base = place.base()?;
    if base != user.head {
        return Err(CliError::Shadow(
            "your repository has new commits since the shadow was made; run proob shadow open, then adopt".into(),
        ));
    }
    let branch = branch.map_or_else(|| default_adopt_branch(clock()), str::to_string);
    check_branch(&place.repo, &branch)?;
    let changed = overlay::capture(&place, &base)?;
    let ws = Workspace::open(Some(&place.work()))?;
    let properties = ws.load_properties()?;
    let chosen = choose(&properties, ids)?;
    let discovered = discover(&ws, &ws.load_config()?.candidate())?;
    let entries = entries(&place, &user.head, &properties, &chosen, discovered.as_ref(), &changed)?;
    print!("{}", plan_text(&branch, &user, &entries));
    let commit = build_commit(&place, &user, &entries, &message(&chosen))?;
    publish(&place, &commit, &branch)?;
    place::write(&place.adopted_file(), format!("{branch}\n").as_bytes())?;
    print!("{}", done_text(&branch, &user, chosen.len(), entries.len()));
    Ok(Exit::Pass)
}

fn refuse_in_progress(repo: &Path) -> Result<(), CliError> {
    let gitdir = PathBuf::from(line(&Step::new(repo, &["rev-parse", "--absolute-git-dir"]))?);
    let states = [
        ("rebase-merge", "a rebase", "git rebase --continue", "git rebase --abort"),
        ("rebase-apply", "a rebase", "git rebase --continue", "git rebase --abort"),
        ("MERGE_HEAD", "a merge", "git merge --continue", "git merge --abort"),
    ];
    match states.iter().find(|(file, ..)| gitdir.join(file).exists()) {
        Some((_, what, go, stop)) => Err(CliError::Shadow(format!(
            "{what} is in progress in your repository; finish it ({go}) or abort it ({stop}), then run proob shadow adopt again"
        ))),
        None => Ok(()),
    }
}

fn check_branch(repo: &Path, branch: &str) -> Result<(), CliError> {
    let invalid =
        || CliError::Shadow(format!("{branch} is not a valid branch name; pick another with --branch <name>"));
    if branch.starts_with('-') {
        return Err(invalid());
    }
    line(&Step::new(repo, &["check-ref-format", "--branch", branch])).map_err(|_| invalid())?;
    match git::run(&Step::new(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])) {
        Ok(_) => Err(CliError::Shadow(format!(
            "branch {branch} already exists in your repository; pick another name with --branch <name>"
        ))),
        Err(_) => Ok(()),
    }
}

/// The properties named, or all of them.
fn choose<'a>(properties: &'a [LoadedProperty], ids: &[String]) -> Result<Vec<&'a LoadedProperty>, CliError> {
    if properties.is_empty() {
        return Err(CliError::Shadow("nothing to adopt yet; run /proob:try first".into()));
    }
    if ids.is_empty() {
        return Ok(properties.iter().collect());
    }
    ids.iter()
        .map(|id| {
            crate::cli::hash::find(properties, id).map_err(|_| {
                CliError::Shadow(format!("no property {id} in the shadow; proob shadow report lists them"))
            })
        })
        .collect()
}

fn bound_files(d: Option<&Discovered>, p: &LoadedProperty) -> Vec<String> {
    d.and_then(|d| d.binding(p.id))
        .map(|(_, b)| b.files.iter().map(|f| f.to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

/// What the branch will carry: the shadow's own files that the overlay
/// changed, for the chosen properties, plus the two merged files.
fn entries(
    place: &Place,
    head: &str,
    properties: &[LoadedProperty],
    chosen: &[&LoadedProperty],
    d: Option<&Discovered>,
    changed: &[String],
) -> Result<Vec<Entry>, CliError> {
    let changed: BTreeSet<&str> = changed.iter().map(String::as_str).collect();
    let fixed = [CONFIG, GATE_WORKFLOW, EVIDENCE_WORKFLOW].map(str::to_string);
    let paths: BTreeSet<String> = chosen
        .iter()
        .flat_map(|p| std::iter::once(p.path.clone()).chain(bound_files(d, p)))
        .chain(fixed)
        .filter(|p| changed.contains(p.as_str()))
        .collect();
    refuse_shared(&place.work(), &paths, properties, chosen)?;
    let user = Git::open(&place.repo);
    let is_new = |path: &str| user.blob_at(head, path).map(|b| b.is_none()).map_err(CliError::failed);
    let mut out: Vec<Entry> = paths
        .iter()
        .map(|p| Ok(Entry { path: p.clone(), content: Content::Work, new: is_new(p)? }))
        .collect::<Result<_, CliError>>()?;
    for (path, text) in merged(&user, head)? {
        out.push(Entry { new: is_new(path)?, path: path.to_string(), content: Content::Text(text) });
    }
    Ok(out)
}

/// `.gitignore` and `.claude/settings.json` as `proob init` would leave the
/// user's current versions, when that changes them.
fn merged(user: &Git, head: &str) -> Result<Vec<(&'static str, String)>, CliError> {
    let at = |path: &str| {
        user.file_at(head, path).map(|b| b.map(|b| String::from_utf8_lossy(&b).into_owned())).map_err(CliError::failed)
    };
    let (ignore, settings) = (at(GITIGNORE)?, at(SETTINGS)?);
    let (settings_text, _) = merged_settings(settings.as_deref())?;
    Ok([
        (GITIGNORE, gitignore_text(ignore.as_deref())),
        (SETTINGS, Some(settings_text).filter(|t| settings.as_deref() != Some(t))),
    ]
    .into_iter()
    .filter_map(|(p, t)| t.map(|t| (p, t)))
    .collect())
}

/// A test file that also binds a property left out would name a property
/// the branch does not carry, and `proob validate` would fail there.
fn refuse_shared(
    work: &Path,
    paths: &BTreeSet<String>,
    all: &[LoadedProperty],
    chosen: &[&LoadedProperty],
) -> Result<(), CliError> {
    let left_out: Vec<&LoadedProperty> = all.iter().filter(|p| !chosen.iter().any(|c| c.id == p.id)).collect();
    for path in paths {
        let text = std::fs::read_to_string(work.join(path)).unwrap_or_default();
        if let Some(p) = left_out.iter().find(|p| text.contains(&format!("proob:{}", p.file.id))) {
            return Err(CliError::Shadow(format!(
                "{path} also binds {}, which you did not choose; adopt it too or move its test to a file of its own",
                p.file.id
            )));
        }
    }
    Ok(())
}

fn message(chosen: &[&LoadedProperty]) -> String {
    let list: String = chosen.iter().map(|p| format!("- {}\n", p.file.statement.trim())).collect();
    format!(
        "proob: adopt {} propert{} from shadow mode\n\nEach reads Proposed until a teammate approves this pull request.\n\n{list}",
        chosen.len(),
        if chosen.len() == 1 { "y" } else { "ies" }
    )
}

// ---------------------------------------------------------------------------
// The commit and the branch
// ---------------------------------------------------------------------------

fn mode(work: &Path, e: &Entry) -> &'static str {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let exec = matches!(e.content, Content::Work)
            && std::fs::metadata(work.join(&e.path)).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
        if exec {
            return "100755";
        }
    }
    let _ = (work, e);
    "100644"
}

/// The blob for one entry, written into the shadow's object store.
fn blob(place: &Place, e: &Entry) -> Result<String, CliError> {
    let work = place.work();
    match &e.content {
        Content::Work => line(&Step::new(&work, &["hash-object", "-w", "--", &e.path])),
        Content::Text(text) => {
            let tmp = place.dir.join("adopt.blob");
            place::write(&tmp, text.as_bytes())?;
            let oid = line(&Step::new(
                &work,
                &["hash-object", "-w", &format!("--path={}", e.path), "--", &tmp.display().to_string()],
            ));
            let _ = std::fs::remove_file(&tmp);
            oid
        }
    }
}

/// The author and committer for the commit: the user's own git identity.
fn identity(repo: &Path) -> Result<Vec<(String, String)>, CliError> {
    let get = |key: &str| line(&Step::new(repo, &["config", key])).ok().filter(|v| !v.is_empty());
    match (get("user.name"), get("user.email")) {
        (Some(name), Some(email)) => Ok(["AUTHOR", "COMMITTER"]
            .iter()
            .flat_map(|who| [(format!("GIT_{who}_NAME"), name.clone()), (format!("GIT_{who}_EMAIL"), email.clone())])
            .collect()),
        _ => Err(CliError::Shadow(
            "git does not know who you are; set git config user.name and user.email, then run proob shadow adopt again"
                .into(),
        )),
    }
}

fn build_commit(place: &Place, user: &UserRepo, entries: &[Entry], message: &str) -> Result<String, CliError> {
    let work = place.work();
    let index = place.dir.join("adopt.index").display().to_string();
    let ident = identity(&place.repo)?;
    let at = |args: &[&str]| Step::new(&work, args).env("GIT_INDEX_FILE", &index);
    let made = (|| {
        let blobs = entries.iter().map(|e| blob(place, e)).collect::<Result<Vec<String>, CliError>>()?;
        let adds = entries.iter().zip(&blobs).map(|(e, oid)| {
            at(&["update-index", "--add", "--cacheinfo", &format!("{},{oid},{}", mode(&work, e), e.path)])
        });
        git::run_all(&std::iter::once(at(&["read-tree", &user.head])).chain(adds).collect::<Vec<_>>())?;
        let tree = line(&at(&["write-tree"]))?;
        let mut commit = at(&["commit-tree", &tree, "-p", &user.head, "-m", message]);
        commit.env.extend(ident);
        line(&commit)
    })();
    let _ = std::fs::remove_file(&index);
    made
}

/// The commit as a new branch of the user's repository: a fetch from the
/// shadow into `refs/heads/<branch>`, with no FETCH_HEAD and no maintenance.
pub fn publish_plan(repo: &Path, work: &Path, commit: &str, branch: &str) -> Vec<Step> {
    const TMP: &str = "refs/proob/adopt";
    let refspec = format!("{TMP}:refs/heads/{branch}");
    vec![
        Step::new(work, &["update-ref", TMP, commit]),
        Step::new(
            repo,
            &[
                "-c",
                "gc.auto=0",
                "-c",
                "maintenance.auto=false",
                "fetch",
                "--quiet",
                "--no-tags",
                "--no-write-fetch-head",
                &work.display().to_string(),
                &refspec,
            ],
        ),
        Step::new(work, &["update-ref", "-d", TMP]),
    ]
}

fn publish(place: &Place, commit: &str, branch: &str) -> Result<(), CliError> {
    git::run_all(&publish_plan(&place.repo, &place.work(), commit, branch))
}

// ---------------------------------------------------------------------------
// Words
// ---------------------------------------------------------------------------

fn plan_text(branch: &str, user: &UserRepo, entries: &[Entry]) -> String {
    let files: String =
        entries.iter().map(|e| format!("  {:<8} {}\n", if e.new { "new" } else { "changed" }, e.path)).collect();
    format!(
        "shadow: adopt creates branch {branch} from your last commit {}, without switching your checkout:\n{files}",
        short(&user.head)
    )
}

fn done_text(branch: &str, user: &UserRepo, properties: usize, files: usize) -> String {
    let next: Vec<String> = match (&user.origin, on_path("gh")) {
        (None, _) => vec![format!("next: git remote add origin <your GitHub URL>, then git push -u origin {branch}")],
        (Some(_), true) => vec![
            format!("next: git push -u origin {branch}"),
            format!("next: gh pr create --head {branch} --base {} --fill", user.default_branch),
        ],
        (Some(_), false) => vec![
            format!("next: git push -u origin {branch}"),
            format!("next: open a pull request from {branch} into {}", user.default_branch),
        ],
    };
    format!(
        "shadow: made branch {branch} in your repository ({properties} propert{}, {files} file{}); your checkout is unchanged\n{}\nshadow: a teammate's review of that pull request is the approval (a merge you review yourself counts as self-signed)\n",
        if properties == 1 { "y" } else { "ies" },
        if files == 1 { "" } else { "s" },
        next.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_branch_arrives_by_fetch_without_fetch_head_or_maintenance() {
        let plan = publish_plan(Path::new("/r"), Path::new("/s/work"), "abc", "proob/adopt-x");
        let fetch = plan[1].args.join(" ");
        assert_eq!(plan[1].dir, Path::new("/r"));
        assert!(
            fetch.contains(
                "fetch --quiet --no-tags --no-write-fetch-head /s/work refs/proob/adopt:refs/heads/proob/adopt-x"
            ),
            "{fetch}"
        );
        assert!(fetch.starts_with("-c gc.auto=0 -c maintenance.auto=false"));
    }
}
