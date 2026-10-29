//! The only place proob starts a child process.
//!
//! `run` takes a program path and an argv array, never a command line, so no
//! argument is ever interpreted by a shell; property-derived arguments reach
//! a runner only as elements of that array (adapters place them after the
//! end-of-options separator). The child gets no stdin (`/dev/null`), its
//! stdout and stderr are captured in full, and it is killed when the timeout
//! passes. Environment variables are inherited and `env` is added on top: a
//! runner needs `PATH` and `HOME`, and the caller names what else it sets.
//!
//! Nothing here interprets output; callers parse it and fail closed when it is
//! not what they expected.
use std::ffi::OsString;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::thread::JoinHandle;
use std::time::Duration;
use wait_timeout::ChildExt;

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
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpawnError::Start { program, message } => write!(f, "could not start {}: {message}", program.display()),
            SpawnError::TimedOut { program, after } => {
                write!(f, "{} did not finish within {} ms and was killed", program.display(), after.as_millis())
            }
            SpawnError::Io { program, message } => write!(f, "{} failed after starting: {message}", program.display()),
        }
    }
}

impl std::error::Error for SpawnError {}

/// Runs `program` with `args` as its argv, in `cwd` when given, with `env`
/// added to the inherited environment, and waits at most `timeout`.
pub fn run(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    env: &[(OsString, OsString)],
    timeout: Duration,
) -> Result<Output, SpawnError> {
    let mut child = command(program, args, cwd, env)
        .spawn()
        .map_err(|e| SpawnError::Start { program: program.to_path_buf(), message: e.to_string() })?;
    let stdout = drain_stdout(child.stdout.take());
    let stderr = drain_stderr(child.stderr.take());
    let status = child.wait_timeout(timeout).map_err(|e| io_error(program, &e.to_string()));
    match status {
        Ok(Some(status)) => {
            Ok(Output { code: status.code(), stdout: joined(program, stdout)?, stderr: joined(program, stderr)? })
        }
        Ok(None) => {
            reap(&mut child);
            // Draining after the kill lets the reader threads finish; their
            // partial output is discarded with the timeout.
            let _ = joined(program, stdout);
            let _ = joined(program, stderr);
            Err(SpawnError::TimedOut { program: program.to_path_buf(), after: timeout })
        }
        Err(e) => {
            reap(&mut child);
            Err(e)
        }
    }
}

fn command(program: &Path, args: &[OsString], cwd: Option<&Path>, env: &[(OsString, OsString)]) -> Command {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.envs(env.iter().map(|(k, v)| (k.as_os_str(), v.as_os_str())));
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
    let _ = child.kill();
    let _ = child.wait();
}
