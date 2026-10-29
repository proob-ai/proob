//! proob's effects: everything that touches the world so the verified core
//! never has to.
//!
//! `git` reads history and the working tree (behind the `Repo` trait);
//! `github` is the read-only client behind the `GitHubApi` trait, with the
//! per-run call budget and a token that is never printed; `cache` keeps
//! fetched payloads and nothing else; `facts` assembles an `Introduction`
//! from GitHub's records; `evidence` walks accepted evidence runs through
//! the verified `accept_run`; `gather` turns all of it into a `DeriveInput`
//! and a `Governance` for the verified `derive` and `verdict`; `fixture`
//! serves the same traits from JSON scenarios for tests.
//!
//! Nothing here stores or reads an approval or a verdict: the cache's entry
//! types are the facts crate's payloads, `gather` calls `eligibility` on
//! every run, and the only child process is `git` through proob-spawn.
pub mod cache;
pub mod error;
pub mod evidence;
pub mod facts;
pub mod fixture;
pub mod gather;
pub mod git;
pub mod github;
