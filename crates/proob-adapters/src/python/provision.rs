//! proob's own Python environment (`proob/.cache/python`), made and filled
//! by proob so nobody has to: the pinned tools first, then each project's own
//! requirement files, whose pins of pytest and Hypothesis therefore win. The
//! environment is made by the first of `makers` the pins install on. A stamp
//! inside it holds the pins and the requirement files it was filled from, so
//! it is filled again only when one of them changed, whether or not every
//! one of them installed.
use super::tools::{Need, OWN_ENV, PINS, Resolution, own_python, resolve};
use crate::argv::Command;
use crate::rust::tools::find_on_path;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The Python the rendered workflows set up; uv is asked for the same one.
pub const GATE_PYTHON: &str = "3.12";
/// After the gate's: the other interpreters the pins install on, by name.
const OTHER_PYTHONS: [&str; 3] = ["python3.13", "python3.11", "python3"];
/// What describes a project pip can install, and its requirement files by
/// their usual names.
const PACKAGE_FILES: [&str; 2] = ["pyproject.toml", "setup.py"];
const REQUIREMENTS: [&str; 5] = [
    "requirements.txt",
    "requirements-test.txt",
    "requirements-dev.txt",
    "test-requirements.txt",
    "dev-requirements.txt",
];
const PINS_FILE: &str = "proob/.cache/python-tools.txt";
const STAMP: &str = "proob-filled-from.txt";
/// One venv or one pip run; a project's requirements can take minutes.
const STEP_TIMEOUT: Duration = Duration::from_secs(900);
const NO_PYTHON: &str = "no python3 and no uv on PATH";
/// A repository can commit either as a link; the environment, and its
/// removal, would then land outside the repository.
const LINKABLE: [&str; 2] = ["proob", "proob/.cache"];

/// How the environment comes to exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Maker {
    /// It is there already.
    Kept,
    /// `<python> -m venv`.
    Venv(PathBuf),
    /// `uv venv --seed --python <the gate's>`.
    Uv(PathBuf),
}

/// In the order tried: the environment there is, the gate's Python, uv (which
/// brings that Python when the machine lacks it), then any other `python3`.
pub fn makers(kept: bool, find: &dyn Fn(&str) -> Option<PathBuf>) -> Vec<Maker> {
    let venv = |name: &str| find(name).map(Maker::Venv);
    kept.then_some(Maker::Kept)
        .into_iter()
        .chain(venv(&format!("python{GATE_PYTHON}")))
        .chain(find("uv").map(Maker::Uv))
        .chain(OTHER_PYTHONS.iter().filter_map(|name| venv(name)))
        .collect()
}

/// Whether `provide` has work for the Python projects at `roots`: proob's
/// environment was filled from other pins or requirements, or there is none
/// and `python3` lacks a tool. A `python3` that has them all is left to serve.
pub fn stale(repo: &Path, roots: &[PathBuf]) -> bool {
    match own_python(repo) {
        Some(_) => std::fs::read_to_string(repo.join(OWN_ENV).join(STAMP)).ok() != Some(filled_from(roots)),
        None => roots
            .iter()
            .any(|root| !matches!(resolve(repo, root, &[Need::Pytest, Need::Mutmut], &[]), Resolution::Ready(_))),
    }
}

/// Makes the environment where the pins install, then installs each project.
/// The stamp is written even when a project did not install: the same files
/// would fail the same way at every open.
pub fn provide(repo: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if LINKABLE.iter().any(|dir| repo.join(dir).is_symlink()) {
        return Err(format!("{OWN_ENV} is behind a symbolic link"));
    }
    write(&repo.join(PINS_FILE), PINS)?;
    first_made(&makers(own_python(repo).is_some(), &|name| find_on_path(name, &[])), repo)?;
    let failures = roots.iter().filter_map(|root| install_project(repo, root).err()).collect();
    write(&repo.join(OWN_ENV).join(STAMP), &filled_from(roots))?;
    none(failures)
}

fn none(failures: Vec<String>) -> Result<(), String> {
    if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
}

/// The pins and every file a project is installed from, as the stamp keeps them.
fn filled_from(roots: &[PathBuf]) -> String {
    let files = roots.iter().flat_map(|root| PACKAGE_FILES.iter().chain(&REQUIREMENTS).map(move |f| root.join(f)));
    std::iter::once(PINS.to_string())
        .chain(files.filter_map(|f| std::fs::read_to_string(&f).ok().map(|text| format!("# {}\n{text}", f.display()))))
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_made(makers: &[Maker], repo: &Path) -> Result<(), String> {
    match makers.split_first() {
        None => Err(NO_PYTHON.to_string()),
        Some((maker, [])) => make(maker, repo),
        Some((maker, rest)) => {
            make(maker, repo).or_else(|e| first_made(rest, repo).map_err(|later| format!("{e}; {later}")))
        }
    }
}

/// The environment by `maker`, with the pinned tools in it.
fn make(maker: &Maker, repo: &Path) -> Result<(), String> {
    let env = repo.join(OWN_ENV);
    let made = match maker {
        Maker::Kept => Ok(()),
        Maker::Venv(python) => remade(&env, Command::new(python, STEP_TIMEOUT).arg("-m").arg("venv")),
        Maker::Uv(uv) => {
            remade(&env, Command::new(uv, STEP_TIMEOUT).arg("venv").arg("--seed").arg("--python").arg(GATE_PYTHON))
        }
    };
    let filled = made.and_then(|()| pip(repo, repo)).and_then(|pip| run(&pip.own("-r", &repo.join(PINS_FILE))));
    // A new environment the pins did not install on would be taken for a usable one.
    if filled.is_err() && *maker != Maker::Kept {
        let _ = std::fs::remove_dir_all(&env);
    }
    filled
}

/// Removed first: neither venv nor every uv replaces an environment in place.
fn remade(env: &Path, maker: Command) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(env);
    run(&maker.own_path(env))
}

fn pip(repo: &Path, cwd: &Path) -> Result<Command, String> {
    let python = own_python(repo).ok_or_else(|| format!("{OWN_ENV} has no interpreter"))?;
    Ok(["-m", "pip", "install", "--quiet", "--disable-pip-version-check"]
        .into_iter()
        .fold(Command::new(python, STEP_TIMEOUT).cwd(cwd), Command::arg))
}

/// The project itself when it is a package, then its requirement files: each
/// in its own pip run, so one that fails keeps no other out, and from the
/// project's directory, where a requirement file's relative paths start.
/// A `pyproject.toml` that only configures tools is no package.
fn install_project(repo: &Path, root: &Path) -> Result<(), String> {
    let pyproject = std::fs::read_to_string(root.join("pyproject.toml")).unwrap_or_default();
    let package =
        root.join("setup.py").is_file() || ["[project]", "[build-system]"].iter().any(|t| pyproject.contains(t));
    let files = REQUIREMENTS.iter().map(|f| ("-r", root.join(f))).filter(|(_, f)| f.is_file());
    let steps = package.then(|| ("-e", root.to_path_buf())).into_iter().chain(files);
    let step = |(flag, path): (&'static str, PathBuf)| pip(repo, root).and_then(|pip| run(&pip.own(flag, &path)));
    none(steps.filter_map(|s| step(s).err()).collect())
        .map_err(|e| format!("the project at {} did not install: {e}", root.display()))
}

fn run(command: &Command) -> Result<(), String> {
    let out = command.run().map_err(|e| e.to_string())?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it failed without a message");
    let program = command.program().file_name().unwrap_or_default().to_string_lossy();
    if out.success() { Ok(()) } else { Err(format!("{program}: {}", last.trim())) }
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    let wrote = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(path, text));
    wrote.map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gates_python_is_tried_before_any_other() {
        let all = |name: &str| Some(PathBuf::from(format!("/bin/{name}")));
        let venv = |name: &str| Maker::Venv(PathBuf::from(format!("/bin/{name}")));
        assert_eq!(
            makers(true, &all),
            vec![
                Maker::Kept,
                venv("python3.12"),
                Maker::Uv("/bin/uv".into()),
                venv("python3.13"),
                venv("python3.11"),
                venv("python3")
            ]
        );
        let only_python3 = |name: &str| (name == "python3").then(|| PathBuf::from("/bin/python3"));
        assert_eq!(makers(false, &only_python3), vec![venv("python3")]);
        assert_eq!(first_made(&makers(false, &|_| None), Path::new("/nowhere")), Err(NO_PYTHON.to_string()));
    }

    /// Nothing is written through a `proob` the repository committed as a link.
    #[cfg(unix)]
    #[test]
    fn an_environment_behind_a_link_is_not_made() {
        let repo = tempfile::tempdir().expect("tempdir");
        let elsewhere = tempfile::tempdir().expect("tempdir");
        std::os::unix::fs::symlink(elsewhere.path(), repo.path().join("proob")).expect("link");
        assert!(provide(repo.path(), &[]).is_err());
        assert!(std::fs::read_dir(elsewhere.path()).expect("dir").next().is_none());
    }

    /// The stamp follows the pins and the requirement files: an environment
    /// filled from other ones is stale, and one filled from these is not.
    #[cfg(unix)]
    #[test]
    fn an_environment_is_stale_until_it_was_filled_from_these_files() {
        let repo = tempfile::tempdir().expect("tempdir");
        let root = repo.path().join("svc");
        let env = repo.path().join(OWN_ENV);
        std::fs::create_dir_all(env.join("bin")).expect("mkdir");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(env.join("bin/python3"), "").expect("write");
        std::fs::write(root.join("requirements-test.txt"), "freezegun\n").expect("write");
        let roots = vec![root.clone()];
        assert!(stale(repo.path(), &roots));
        std::fs::write(env.join(STAMP), filled_from(&roots)).expect("stamp");
        assert!(!stale(repo.path(), &roots));
        std::fs::write(root.join("requirements-test.txt"), "freezegun\nrespx\n").expect("write");
        assert!(stale(repo.path(), &roots));
    }
}
