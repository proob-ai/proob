//! The `proob` binary: the CLI, hook, and Action edges around the verified core.
//!
//! The edges are thin: they read files and git, talk to GitHub through the
//! effects crate, hand facts to the verified `derive`, `verdict`, `policy`,
//! and `preview`, and render what comes back. No judgement lives here.
mod cli;
mod hook;

fn main() -> std::process::ExitCode {
    cli::main()
}
