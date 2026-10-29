//! `Git` against a real repository built in a temporary directory: heads,
//! introducing commits, history, drift commits, blobs, tracked files, changed
//! paths, and the refusal of option-shaped revisions.
#![cfg(unix)]
use proob_effects::error::GitError;
use proob_effects::git::{Git, Repo};
use proob_facts::hex;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

const P: &str = "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4C6.toml";
const LIB: &str = "src/lib.rs";

fn git(root: &Path, args: &[&str]) -> String {
    let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
    let env = [
        (OsString::from("GIT_CONFIG_GLOBAL"), OsString::from("/dev/null")),
        (OsString::from("GIT_CONFIG_NOSYSTEM"), OsString::from("1")),
        (OsString::from("GIT_AUTHOR_NAME"), OsString::from("t")),
        (OsString::from("GIT_AUTHOR_EMAIL"), OsString::from("t@example.invalid")),
        (OsString::from("GIT_COMMITTER_NAME"), OsString::from("t")),
        (OsString::from("GIT_COMMITTER_EMAIL"), OsString::from("t@example.invalid")),
    ];
    let out = proob_spawn::run(Path::new("git"), &argv, Some(root), &env, Duration::from_secs(60)).expect("git runs");
    assert!(out.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
    std::fs::write(full, text).expect("write");
}

/// A repository with three commits on `main`: A adds P, B modifies P, C adds src/lib.rs.
fn repository() -> (tempfile::TempDir, [String; 3]) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(root, P, "id = \"01J9Z3W1V8Q4K7N2M5R8T1X4C6\"\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "A: add property"]);
    let a = git(root, &["rev-parse", "HEAD"]);
    write(root, P, "id = \"01J9Z3W1V8Q4K7N2M5R8T1X4C6\"\ntitle = \"t\"\n");
    git(root, &["commit", "-q", "-am", "B: amend property"]);
    let b = git(root, &["rev-parse", "HEAD"]);
    write(root, LIB, "pub fn f() {}\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "C: add lib"]);
    let c = git(root, &["rev-parse", "HEAD"]);
    (dir, [a, b, c])
}

fn oid(text: &str) -> [u8; 32] {
    hex::decode_oid(text).expect("sha")
}

#[test]
fn history_questions_are_answered_from_a_real_repository() {
    let (dir, [a, b, c]) = repository();
    let repo = Git::open(dir.path());

    assert_eq!(repo.head("main").expect("head"), oid(&c));
    assert_eq!(hex::encode_oid(&repo.head("main").expect("head")), c, "ids round-trip to git's text");

    assert_eq!(repo.introducing_commit("main", &[P]).expect("log"), Some(oid(&b)));
    assert_eq!(repo.introducing_commit("main", &[LIB]).expect("log"), Some(oid(&c)));
    assert_eq!(repo.introducing_commit("main", &["absent.txt"]).expect("log"), None);
    assert_eq!(repo.introducing_commit("main", &[P, LIB]).expect("log"), Some(oid(&c)), "the latest over a set");

    assert_eq!(repo.touching_commits("main", &[P], 5).expect("log"), vec![oid(&b), oid(&a)]);
    assert_eq!(repo.touching_commits("main", &[P], 1).expect("log"), vec![oid(&b)]);

    let since_a: Vec<[u8; 32]> =
        repo.commits_since(&oid(&a), "main", &[P]).expect("log").into_iter().map(|(o, _)| o).collect();
    assert_eq!(since_a, vec![oid(&b)], "drift counts the amendment, not the unrelated commit");
    assert!(repo.commits_since(&oid(&b), "main", &[P]).expect("log").is_empty());
    let (_, t) = repo.commits_since(&oid(&b), "main", &[LIB]).expect("log")[0];
    assert!(t >= repo.commit_time(&oid(&b)).expect("time"), "commit times are ordered");

    let blob_b = repo.blob_at(&b, P).expect("blob").expect("present");
    let blob_a = repo.blob_at(&a, P).expect("blob").expect("present");
    assert_ne!(blob_a, blob_b, "the amendment changed the blob");
    assert_eq!(repo.blob_at("main", P).expect("blob"), Some(blob_b));
    assert_eq!(repo.blob_at(&a, LIB).expect("blob"), None, "not yet added at A");
    assert_eq!(repo.blob_at("main", "nope.txt").expect("blob"), None);

    let mut files = repo.ls_files().expect("ls-files");
    files.sort();
    assert_eq!(files, vec![P.to_string(), LIB.to_string()]);

    let mut changed = repo.changed_paths(&a, "main").expect("diff");
    changed.sort();
    assert_eq!(changed, vec![P.to_string(), LIB.to_string()]);
    assert_eq!(repo.changed_paths(&b, "main").expect("diff"), vec![LIB.to_string()]);

    assert_eq!(repo.read_file(LIB).expect("read"), Some(b"pub fn f() {}\n".to_vec()));
    assert_eq!(repo.read_file("absent.txt").expect("read"), None);
}

#[test]
fn option_shaped_revisions_are_refused_before_git_sees_them() {
    let (dir, _) = repository();
    let repo = Git::open(dir.path());
    assert!(matches!(repo.head("--output=/tmp/x"), Err(GitError::UnsafeRevision { .. })));
    assert!(matches!(repo.blob_at("-x", P), Err(GitError::UnsafeRevision { .. })));
    assert!(matches!(repo.changed_paths("-a", "main"), Err(GitError::UnsafeRevision { .. })));
}

#[test]
fn an_unknown_revision_fails_closed() {
    let (dir, _) = repository();
    let repo = Git::open(dir.path());
    assert!(matches!(repo.head("no-such-branch"), Err(GitError::Failed { .. })));
}
