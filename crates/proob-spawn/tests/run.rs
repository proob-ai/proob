//! `run` against real coreutils: argv is passed verbatim, output is captured,
//! the exit code is reported, stdin is not inherited, the environment and the
//! working directory are applied, a slow child is killed at the timeout, and
//! a missing program is a `Start` error rather than a panic.
#![cfg(unix)]
use proob_spawn::{SpawnError, run};
use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

const SECOND: Duration = Duration::from_secs(1);

#[test]
fn argv_is_passed_verbatim_and_stdout_captured() {
    // A leading dash in a data argument is still data: nothing interprets it.
    let out = run(Path::new("echo"), &args(&["--", "-not-an-option", "two words"]), None, &[], 5 * SECOND)
        .expect("echo runs");
    assert!(out.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-- -not-an-option two words\n");
    assert!(out.stderr.is_empty());
}

#[test]
fn exit_code_is_reported() {
    let out = run(Path::new("false"), &[], None, &[], 5 * SECOND).expect("false runs");
    assert!(!out.success());
    assert_eq!(out.code, Some(1));
}

#[test]
fn stdin_is_not_inherited() {
    // With an inherited stdin `cat` would wait for input; with /dev/null it exits at once.
    let out = run(Path::new("cat"), &[], None, &[], 5 * SECOND).expect("cat runs");
    assert!(out.success());
    assert!(out.stdout.is_empty());
}

#[test]
fn environment_is_added_to_the_inherited_one() {
    let env = [(OsString::from("PROOB_SPAWN_TEST"), OsString::from("present"))];
    let out = run(Path::new("env"), &[], None, &env, 5 * SECOND).expect("env runs");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.lines().any(|l| l == "PROOB_SPAWN_TEST=present"), "{text}");
    assert!(text.lines().any(|l| l.starts_with("PATH=")), "PATH is inherited: {text}");
}

#[test]
fn working_directory_is_applied() {
    let dir = tempfile::tempdir().expect("tempdir");
    let want = dir.path().canonicalize().expect("canonical");
    let out = run(Path::new("pwd"), &[], Some(dir.path()), &[], 5 * SECOND).expect("pwd runs");
    let got = String::from_utf8_lossy(&out.stdout);
    assert_eq!(Path::new(got.trim_end()).canonicalize().expect("canonical"), want);
}

#[test]
fn slow_child_is_killed_at_the_timeout() {
    let started = Instant::now();
    let err = run(Path::new("sleep"), &args(&["30"]), None, &[], Duration::from_millis(300)).unwrap_err();
    assert!(matches!(err, SpawnError::TimedOut { .. }), "{err:?}");
    assert!(started.elapsed() < 10 * SECOND, "the child was killed, not waited for");
}

/// A shell that starts a 30 s grandchild holding its stdout, writes the
/// grandchild's pid to `pid_file`, and waits for it: `cargo kani` and CBMC.
fn tree(pid_file: &Path) -> Vec<OsString> {
    args(&["-c", &format!("sleep 30 & echo $! > '{}'; wait", pid_file.display())])
}

/// Whether `done` becomes true within 10 s, checked every 100 ms.
fn eventually(done: impl Fn() -> bool) -> bool {
    (0..100).any(|_| {
        let now = done();
        if !now {
            std::thread::sleep(Duration::from_millis(100));
        }
        now
    })
}

/// The pid in `pid_file` soon names no process (a killed one is reaped by init).
fn gone(pid_file: &Path) -> bool {
    let pid = std::fs::read_to_string(pid_file).expect("the shell wrote the grandchild's pid");
    let alive = || {
        std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    eventually(|| !alive())
}

#[test]
fn a_timeout_kills_the_grandchildren_too() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pid_file = dir.path().join("pid");
    let started = Instant::now();
    let err = run(Path::new("sh"), &tree(&pid_file), None, &[], Duration::from_millis(500)).unwrap_err();
    assert!(matches!(err, SpawnError::TimedOut { .. }), "{err:?}");
    assert!(started.elapsed() < 10 * SECOND, "a grandchild holding the pipes must not outlast the timeout");
    assert!(gone(&pid_file), "the grandchild outlived the timeout");
}

#[test]
fn an_interrupted_proob_takes_its_children_with_it() {
    const CHILD: &str = "PROOB_SPAWN_TEST_INTERRUPTED";
    if let Some(pid_file) = std::env::var_os(CHILD) {
        // This process is the "proob" being interrupted.
        let _ = run(Path::new("sh"), &tree(Path::new(&pid_file)), None, &[], 60 * SECOND);
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let pid_file = dir.path().join("pid");
    let mut proob = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", "an_interrupted_proob_takes_its_children_with_it", "--nocapture"])
        .env(CHILD, &pid_file)
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("re-run this test as the child");
    let started = eventually(|| std::fs::read_to_string(&pid_file).is_ok_and(|s| s.ends_with('\n')));
    assert!(started, "the child never started its tree");
    let sent = std::process::Command::new("kill").args(["-INT", &proob.id().to_string()]).status();
    assert!(sent.is_ok_and(|s| s.success()));
    let status = proob.wait().expect("the child exits");
    assert!(!status.success(), "the interrupt ends proob");
    assert!(gone(&pid_file), "the grandchild outlived the interrupted proob");
}

#[test]
fn missing_program_is_a_start_error() {
    let err = run(Path::new("/nonexistent/proob-no-such-program"), &[], None, &[], SECOND).unwrap_err();
    assert!(matches!(err, SpawnError::Start { .. }), "{err:?}");
    assert!(err.to_string().contains("proob-no-such-program"));
}
