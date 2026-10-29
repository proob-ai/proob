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

#[test]
fn missing_program_is_a_start_error() {
    let err = run(Path::new("/nonexistent/proob-no-such-program"), &[], None, &[], SECOND).unwrap_err();
    assert!(matches!(err, SpawnError::Start { .. }), "{err:?}");
    assert!(err.to_string().contains("proob-no-such-program"));
}
