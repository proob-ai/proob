//! The only place proob starts a child process.
//!
//! `run` takes a program path and an argv array, never a command line, so no
//! argument is ever interpreted by a shell; property-derived arguments reach
//! a runner only as elements of that array (adapters place them after the
//! end-of-options separator). The child gets no stdin (`/dev/null`), its
//! stdout and stderr are captured in full, and it is killed when the timeout
//! passes. On Unix the child leads its own process group, and the kill takes
//! the whole group: a runner's own children (CBMC under `cargo kani`, a test
//! binary under `cargo test`) would otherwise outlive it, keep its pipes open
//! so the timeout never returns, and keep their memory. SIGINT, SIGTERM, or
//! SIGHUP sent to proob kills every live group the same way before proob dies.
//! Environment variables are inherited and `env` is added on top: a runner
//! needs `PATH` and `HOME`, and the caller names what else it sets.
//!
//! A verifier can take the machine down with it (CBMC under Kani took 6 to
//! 13 GB per harness), so while it waits `run` reads the resident memory of
//! the child's group from `ps` every `POLL`, and kills the group once it
//! passes the budget: `PROOB_MEMORY_LIMIT_MB` (`0` turns the watch off), else
//! half the physical memory. A poll `ps` cannot answer kills nothing.
//!
//! Nothing here interprets output; callers parse it and fail closed when it is
//! not what they expected.
use std::ffi::OsString;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;

/// The variable that sets the memory budget, in MB.
pub const MEMORY_LIMIT_VAR: &str = "PROOB_MEMORY_LIMIT_MB";
/// How often the group's memory is read while the child runs.
const POLL: Duration = Duration::from_millis(500);
const MB: u64 = 1024 * 1024;

/// What a finished child left behind. `code` is `None` when the child was
/// ended by a signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    /// Exit code zero.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Why no `Output` exists. Each names the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnError {
    /// The process never started: not found, not executable, bad `cwd`.
    Start { program: PathBuf, message: String },
    /// The process was still running after `after` and was killed.
    TimedOut { program: PathBuf, after: Duration },
    /// Waiting on the process or reading its pipes failed after it started.
    Io { program: PathBuf, message: String },
    /// The process group's resident memory passed `limit_bytes` (it read
    /// `peak_bytes`) and the group was killed.
    OutOfMemory { program: PathBuf, limit_bytes: u64, peak_bytes: u64 },
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpawnError::Start { program, message } => write!(f, "could not start {}: {message}", program.display()),
            SpawnError::TimedOut { program, after } => {
                write!(f, "{} did not finish within {} ms and was killed", program.display(), after.as_millis())
            }
            SpawnError::Io { program, message } => write!(f, "{} failed after starting: {message}", program.display()),
            SpawnError::OutOfMemory { program, limit_bytes, peak_bytes } => write!(
                f,
                "{} passed its memory budget of {} MB (it reached {} MB) and was stopped (out of memory); raise {MEMORY_LIMIT_VAR} to allow more",
                program.display(),
                limit_bytes / MB,
                peak_bytes / MB
            ),
        }
    }
}

impl std::error::Error for SpawnError {}

/// Runs `program` with `args` as its argv, in `cwd` when given, with `env`
/// added to the inherited environment, and waits at most `timeout`, within
/// the memory budget (module doc).
pub fn run(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    env: &[(OsString, OsString)],
    timeout: Duration,
) -> Result<Output, SpawnError> {
    run_within(program, args, cwd, env, timeout, memory_budget())
}

/// `run` with the budget in bytes given; `None` watches nothing.
fn run_within(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    env: &[(OsString, OsString)],
    timeout: Duration,
    limit: Option<u64>,
) -> Result<Output, SpawnError> {
    let mut child = command(program, args, cwd, env)
        .spawn()
        .map_err(|e| SpawnError::Start { program: program.to_path_buf(), message: e.to_string() })?;
    #[cfg(unix)]
    let _live = group::Live::track(child.id());
    let stdout = drain_stdout(child.stdout.take());
    let stderr = drain_stderr(child.stderr.take());
    let stopped = match watch(&mut child, timeout, limit) {
        Ok(Ended::Exited(status)) => {
            return Ok(Output {
                code: status.code(),
                stdout: joined(program, stdout)?,
                stderr: joined(program, stderr)?,
            });
        }
        Ok(Ended::TimedOut) => SpawnError::TimedOut { program: program.to_path_buf(), after: timeout },
        Ok(Ended::OutOfMemory { limit_bytes, peak_bytes }) => {
            SpawnError::OutOfMemory { program: program.to_path_buf(), limit_bytes, peak_bytes }
        }
        Err(e) => io_error(program, &e.to_string()),
    };
    reap(&mut child);
    // Draining after the kill lets the reader threads finish; their partial
    // output is discarded with the error.
    let _ = joined(program, stdout);
    let _ = joined(program, stderr);
    Err(stopped)
}

/// How a watched child stopped running.
enum Ended {
    Exited(ExitStatus),
    TimedOut,
    OutOfMemory { limit_bytes: u64, peak_bytes: u64 },
}

/// Waits in `POLL` slices until the child exits, `timeout` passes, or its
/// group's memory passes `limit`.
fn watch(child: &mut Child, timeout: Duration, limit: Option<u64>) -> std::io::Result<Ended> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.wait_timeout(POLL.min(deadline.saturating_duration_since(Instant::now())))? {
            return Ok(Ended::Exited(status));
        }
        if Instant::now() >= deadline {
            return Ok(Ended::TimedOut);
        }
        let over = limit.and_then(|l| group_rss(child.id()).filter(|rss| *rss > l).map(|rss| (l, rss)));
        if let Some((limit_bytes, peak_bytes)) = over {
            return Ok(Ended::OutOfMemory { limit_bytes, peak_bytes });
        }
    }
}

/// The resident bytes of every process in the group `pgid` leads, from
/// `ps`; `None` when `ps` gives no reading.
fn group_rss(pgid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-axo", "pgid=,rss="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(group_kib(&String::from_utf8_lossy(&out.stdout), pgid).saturating_mul(1024))
}

/// Pure: the summed `rss` (KiB) of the `pgid rss` rows whose pgid is `pgid`.
fn group_kib(ps: &str, pgid: u32) -> u64 {
    let row = |line: &str| {
        let mut cols = line.split_whitespace().map(str::parse::<u64>);
        Some((cols.next()?.ok()?, cols.next()?.ok()?))
    };
    ps.lines().filter_map(row).filter(|(g, _)| *g == u64::from(pgid)).map(|(_, kib)| kib).sum()
}

/// The budget in bytes: the variable when set, else half the physical memory.
fn memory_budget() -> Option<u64> {
    static PHYSICAL: OnceLock<Option<u64>> = OnceLock::new();
    budget(std::env::var(MEMORY_LIMIT_VAR).ok().as_deref(), *PHYSICAL.get_or_init(physical_memory))
}

/// Pure: `setting` MB (`0`: no budget); unset or not a whole number, half
/// of `physical` when known.
fn budget(setting: Option<&str>, physical: Option<u64>) -> Option<u64> {
    match setting.and_then(|s| s.trim().parse::<u64>().ok()) {
        Some(0) => None,
        Some(mb) => Some(mb.saturating_mul(MB)),
        None => physical.map(|bytes| bytes / 2),
    }
}

#[cfg(unix)]
fn physical_memory() -> Option<u64> {
    // SAFETY: sysconf(3) reads a system constant; no memory is touched.
    let (pages, size) = unsafe { (libc::sysconf(libc::_SC_PHYS_PAGES), libc::sysconf(libc::_SC_PAGESIZE)) };
    Some(u64::try_from(pages).ok()?.saturating_mul(u64::try_from(size).ok()?)).filter(|b| *b > 0)
}

// shortcut: no physical-memory probe off unix, so no default budget there; add one if Windows matters.
#[cfg(not(unix))]
fn physical_memory() -> Option<u64> {
    None
}

fn command(program: &Path, args: &[OsString], cwd: Option<&Path>, env: &[(OsString, OsString)]) -> Command {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.envs(env.iter().map(|(k, v)| (k.as_os_str(), v.as_os_str())));
    #[cfg(unix)]
    group::lead(&mut command);
    command
}

type Drain = JoinHandle<std::io::Result<Vec<u8>>>;

fn drain_stdout(pipe: Option<ChildStdout>) -> Drain {
    std::thread::spawn(move || read_all(pipe))
}

fn drain_stderr(pipe: Option<ChildStderr>) -> Drain {
    std::thread::spawn(move || read_all(pipe))
}

fn read_all<R: Read>(pipe: Option<R>) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        pipe.read_to_end(&mut bytes)?;
    }
    Ok(bytes)
}

fn joined(program: &Path, drain: Drain) -> Result<Vec<u8>, SpawnError> {
    match drain.join() {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(e)) => Err(io_error(program, &e.to_string())),
        Err(_) => Err(io_error(program, "the pipe reader thread panicked")),
    }
}

fn io_error(program: &Path, message: &str) -> SpawnError {
    SpawnError::Io { program: program.to_path_buf(), message: message.to_string() }
}

/// Kills and waits, so no zombie outlives the call. Errors here are moot: the
/// caller already has the error it will report.
fn reap(child: &mut Child) {
    #[cfg(unix)]
    group::kill(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
mod group {
    //! Each child leads a process group whose id is its pid. The live groups
    //! sit in a fixed table of atomics, the only state a signal handler may
    //! touch, so an interrupted proob can kill them before it dies.
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    use std::sync::Once;
    use std::sync::atomic::{AtomicI32, Ordering};

    /// A child beyond this many, or one spawned in the instant before it is
    /// tracked, is killed at its timeout while proob lives, but outlives an
    /// interrupted proob: it is outside the terminal's foreground group.
    /// proob runs one child at a time today.
    const SLOTS: usize = 64;
    static LIVE: [AtomicI32; SLOTS] = [const { AtomicI32::new(0) }; SLOTS];
    static HANDLERS: Once = Once::new();

    pub fn lead(command: &mut Command) {
        command.process_group(0);
        HANDLERS.call_once(install);
    }

    /// Kills every process in the group `pid` leads.
    pub fn kill(pid: u32) {
        if let Ok(pgid) = i32::try_from(pid) {
            // SAFETY: kill(2) with a negative pid signals that process group; no memory is touched.
            unsafe { libc::kill(-pgid, libc::SIGKILL) };
        }
    }

    /// A live group in the table, removed when dropped.
    pub struct Live(Option<&'static AtomicI32>);

    impl Live {
        pub fn track(pid: u32) -> Live {
            let pgid = i32::try_from(pid).unwrap_or(0);
            Live(
                LIVE.iter()
                    .find(|s| pgid > 0 && s.compare_exchange(0, pgid, Ordering::SeqCst, Ordering::SeqCst).is_ok()),
            )
        }
    }

    impl Drop for Live {
        fn drop(&mut self) {
            if let Some(slot) = self.0 {
                slot.store(0, Ordering::SeqCst);
            }
        }
    }

    fn install() {
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            // SAFETY: `on_signal` is async-signal-safe: atomic loads, kill(2), signal(2), raise(3).
            let previous =
                unsafe { libc::signal(signal, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t) };
            if previous == libc::SIG_IGN {
                // A signal the parent ignored (nohup) stays ignored.
                unsafe { libc::signal(signal, libc::SIG_IGN) };
            }
        }
    }

    extern "C" fn on_signal(signal: libc::c_int) {
        for slot in &LIVE {
            let pgid = slot.load(Ordering::SeqCst);
            if pgid > 0 {
                // SAFETY: as in `kill`.
                unsafe { libc::kill(-pgid, libc::SIGKILL) };
            }
        }
        // SAFETY: restore the default action and re-raise, so proob dies of the signal as before.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    const GB: u64 = 1024 * MB;

    #[test]
    fn the_budget_is_the_variable_else_half_the_machine() {
        assert_eq!(budget(None, Some(16 * GB)), Some(8 * GB));
        assert_eq!(budget(Some("100"), Some(16 * GB)), Some(100 * MB));
        assert_eq!(budget(Some("0"), Some(16 * GB)), None, "0 turns the watch off");
        assert_eq!(budget(Some("8G"), Some(16 * GB)), Some(8 * GB), "not a number: the default");
        assert_eq!(budget(Some("-1"), Some(16 * GB)), Some(8 * GB));
        assert_eq!(budget(None, None), None, "an unknown machine: no watch");
    }

    #[test]
    fn group_memory_sums_only_the_group() {
        let ps = "  41   1000\n 42 20\n42   300\n garbage\n\n 7 5\n";
        assert_eq!(group_kib(ps, 42), 320);
        assert_eq!(group_kib(ps, 99), 0);
    }

    #[test]
    fn a_child_over_the_budget_is_killed_as_out_of_memory() {
        // Every byte of the 300 MB is written, so it is resident.
        let script = "import time; b = b'x' * (300 << 20); time.sleep(30)";
        let args = [OsString::from("-c"), OsString::from(script)];
        let started = Instant::now();
        let err =
            run_within(Path::new("python3"), &args, None, &[], Duration::from_secs(60), Some(100 * MB)).unwrap_err();
        assert!(err.to_string().contains(MEMORY_LIMIT_VAR), "{err}");
        let SpawnError::OutOfMemory { limit_bytes, peak_bytes, .. } = err else { panic!("{err:?}") };
        assert_eq!(limit_bytes, 100 * MB);
        assert!(peak_bytes > 100 * MB, "{peak_bytes}");
        assert!(started.elapsed() < Duration::from_secs(20), "the group was killed, not waited for");
    }

    #[test]
    fn a_child_under_the_budget_runs_to_the_end() {
        // Long enough for the watch to read the group at least once.
        let out = run_within(Path::new("sleep"), &[OsString::from("1")], None, &[], Duration::from_secs(10), Some(GB))
            .expect("sleep runs");
        assert!(out.success());
    }
}
