//! `cargo xtask <cmd>`: one entry point for every local and CI exec command.
//!
//! One concept per module:
//!   cli    the argv grammar (pure)
//!   commit Conventional Commit subjects (pure)
//!   plan   what each task runs, as data (pure)
//!   tools  where cargo-verus and claude are (reads PATH and the filesystem once)
//!   run    the one effectful runner
//!   error  every way this can fail, and how it is reported

mod cli;
mod commit;
mod error;
mod plan;
mod run;
mod tools;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cli::Command;
use error::Error;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            error.exit_code()
        }
    }
}

fn dispatch(args: &[String]) -> Result<(), Error> {
    match cli::parse(args)? {
        Command::Help => {
            print!("{}", cli::help());
            Ok(())
        }
        Command::Run(task) => run::task(&context()?, &task),
        Command::Ci { dry_run: true } => run::dry_run(&context()?),
        Command::Ci { dry_run: false } => run::ci(&context()?),
    }
}

fn context() -> Result<run::Context, Error> {
    Ok(run::Context { root: workspace_root(), tools: tools::detect()? })
}

/// The workspace root: the parent of this crate's directory, fixed at build time,
/// so every step runs from the same place whatever the caller's directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}
