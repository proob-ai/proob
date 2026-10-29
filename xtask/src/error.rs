//! Every way xtask can fail, and the one line each prints.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::cli::ParseError;
use crate::plan::PlanError;

#[derive(Debug)]
pub enum Error {
    Usage(ParseError),
    Plan(PlanError),
    /// tools/get-verus.sh no longer has the line xtask reads the pin from.
    VerusPin,
    Spawn {
        program: String,
        source: io::Error,
    },
    Failed {
        command: String,
        code: Option<i32>,
    },
    Expectation {
        command: String,
        expect: String,
    },
    TokenPresent(&'static str),
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Path(String),
    CiFailed(usize),
    /// This many subjects break the commit conventions.
    Lint(usize),
}

impl Error {
    /// Usage errors exit 2, like getopt; everything else exits 1.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Error::Usage(_) => ExitCode::from(2),
            _ => ExitCode::FAILURE,
        }
    }
}

impl From<ParseError> for Error {
    fn from(error: ParseError) -> Self {
        Error::Usage(error)
    }
}

impl From<PlanError> for Error {
    fn from(error: PlanError) -> Self {
        Error::Plan(error)
    }
}

fn spawn_line(program: &str, source: &io::Error) -> String {
    match source.kind() {
        io::ErrorKind::NotFound => {
            format!("{program} is not installed or not on PATH (prerequisites: xtask/README.md)")
        }
        _ => format!("could not start {program}: {source}"),
    }
}

fn exit_word(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("exit {code}"),
        None => "killed by a signal".to_owned(),
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(error) => write!(f, "{error}"),
            Error::Plan(error) => write!(f, "{error}"),
            Error::VerusPin => {
                write!(f, "tools/get-verus.sh no longer pins VERUS_VERSION; xtask reads the pin from that line")
            }
            Error::Spawn { program, source } => write!(f, "{}", spawn_line(program, source)),
            Error::Failed { command, code } => write!(f, "{}: {command}", exit_word(*code)),
            Error::Expectation { command, expect } => write!(f, "{command}: expected {expect}"),
            Error::TokenPresent(name) => write!(
                f,
                "{name} is set; the declaration check needs no token in the environment (env -u {name} cargo xtask declaration)"
            ),
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Path(why) => write!(f, "cannot build the child's PATH: {why}"),
            Error::CiFailed(n) => write!(f, "ci: {n} command(s) failed"),
            Error::Lint(n) => write!(f, "commit-lint: {n} subject(s) break the conventions in CONTRIBUTING.md"),
        }
    }
}
