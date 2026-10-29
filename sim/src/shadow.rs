//! Shadow mode end to end, over a copy of the Python fixture
//! (`tests/fixtures/py-repo`) that has never seen proob: open, a property
//! and its tokened test written into the shadow as `/proob:try` would, a
//! failing `--shadow run check`, the report, a new commit carried over,
//! adopt, clean. After every step the user's repository is compared with its
//! state before the step: every path (its `.git` included) with size and
//! mtime, and `git status --porcelain`. Only adopt may change anything, and
//! only by adding one branch: refs, reflogs, and objects under `.git`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::github::ALICE;
use crate::outcome::Outcome;
use crate::world::{Ctx, DAY, Fallible, NOW, Run, World, copy_tree, first_line, run};

const BUCKET: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XPB";
const PROPERTY: &str = "proob/properties/01K8V5NQ4A0F3R7Y2M9D6P1XPB.toml";
const TEST: &str = "tests/test_bucket.py";
const BRANCH: &str = "proob/adopt-2026-10-28";
/// What adopt must put on the branch: nothing more, nothing less.
const ADOPTED: [&str; 7] = [
    ".claude/settings.json",
    ".github/workflows/proob-evidence.yml",
    ".github/workflows/proob-gate.yml",
    ".gitignore",
    "proob/config.toml",
    PROPERTY,
    TEST,
];
/// The tokened test `/proob:try` writes for the bucket property: it fails.
const TEST_TEXT: &str = "from hypothesis import given\nfrom hypothesis import strategies as st\n\nfrom interval.bucket import TokenBucket\n\n\n# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPB\n@given(st.lists(st.integers(min_value=0, max_value=10)))\ndef test_bucket_never_goes_negative(takes):\n    bucket = TokenBucket(capacity=5)\n    for n in takes:\n        bucket.take(n)\n        assert bucket.tokens >= 0\n";

type Footprint = (BTreeMap<PathBuf, (u64, SystemTime)>, String);

pub fn run_scenario(ctx: &Ctx) -> Outcome {
    match scenario(ctx) {
        Ok(()) => {
            Outcome::Ok(format!("{} steps, the repository untouched after each but adopt's one branch", STEPS.len()))
        }
        Err(e) => Outcome::Failed(e),
    }
}

struct Shadow {
    w: World,
    home: PathBuf,
}

impl Shadow {
    fn proob(&self, args: &[&str]) -> Fallible<Run> {
        let mut cmd = self.w.proob(&self.w.repo.dir)?;
        cmd.env("PROOB_SHADOW_HOME", &self.home).args(args);
        run(&mut cmd)
    }

    /// With the host's PATH: the Python adapter runs python3.
    fn proob_tools(&self, args: &[&str]) -> Fallible<Run> {
        let mut cmd = self.w.proob(&self.w.repo.dir)?;
        cmd.env("PROOB_SHADOW_HOME", &self.home).env("PATH", std::env::var_os("PATH").unwrap_or_default()).args(args);
        run(&mut cmd)
    }

    fn ok(&self, args: &[&str]) -> Fallible<String> {
        let out = self.proob(args)?;
        match out.code {
            0 => Ok(out.stdout),
            code => Err(format!("proob {}: exit {code}: {}", args.join(" "), first_line(&out.stderr))),
        }
    }

    fn footprint(&self) -> Fallible<Footprint> {
        let mut files = BTreeMap::new();
        walk(&self.w.repo.dir, &mut files)?;
        let out = run(Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.w.repo.dir)
            .env("GIT_OPTIONAL_LOCKS", "0"))?;
        Ok((files, out.stdout))
    }

    /// The paths that differ from `before`, relative to the repository.
    fn changed(&self, before: &Footprint, step: &str) -> Fallible<Vec<String>> {
        let now = self.footprint()?;
        if now.1 != before.1 {
            return Err(format!("{step}: git status changed: {:?} -> {:?}", before.1, now.1));
        }
        let keys = before.0.keys().chain(now.0.keys());
        let mut changed: Vec<String> = keys
            .filter(|k| before.0.get(*k) != now.0.get(*k))
            .filter_map(|k| k.strip_prefix(&self.w.repo.dir).ok().map(|p| p.display().to_string()))
            .collect();
        changed.dedup();
        Ok(changed)
    }

    fn untouched(&self, before: &Footprint, step: &str) -> Fallible<()> {
        match self.changed(before, step)? {
            c if c.is_empty() => Ok(()),
            c => Err(format!("{step}: the user's repository changed: {c:?}")),
        }
    }
}

fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, (u64, SystemTime)>) -> Fallible<()> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(Result::ok) {
        let meta = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        out.insert(entry.path(), (meta.len(), meta.modified().map_err(|e| e.to_string())?));
        if meta.is_dir() {
            walk(&entry.path(), out)?;
        }
    }
    Ok(())
}

fn expect(what: &str, text: &str, needle: &str) -> Fallible<()> {
    if text.contains(needle) { Ok(()) } else { Err(format!("{what}: expected {needle:?} in:\n{text}")) }
}

/// The fixture as a repository that has never seen proob.
fn user_repo(s: &Shadow) -> Fallible<()> {
    let fixture = s.w.ctx.root.join("tests/fixtures/py-repo");
    copy_tree(&fixture, &s.w.repo.dir)?;
    fs::remove_dir_all(s.w.repo.dir.join("proob")).map_err(|e| e.to_string())?;
    for test in ["tests/test_props.py", "tests/test_vacuous.py"] {
        let text: String =
            s.w.repo.read(test)?.lines().filter(|l| !l.starts_with("# proob:")).map(|l| format!("{l}\n")).collect();
        s.w.repo.write(test, &text)?;
    }
    s.w.repo.git(&["config", "user.name", ALICE.login])?;
    s.w.repo.git(&["config", "user.email", &ALICE.email()])?;
    s.w.repo.commit("the code, before proob", &ALICE, NOW - 2 * DAY).map(drop)
}

type Step = fn(&Shadow) -> Fallible<()>;

const STEPS: [Step; 7] = [open, declare_in_shadow, failing_check, report, carried_over, adopt, clean];

fn scenario(ctx: &Ctx) -> Fallible<()> {
    let w = World::new(ctx)?;
    let s = Shadow { home: w.path("shadow-home"), w };
    user_repo(&s)?;
    STEPS.iter().try_for_each(|step| step(&s))
}

impl Shadow {
    fn work(&self) -> Fallible<PathBuf> {
        self.ok(&["shadow", "path"]).map(|p| PathBuf::from(p.trim()))
    }

    fn short_head(&self) -> Fallible<String> {
        self.w.repo.rev("HEAD").map(|h| h[..7].to_owned())
    }
}

fn open(s: &Shadow) -> Fallible<()> {
    let before = s.footprint()?;
    let opened = s.ok(&["shadow", "open"])?;
    s.untouched(&before, "open")?;
    expect("open", &opened, &format!("(your last commit {}, nothing tried yet)", s.short_head()?))
}

/// What `/proob:try` writes, in the shadow only.
fn declare_in_shadow(s: &Shadow) -> Fallible<()> {
    let (work, before) = (s.work()?, s.footprint()?);
    let fixture = s.w.ctx.root.join("tests/fixtures/py-repo");
    fs::create_dir_all(work.join("proob/properties")).map_err(|e| e.to_string())?;
    fs::copy(fixture.join(PROPERTY), work.join(PROPERTY)).map_err(|e| e.to_string())?;
    fs::write(work.join(TEST), TEST_TEXT).map_err(|e| e.to_string())?;
    s.untouched(&before, "writing into the shadow")
}

fn failing_check(s: &Shadow) -> Fallible<()> {
    let before = s.footprint()?;
    let check = s.proob_tools(&["--shadow", "run", "check", BUCKET, "--seed", "3"])?;
    s.untouched(&before, "--shadow run check")?;
    match check.code {
        1 => Ok(()),
        code => Err(format!(
            "--shadow run check: exit {code} (want 1, a counterexample): {}{}",
            check.stdout,
            first_line(&check.stderr)
        )),
    }
}

fn report(s: &Shadow) -> Fallible<()> {
    let before = s.footprint()?;
    let text = s.proob(&["shadow", "report"])?.stdout;
    s.untouched(&before, "report")?;
    [
        "shadow: 1 property tried · 1 fails · nothing in your repository was changed\n",
        &format!("· replay: proob --shadow run check {BUCKET} --seed 3"),
        "every property is Proposed (no one has approved it: shadow results are local)",
        "Make it real:\n  0. fix the code or drop the failing property before adopting\n  1. proob shadow adopt — ",
        "  3. make the `proob gate` check required on main",
    ]
    .iter()
    .try_for_each(|needle| expect("report", &text, needle))
}

/// A new commit in the repository, and the overlay carried over to it.
fn carried_over(s: &Shadow) -> Fallible<()> {
    s.w.repo.write("README.md", "interval utilities\n")?;
    s.w.repo.commit("a readme", &ALICE, NOW - DAY)?;
    let before = s.footprint()?;
    let reopened = s.ok(&["shadow", "open"])?;
    s.untouched(&before, "open after a new commit")?;
    expect("open again", &reopened, &format!("(your last commit {}, 1 property carried over)", s.short_head()?))?;
    let work = s.work()?;
    match work.join(TEST).is_file() && work.join("README.md").is_file() {
        true => Ok(()),
        false => Err("open again: the shadow lacks the carried test or the new commit".to_owned()),
    }
}

fn adopt(s: &Shadow) -> Fallible<()> {
    let (head, branch) = (s.w.repo.rev("HEAD")?, s.w.repo.git(&["symbolic-ref", "HEAD"])?);
    let before = s.footprint()?;
    let adopted = s.ok(&["shadow", "adopt"])?;
    let beyond: Vec<String> = s.changed(&before, "adopt")?.into_iter().filter(|p| !adopt_may_change(p)).collect();
    if !beyond.is_empty() {
        return Err(format!("adopt changed more than one branch: {beyond:?}"));
    }
    expect(
        "adopt",
        &adopted,
        &format!("shadow: made branch {BRANCH} in your repository (1 property, 7 files); your checkout is unchanged"),
    )?;
    let tree = s.w.repo.git(&["diff", "--name-only", &head, BRANCH])?;
    if tree.lines().collect::<Vec<_>>() != ADOPTED {
        return Err(format!("adopt: the branch carries {tree:?}, not {ADOPTED:?}"));
    }
    match s.w.repo.git(&["symbolic-ref", "HEAD"])? == branch && s.w.repo.rev("HEAD")? == head {
        true => Ok(()),
        false => Err("adopt: the user's checkout moved".to_owned()),
    }
}

fn clean(s: &Shadow) -> Fallible<()> {
    let (work, before) = (s.work()?, s.footprint()?);
    let cleaned = s.ok(&["shadow", "clean"])?;
    s.untouched(&before, "clean")?;
    expect("clean", &cleaned, &format!("(branch {BRANCH} that adopt made stays in your repository)"))?;
    match work.exists() {
        true => Err("clean: the shadow is still there".to_owned()),
        false => Ok(()),
    }
}

/// A new branch is a ref, its reflog, and objects; the directories holding them.
fn adopt_may_change(path: &str) -> bool {
    [".git/objects", ".git/refs", ".git/logs"].iter().any(|p| path.starts_with(p))
}
