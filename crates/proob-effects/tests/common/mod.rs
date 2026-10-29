//! Shared helpers: loading scenarios from `tests/fixtures/`, building a
//! gather `Context` over the fixture fakes, and the shas the fixtures use.
#![allow(dead_code)]
use proob_core::types::{Approval, Ineligible, Oid, Secs, Sig, UnreviewedReason};
use proob_effects::facts::GovernedSet;
use proob_effects::fixture::Scenario;
use proob_effects::gather::Context;
use proob_effects::git::Repo;
use proob_effects::github::GitHubApi;
use std::path::PathBuf;

pub const PROPERTY_ULID: &str = "01J9Z3W1V8Q4K7N2M5R8T1X4C6";
pub const PROPERTY_PATH: &str = "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4C6.toml";

/// The property's id as the core sees it.
pub fn property_id() -> u128 {
    proob_facts::ulid::decode(PROPERTY_ULID).expect("fixture ULID")
}
/// 2026-10-28T00:00:00Z.
pub const NOW: Secs = 1_793_145_600;
pub const DAY: Secs = 86_400;

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

pub fn review_scenario(name: &str) -> Scenario {
    load(&fixtures().join("reviews").join(format!("{name}.json")))
}

pub fn run_scenario(name: &str) -> Scenario {
    load(&fixtures().join("runs").join(format!("{name}.json")))
}

fn load(path: &std::path::Path) -> Scenario {
    Scenario::load(path).unwrap_or_else(|e| panic!("{e}"))
}

/// Every scenario file in a fixture directory, by name.
pub fn scenario_names(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(fixtures().join(dir))
        .expect("fixture dir")
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(str::to_string))
        .collect();
    names.sort();
    names
}

/// The 40-hex sha made of one repeated character, as the core stores it.
pub fn sha(c: char) -> Oid {
    let byte = u8::from_str_radix(&format!("{c}{c}"), 16).expect("hex char");
    let mut oid = [0u8; 32];
    oid[..20].fill(byte);
    oid
}

/// A 40-hex sha made of a repeated two-character pair.
pub fn sha2(pair: &str) -> Oid {
    let byte = u8::from_str_radix(pair, 16).expect("hex pair");
    let mut oid = [0u8; 32];
    oid[..20].fill(byte);
    oid
}

pub fn ctx<'a>(
    api: &'a dyn GitHubApi,
    repo: &'a dyn Repo,
    governed: &'a GovernedSet,
    allow_self_signed: bool,
) -> Context<'a> {
    Context {
        api,
        repo,
        default_branch: "main",
        default_ref: "origin/main",
        governed,
        allow_self_signed,
        now: NOW,
        history_depth: 8,
    }
}

pub fn team(pr: u64, merge: char) -> Approval {
    Approval::Approved { sig: Sig::Team, pr, merge_sha: sha(merge) }
}

pub fn self_signed(pr: u64, merge: char) -> Approval {
    Approval::Approved { sig: Sig::SelfSigned, pr, merge_sha: sha(merge) }
}

pub fn no_eligible(why: Ineligible) -> Approval {
    Approval::Unreviewed(UnreviewedReason::NoEligibleReview(why))
}

pub fn direct_push() -> Approval {
    Approval::Unreviewed(UnreviewedReason::DirectPush)
}

pub fn revoked() -> Approval {
    Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked)
}
