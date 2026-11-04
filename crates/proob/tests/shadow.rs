//! Shadow mode on a committed copy of the Python fixture without its
//! `proob/` and without binding tokens: a repository that has never seen
//! proob. Every test checks the invariant that matters most: nothing in the
//! user's repository changes (a recursive listing of the working tree and
//! `.git` with sizes and mtimes, and `git status --porcelain`), except the
//! one branch `adopt` makes.
mod common;

use common::*;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const PASSING: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XPA";
const FAILING: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XPB";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/py-repo").canonicalize().expect("fixture")
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("read_dir").filter_map(Result::ok) {
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if src.is_dir() {
            std::fs::create_dir_all(&dst).expect("mkdir");
            copy_tree(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).expect("copy");
        }
    }
}

/// The user's repository and a shadow home of its own.
struct World {
    repo: TempRepo,
    home: TempDir,
}

impl World {
    fn new() -> World {
        let repo = TempRepo::new();
        copy_tree(&fixture_dir(), repo.root());
        std::fs::remove_dir_all(repo.root().join("proob")).expect("rm proob");
        for test in ["tests/test_props.py", "tests/test_vacuous.py"] {
            let text: String =
                repo.read(test).lines().filter(|l| !l.starts_with("# proob:")).map(|l| format!("{l}\n")).collect();
            repo.write(test, &text);
        }
        repo.commit("the code, before proob", NOW - DAY);
        World { repo, home: tempfile::tempdir().expect("home") }
    }

    fn proob(&self) -> Command {
        let mut cmd = self.repo.proob();
        cmd.env("PROOB_SHADOW_HOME", self.home.path());
        cmd
    }

    fn ok(&self, args: &[&str]) -> Run {
        let out = run(self.proob().args(args));
        assert_eq!(out.code, 0, "proob {args:?}\n{}{}", out.stdout, out.stderr);
        out
    }

    fn work(&self) -> PathBuf {
        PathBuf::from(self.ok(&["shadow", "path"]).stdout.trim())
    }

    /// What `/proob:try` would write in the shadow: two properties of the
    /// fixture and the tokened tests that bind them.
    fn declare_in_shadow(&self) -> PathBuf {
        let work = self.work();
        std::fs::create_dir_all(work.join("proob/properties")).expect("mkdir");
        for id in [PASSING, FAILING] {
            let name = format!("proob/properties/{id}.toml");
            std::fs::copy(fixture_dir().join(&name), work.join(&name)).expect("copy property");
        }
        std::fs::copy(fixture_dir().join("tests/test_props.py"), work.join("tests/test_props.py")).expect("copy test");
        work
    }

    fn git_quiet(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.repo.root())
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Every path under the repository (its `.git` included) with size and
    /// mtime, and `git status --porcelain`, read without taking a lock.
    fn footprint(&self) -> Footprint {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, (u64, std::time::SystemTime)>) {
            for entry in std::fs::read_dir(dir).expect("read_dir").filter_map(Result::ok) {
                let meta = std::fs::symlink_metadata(entry.path()).expect("meta");
                out.insert(entry.path(), (meta.len(), meta.modified().expect("mtime")));
                if meta.is_dir() {
                    walk(&entry.path(), out);
                }
            }
        }
        let mut files = BTreeMap::new();
        walk(self.repo.root(), &mut files);
        (files, self.git_quiet(&["status", "--porcelain"]))
    }
}

type Footprint = (BTreeMap<PathBuf, (u64, std::time::SystemTime)>, String);

/// The repository-relative paths that differ from `before`; git status must not.
fn changed(w: &World, before: &Footprint, after: &str) -> Vec<String> {
    let now = w.footprint();
    assert_eq!(before.1, now.1, "git status changed after {after}");
    let mut paths: Vec<String> = before
        .0
        .keys()
        .chain(now.0.keys())
        .filter(|k| before.0.get(*k) != now.0.get(*k))
        .map(|k| k.strip_prefix(w.repo.root()).expect("inside").display().to_string())
        .collect();
    paths.dedup();
    paths
}

fn assert_untouched(w: &World, before: &Footprint, after: &str) {
    let paths = changed(w, before, after);
    assert!(paths.is_empty(), "the user's repository changed after {after}: {paths:?}");
}

fn with_python(mut cmd: Command) -> Command {
    // The adapter runs python3; the sandbox PATH holds only git.
    cmd.env("PATH", std::env::var_os("PATH").expect("PATH"));
    cmd
}

// ---------------------------------------------------------------------------
// open
// ---------------------------------------------------------------------------

#[test]
fn open_makes_a_shadow_and_changes_nothing_in_the_repository() {
    let w = World::new();
    let before = w.footprint();
    let out = w.ok(&["shadow", "open"]);
    assert_untouched(&w, &before, "open");
    let head = w.repo.rev("HEAD");
    assert_eq!(out.stdout.lines().count(), 1, "{}", out.stdout);
    assert!(out.stdout.starts_with("shadow: ready at "), "{}", out.stdout);
    assert!(out.stdout.contains(&format!("(your last commit {}, nothing tried yet)", &head[..7])), "{}", out.stdout);
    let work = w.work();
    assert!(work.starts_with(w.home.path().canonicalize().expect("home")), "{}", work.display());
    let dir = work.parent().expect("dir");
    let name = dir.file_name().expect("name").to_string_lossy().into_owned();
    let repo_name = w.repo.root().canonicalize().expect("canon").file_name().expect("n").to_string_lossy().into_owned();
    assert!(name.starts_with(&format!("{repo_name}-")) && name.len() == repo_name.len() + 9, "{name}");
    assert_eq!(std::fs::read_to_string(dir.join("base")).expect("base").trim(), head);
    assert!(
        std::fs::read_to_string(work.join(".proob-shadow"))
            .expect("marker")
            .contains(&w.repo.root().canonicalize().expect("c").display().to_string())
    );
    assert!(work.join("proob/config.toml").is_file(), "init ran in the shadow");
    let patch = std::fs::read_to_string(dir.join("overlay.patch")).expect("overlay");
    assert!(
        patch.contains("proob/config.toml") && !patch.contains(".proob-shadow") && !patch.contains("a/.gitignore"),
        "{patch}"
    );
    let git = |args: &[&str]| {
        String::from_utf8_lossy(&Command::new("git").args(args).current_dir(&work).output().expect("git").stdout)
            .trim()
            .to_string()
    };
    assert_eq!(git(&["rev-parse", "HEAD"]), head);
    assert_eq!(git(&["remote", "get-url", "--push", "origin"]), "no-push");
}

#[test]
fn open_twice_is_idempotent() {
    let w = World::new();
    w.ok(&["shadow", "open"]);
    let dir = w.work().parent().expect("dir").to_path_buf();
    let patch = std::fs::read(dir.join("overlay.patch")).expect("patch");
    let before = w.footprint();
    let again = w.ok(&["shadow", "open"]);
    assert_untouched(&w, &before, "a second open");
    assert!(again.stdout.contains("up to date, 0 properties"), "{}", again.stdout);
    assert_eq!(std::fs::read(dir.join("overlay.patch")).expect("patch"), patch);
}

#[test]
fn open_says_uncommitted_changes_are_left_out() {
    let w = World::new();
    w.repo.write("interval/merge.py", &(w.repo.read("interval/merge.py") + "# wip\n"));
    let before = w.footprint();
    let out = w.ok(&["shadow", "open"]);
    assert_untouched(&w, &before, "open with uncommitted changes");
    assert_eq!(
        out.stdout.lines().nth(1),
        Some("shadow: uncommitted changes in your repository are not included; commit them to try them")
    );
    assert!(
        !w.work().join("interval/merge.py").is_file()
            || !std::fs::read_to_string(w.work().join("interval/merge.py")).expect("r").contains("# wip")
    );
}

#[test]
fn open_carries_the_overlay_to_a_new_commit_and_keeps_the_old_shadow_on_a_conflict() {
    let w = World::new();
    w.ok(&["shadow", "open"]);
    let work = w.declare_in_shadow();
    w.repo.write("README.md", "notes\n");
    let second = w.repo.commit("unrelated", NOW);
    let out = w.ok(&["shadow", "open"]);
    assert!(
        out.stdout.contains(&format!("(your last commit {}, 2 properties carried over)", &second[..7])),
        "{}",
        out.stdout
    );
    assert!(work.join(format!("proob/properties/{FAILING}.toml")).is_file());
    assert!(
        std::fs::read_to_string(work.join("tests/test_props.py")).expect("r").contains(&format!("# proob:{FAILING}"))
    );
    assert!(work.join("README.md").is_file(), "the new commit is in the shadow");

    // The user rewrites the very lines the overlay changed.
    let text = w
        .repo
        .read("tests/test_props.py")
        .replace("@given(st.lists(st.integers(min_value=0, max_value=10)))", "@given(st.lists(st.integers(0, 3)))");
    w.repo.write("tests/test_props.py", &text);
    w.repo.commit("narrow the strategy", NOW + 1);
    let before = w.footprint();
    let out = run(w.proob().args(["shadow", "open"]));
    assert_untouched(&w, &before, "a conflicting open");
    assert_eq!(out.code, 2, "{}{}", out.stdout, out.stderr);
    assert_eq!(out.stderr.lines().count(), 1, "{}", out.stderr);
    assert!(
        out.stderr.starts_with("shadow: your latest commit changes tests/test_props.py, which the shadow also changed; the shadow is kept as it was;"),
        "{}",
        out.stderr
    );
    let base = std::fs::read_to_string(work.parent().expect("d").join("base")).expect("base");
    assert_eq!(base.trim(), second, "the old shadow is untouched");
    assert!(work.join(format!("proob/properties/{FAILING}.toml")).is_file());
}

// ---------------------------------------------------------------------------
// errors
// ---------------------------------------------------------------------------

#[test]
fn outside_a_git_repository_the_error_says_so_in_one_line() {
    let dir = tempfile::tempdir().expect("dir");
    let w = World::new();
    let mut cmd = proob(w.repo.gh_config.path());
    let out = run(cmd.env("PROOB_SHADOW_HOME", w.home.path()).arg("--repo").arg(dir.path()).args(["shadow", "open"]));
    assert_eq!(out.code, 2);
    assert_eq!(out.stderr.lines().count(), 1, "{}", out.stderr);
    assert!(
        out.stderr.starts_with("shadow: ")
            && out.stderr.contains("is not a git repository; run this inside your repository or pass --repo <path>"),
        "{}",
        out.stderr
    );
}

#[test]
fn open_fails_loudly_when_nothing_in_the_repository_can_be_checked() {
    let repo = TempRepo::new();
    repo.write("go.mod", "module example.com/svc\n");
    repo.write("main.go", "package main\n\nfunc main() {}\n");
    repo.commit("a Go service", NOW - DAY);
    let w = World { repo, home: tempfile::tempdir().expect("home") };
    let before = w.footprint();
    let out = run(w.proob().args(["shadow", "open"]));
    assert_untouched(&w, &before, "open");
    assert_eq!(out.code, 2, "{}{}", out.stdout, out.stderr);
    assert_eq!(out.stderr.lines().count(), 1, "{}", out.stderr);
    assert!(
        out.stderr.starts_with("shadow: proob cannot check this repository: it found no Python or Rust project")
            && out.stderr.contains("nothing was tried"),
        "{}",
        out.stderr
    );
}

#[test]
fn without_a_shadow_every_shadow_command_says_how_to_make_one() {
    let w = World::new();
    let expected = "shadow: no shadow yet for this repository; run /proob:try or proob shadow open\n";
    for args in [
        &["--shadow", "status"][..],
        &["--shadow", "validate"],
        &["shadow", "report"],
        &["shadow", "path"],
        &["shadow", "adopt"],
    ] {
        let out = run(w.proob().args(args));
        assert_eq!((out.code, out.stderr.as_str()), (2, expected), "{args:?}");
    }
}

// ---------------------------------------------------------------------------
// --shadow
// ---------------------------------------------------------------------------

#[test]
fn the_shadow_flag_runs_each_command_in_the_shadow() {
    let w = World::new();
    w.ok(&["shadow", "open"]);
    w.declare_in_shadow();
    let plain = run(w.proob().args(["status"]));
    assert_eq!(plain.code, 2, "the repository itself has no proob/: {}", plain.stderr);
    let before = w.footprint();

    let status = w.ok(&["--shadow", "status", "--json"]).json();
    let words: Vec<&str> =
        status["properties"].as_array().expect("rows").iter().map(|r| r["word"].as_str().expect("word")).collect();
    assert_eq!(words.len(), 2, "{status}");
    assert!(w.ok(&["--shadow", "validate"]).stdout.starts_with("proob validate: ok (2 property files)"));
    let hash = w.ok(&["--shadow", "hash", FAILING, "--json"]).json();
    assert_eq!(hash["scope_files"], serde_json::json!(["interval/bucket.py"]));
    assert!(hash["binding_files"].as_array().expect("files").contains(&Value::from("tests/test_props.py")));
    let head = w.repo.rev("HEAD");
    let check = run(w.proob().args(["--shadow", "check", "--base", &head, "--head", &head, "--json"]));
    assert!(check.code == 0 || check.code == 1, "{}", check.stderr);
    assert_eq!(check.json()["properties"].as_array().map(Vec::len), Some(2));
    let out = run(with_python(w.proob()).args(["--shadow", "run", "check", FAILING, "--seed", "3", "--json"]));
    assert_eq!(out.code, 1, "a counterexample: {}{}", out.stdout, out.stderr);
    assert_eq!(out.json()["outcome"]["result"], "counterexample");
    assert_untouched(&w, &before, "--shadow status, validate, hash, check, run");
    let results =
        std::fs::read_to_string(w.work().join("proob/.cache/results/results.jsonl")).expect("a shadow run is recorded");
    assert_eq!(results.lines().count(), 1);
}

#[test]
fn inside_the_shadow_commands_find_the_repository_through_the_marker() {
    let w = World::new();
    w.ok(&["shadow", "open"]);
    let work = w.work();
    let mut cmd = proob(w.repo.gh_config.path());
    cmd.env("PROOB_SHADOW_HOME", w.home.path()).arg("--repo").arg(&work).args(["shadow", "path"]);
    let out = run(&mut cmd);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(PathBuf::from(out.stdout.trim()), work);
}

// ---------------------------------------------------------------------------
// report
// ---------------------------------------------------------------------------

#[test]
fn the_report_shows_each_result_the_replay_and_how_to_make_it_real() {
    let w = World::new();
    w.ok(&["shadow", "open"]);
    w.declare_in_shadow();
    let fails = run(with_python(w.proob()).args(["--shadow", "run", "check", FAILING, "--seed", "3"]));
    assert_eq!(fails.code, 1, "{}{}", fails.stdout, fails.stderr);
    let before = w.footprint();
    let out = run(w.proob().args(["shadow", "report"]));
    assert_untouched(&w, &before, "report");
    assert_eq!(out.code, 1, "a property fails: {}{}", out.stdout, out.stderr);
    let text = &out.stdout;
    assert_eq!(
        text.lines().next(),
        Some("shadow: 2 properties tried · 1 fails · 1 not run yet · nothing in your repository was changed"),
        "{text}"
    );
    assert!(text.contains("every property is Proposed (no one has approved it: shadow results are local)"), "{text}");
    assert!(text.contains("fails: input takes=[6]"), "{text}");
    assert!(text.contains(&format!("· replay: proob --shadow run check {FAILING} --seed 3")), "{text}");
    assert!(text.contains(&format!("not run yet: proob --shadow run check {PASSING}")), "{text}");
    assert!(
        text.contains(
            "Make it real:\n  0. fix the code or drop the failing property, and check or drop the property with no check result \
             (each line says how) before adopting\n  1. proob shadow adopt — "
        ),
        "{text}"
    );
    assert!(
        text.contains("2. add your GitHub remote") && text.contains("3. make the `proob gate` check required on main"),
        "{text}"
    );
    for word in ["Holds", "holds", "verified", "approved"] {
        assert!(!text.replace("has approved it", "").contains(word), "{word}: {text}");
    }
    let dir = w.work().parent().expect("d").to_path_buf();
    assert_eq!(std::fs::read_to_string(dir.join("report.md")).expect("md"), *text);

    let json = w.proob().args(["shadow", "report", "--json"]).output().expect("run");
    let j: Value = serde_json::from_slice(&json.stdout).expect("json");
    assert_eq!(j["schema_version"], 1);
    assert_eq!(
        j["verdict"],
        serde_json::json!({ "word": "Proposed", "why": "no one has approved it: shadow results are local" })
    );
    assert_eq!(j["counts"], serde_json::json!({ "tried": 2, "fails": 1, "passes": 0, "not_run": 1, "errored": 0 }));
    let row = j["properties"].as_array().expect("rows").iter().find(|r| r["id"] == FAILING).expect("row").clone();
    assert_eq!(row["verdict"], "Proposed");
    assert_eq!(row["result"]["state"], "fails");
    assert_eq!(row["result"]["seed"], "3");
    assert_eq!(row["strength"]["state"], "not_scored");
    assert_eq!(j["make_it_real"].as_array().map(Vec::len), Some(4));
    assert_eq!(j["adopted_branch"], Value::Null);
    assert_eq!(std::fs::read_to_string(dir.join("report.json")).expect("json"), String::from_utf8_lossy(&json.stdout));
}

// ---------------------------------------------------------------------------
// adopt
// ---------------------------------------------------------------------------

#[test]
fn adopt_makes_one_branch_with_exactly_the_shadows_files_and_leaves_the_checkout_alone() {
    let w = World::new();
    w.ok(&["shadow", "open"]);
    w.declare_in_shadow();
    let (head, branch_before) = (w.repo.rev("HEAD"), w.git_quiet(&["symbolic-ref", "HEAD"]));
    let status_before = w.git_quiet(&["status", "--porcelain"]);
    let before = w.footprint();
    let out = w.ok(&["shadow", "adopt", "--branch", "proob/try"]);
    let beyond_a_branch: Vec<String> = changed(&w, &before, "adopt")
        .into_iter()
        .filter(|p| ![".git/objects", ".git/refs", ".git/logs"].iter().any(|a| p.starts_with(a)))
        .collect();
    assert!(beyond_a_branch.is_empty(), "adopt changed more than one branch: {beyond_a_branch:?}");
    assert!(out.stdout.starts_with("shadow: adopt creates branch proob/try from your last commit"), "{}", out.stdout);
    assert!(
        out.stdout.contains(
            "shadow: made branch proob/try in your repository (2 properties, 8 files); your checkout is unchanged"
        ),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("next: git remote add origin <your GitHub URL>, then git push -u origin proob/try"),
        "{}",
        out.stdout
    );
    assert_eq!(w.repo.rev("HEAD"), head);
    assert_eq!(w.git_quiet(&["symbolic-ref", "HEAD"]), branch_before);
    assert_eq!(w.git_quiet(&["status", "--porcelain"]), status_before);
    assert_eq!(w.repo.rev("proob/try^"), head);
    let files: Vec<String> =
        w.git_quiet(&["diff", "--name-only", &head, "proob/try"]).lines().map(str::to_string).collect();
    assert_eq!(
        files,
        [
            ".claude/settings.json",
            ".github/workflows/proob-evidence.yml",
            ".github/workflows/proob-gate.yml",
            ".gitignore",
            "proob/config.toml",
            &format!("proob/properties/{PASSING}.toml"),
            &format!("proob/properties/{FAILING}.toml"),
            "tests/test_props.py",
        ]
    );
    assert!(w.git_quiet(&["show", "proob/try:.gitignore"]).contains("proob/.cache/"));

    let again = run(w.proob().args(["shadow", "adopt", "--branch", "proob/try"]));
    assert_eq!(again.code, 2);
    assert_eq!(
        again.stderr,
        "shadow: branch proob/try already exists in your repository; pick another name with --branch <name>\n"
    );
    let report = run(w.proob().args(["shadow", "report"]));
    assert!(
        report
            .stdout
            .lines()
            .next()
            .unwrap_or("")
            .ends_with("adopt made branch proob/try; nothing else in your repository was changed"),
        "{}",
        report.stdout
    );
    let hook: Value = serde_json::from_str(&run(w.proob().args(["hook", "session-start"])).stdout).expect("json");
    assert!(
        hook["systemMessage"]
            .as_str()
            .expect("line")
            .ends_with("adopt made branch proob/try; nothing else in this repository was changed"),
        "{hook}"
    );
    assert!(
        w.ok(&["shadow", "clean"]).stdout.ends_with("(branch proob/try that adopt made stays in your repository)\n")
    );
}

#[test]
fn adopt_of_one_property_refuses_a_test_file_that_binds_another() {
    let w = World::new();
    w.ok(&["shadow", "open"]);
    w.declare_in_shadow();
    let out = run(w.proob().args(["shadow", "adopt", PASSING]));
    assert_eq!(out.code, 2);
    assert_eq!(
        out.stderr,
        format!(
            "shadow: tests/test_props.py also binds {FAILING}, which you did not choose; adopt it too or move its test to a file of its own\n"
        )
    );
}

#[test]
fn adopt_refuses_during_a_rebase_or_a_merge() {
    let w = World::new();
    w.ok(&["shadow", "open"]);
    w.declare_in_shadow();
    let git_dir = w.repo.root().join(".git");
    std::fs::create_dir_all(git_dir.join("rebase-merge")).expect("mkdir");
    let out = run(w.proob().args(["shadow", "adopt"]));
    assert_eq!(out.code, 2);
    assert_eq!(
        out.stderr,
        "shadow: a rebase is in progress in your repository; finish it (git rebase --continue) or abort it (git rebase --abort), then run proob shadow adopt again\n"
    );
    std::fs::remove_dir_all(git_dir.join("rebase-merge")).expect("rm");
    std::fs::write(git_dir.join("MERGE_HEAD"), w.repo.rev("HEAD")).expect("write");
    let out = run(w.proob().args(["shadow", "adopt"]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.starts_with("shadow: a merge is in progress in your repository;"), "{}", out.stderr);
    assert_eq!(w.git_quiet(&["branch", "--list", "proob/*"]), "");
}

// ---------------------------------------------------------------------------
// clean, the hook
// ---------------------------------------------------------------------------

#[test]
fn clean_removes_the_shadow_and_nothing_else() {
    let w = World::new();
    w.ok(&["shadow", "open"]);
    let dir = w.work().parent().expect("d").to_path_buf();
    let before = w.footprint();
    let out = w.ok(&["shadow", "clean"]);
    assert_untouched(&w, &before, "clean");
    assert!(
        out.stdout.starts_with("shadow: removed ") && out.stdout.contains("(your repository was not touched)"),
        "{}",
        out.stdout
    );
    assert!(!dir.exists());
    assert_eq!(w.ok(&["shadow", "clean"]).stdout, "shadow: nothing to remove; this repository has no shadow\n");
    w.ok(&["shadow", "open"]);
    assert!(w.ok(&["shadow", "clean", "--all"]).stdout.starts_with("shadow: removed "));
    assert!(!dir.exists());
}

#[test]
fn the_session_hook_in_a_repository_with_a_shadow_prints_its_line() {
    let w = World::new();
    let mut quiet = w.proob();
    quiet.args(["hook", "session-start"]).current_dir(w.repo.root());
    assert_eq!(run(&mut quiet).stdout, "", "no proob/ and no shadow: nothing");
    w.ok(&["shadow", "open"]);
    w.declare_in_shadow();
    let before = w.footprint();
    let out = run(w.proob().args(["hook", "session-start"]));
    assert_untouched(&w, &before, "the session hook");
    assert_eq!(out.code, 0);
    let json: Value = serde_json::from_str(&out.stdout).expect("json");
    assert_eq!(
        json["systemMessage"],
        "proob (shadow): 2 properties tried, 2 not run yet · /proob:try to see them · nothing in this repository was changed"
    );
}
