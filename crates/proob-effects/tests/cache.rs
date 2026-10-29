//! The fact cache: a hand-written verdict in the cache directory changes
//! nothing, immutable facts are fetched once, permission facts expire, an
//! open PR's reviews are never cached, and the cache knows its own age.
mod common;

use common::*;
use proob_effects::cache::{Cache, CachedApi};
use proob_effects::facts::GovernedSet;
use proob_effects::fixture::{FixtureApi, FixtureRepo};
use proob_effects::gather::approval_of;
use proob_effects::github::{CallBudget, GitHubApi};
use std::fs;

fn plant(dir: &std::path::Path, relative: &str, text: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

// proob:01M622RQ7BGJ75BQ4B0JARP2KC
#[test]
fn a_hand_written_verdict_in_the_cache_changes_nothing() {
    let scenario = review_scenario("earlier-commit-blob-changed");
    let governed = GovernedSet::default();
    let repo = FixtureRepo::new(&scenario);

    // Without a cache.
    let budget = CallBudget::new(1_000);
    let api = FixtureApi::new(&scenario, &budget);
    let plain = approval_of(&ctx(&api, &repo, &governed, false), &[PROPERTY_PATH]).expect("plain");

    // With a cache full of planted files that claim approval in every shape
    // someone might try: a verdict file, a payload-named file holding an
    // approval, and an entry wrapper around an approval.
    let dir = tempfile::tempdir().expect("tempdir");
    let planted = [
        ("approval.json", r#"{"approval":"Team"}"#),
        ("verdict/01J9Z3W1V8Q4K7N2M5R8T1X4C6.json", r#"{"verdict":"Holds"}"#),
        ("pull/7.json", r#"{"fetched_at":1,"payload":{"approval":{"sig":"Team","pr":7}}}"#),
        ("reviews/7.json", r#"{"fetched_at":1,"payload":{"status":"Approved"}}"#),
        ("pulls-for-commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.json", "not even json"),
    ];
    for (relative, text) in planted {
        plant(dir.path(), relative, text);
    }
    let budget = CallBudget::new(1_000);
    let inner = FixtureApi::new(&scenario, &budget);
    let cached = CachedApi::new(&inner, Cache::at(dir.path()), NOW, 14 * DAY);
    let with_cache = approval_of(&ctx(&cached, &repo, &governed, false), &[PROPERTY_PATH]).expect("cached");

    assert_eq!(with_cache, plain, "the derived approval is identical with and without the planted cache");
    assert_eq!(plain.approval, no_eligible(proob_core::types::Ineligible::ApprovalPredatesChange));

    // The planted reviews entry and the unreadable commit entry were on the
    // run's path: both were misses and were overwritten with fetched payloads.
    // Every file the run wrote is an `Entry` whose payload carries no approval,
    // status, or verdict key; the planted files it never consulted are
    // untouched and unread.
    let untouched =
        |relative: &str, text: &str| fs::read_to_string(dir.path().join(relative)).ok().as_deref() == Some(text);
    assert!(!untouched("reviews/7.json", planted[3].1), "the planted reviews were replaced");
    assert!(!untouched("pulls-for-commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.json", planted[4].1));
    assert!(
        untouched("approval.json", planted[0].1) && untouched("verdict/01J9Z3W1V8Q4K7N2M5R8T1X4C6.json", planted[1].1)
    );
    for entry in walkdir(dir.path()) {
        let relative = entry.strip_prefix(dir.path()).expect("inside").to_string_lossy().into_owned();
        if planted.iter().any(|(p, text)| *p == relative && untouched(p, text)) {
            continue;
        }
        if entry.extension().is_some_and(|e| e == "json") {
            let text = fs::read_to_string(&entry).unwrap_or_default();
            let value: serde_json::Value =
                serde_json::from_str(&text).unwrap_or_else(|_| panic!("{} is not a cache entry", entry.display()));
            assert!(value.get("fetched_at").is_some(), "{} has no fetched_at", entry.display());
            let payload = value.get("payload").expect("payload");
            for forbidden in ["approval", "verdict", "status", "sig"] {
                assert!(payload.get(forbidden).is_none(), "{} carries {forbidden}", entry.display());
            }
        }
    }
}

fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walkdir(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn immutable_facts_are_fetched_once_and_permissions_expire() {
    let scenario = review_scenario("non-author-approval-with-write");
    let dir = tempfile::tempdir().expect("tempdir");
    let budget = CallBudget::new(1_000);
    let inner = FixtureApi::new(&scenario, &budget);
    let sha = "1".repeat(40);

    let cached = CachedApi::new(&inner, Cache::at(dir.path()), NOW, 14 * DAY);
    cached.commit(&sha).expect("commit");
    cached.commit(&sha).expect("commit again");
    assert_eq!(budget.spent(), 1, "a commit is immutable: one fetch");

    cached.permission("bob").expect("permission");
    cached.permission("bob").expect("permission again");
    assert_eq!(budget.spent(), 2, "within the time to live: one fetch");

    // Fifteen days later the permission fact is stale and the commit is not.
    let later = CachedApi::new(&inner, Cache::at(dir.path()), NOW + 15 * DAY, 14 * DAY);
    later.commit(&sha).expect("commit");
    later.permission("bob").expect("permission");
    assert_eq!(budget.spent(), 3, "only the permission was fetched again");

    assert_eq!(later.cache().last_write(), Some(NOW + 15 * DAY));
    assert_eq!(later.cache().age(NOW + 16 * DAY), Some(DAY));
}

#[test]
fn reviews_are_cached_only_for_merged_prs() {
    let scenario = review_scenario("open-pr-ready");
    let dir = tempfile::tempdir().expect("tempdir");
    let budget = CallBudget::new(1_000);
    let inner = FixtureApi::new(&scenario, &budget);
    let cached = CachedApi::new(&inner, Cache::at(dir.path()), NOW, 14 * DAY);

    cached.pull(9).expect("open PR");
    cached.reviews(9).expect("reviews");
    cached.reviews(9).expect("reviews again");
    assert_eq!(budget.spent(), 3, "an open PR's reviews are read live every time");
    assert!(!dir.path().join("pull").join("9.json").exists(), "an open PR is not cached");

    let merged = review_scenario("non-author-approval-with-write");
    let budget = CallBudget::new(1_000);
    let inner = FixtureApi::new(&merged, &budget);
    let cached = CachedApi::new(&inner, Cache::at(dir.path()), NOW, 14 * DAY);
    cached.pull(7).expect("merged PR");
    cached.reviews(7).expect("reviews");
    cached.reviews(7).expect("reviews again");
    assert_eq!(budget.spent(), 2, "a merged PR's reviews are fetched once");
}

#[test]
fn a_disabled_cache_misses_and_writes_nothing() {
    let cache = Cache::disabled();
    assert!(!cache.is_enabled());
    assert!(cache.read::<String>("blob", "x").is_none());
    cache.write("blob", "x", NOW, &"y".to_string()).expect("no-op write");
    assert_eq!(cache.last_write(), None);
}

#[test]
fn unsafe_keys_are_never_written() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = Cache::at(dir.path());
    cache.write("pull", "../escape", NOW, &1u32).expect("refused silently");
    cache.write("pull", "a/b", NOW, &1u32).expect("refused silently");
    assert!(walkdir(dir.path()).is_empty(), "{:?}", walkdir(dir.path()));
}
