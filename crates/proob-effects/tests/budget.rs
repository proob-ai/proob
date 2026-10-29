//! The call budget: with a budget smaller than the governed-file count, the
//! affected files are derived first and the rest answer `RateLimited`; a warm
//! cache stretches the same budget over more files.
mod common;

use common::*;
use proob_effects::cache::{Cache, CachedApi};
use proob_effects::facts::GovernedSet;
use proob_effects::fixture::{FixtureApi, FixtureRepo, Scenario};
use proob_effects::gather::statement_approvals;
use proob_effects::github::CallBudget;

const P1: &str = PROPERTY_PATH;
const P2: &str = "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4D1.toml";
const P3: &str = "proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4D2.toml";

/// Three property files introduced by the same approved PR.
fn three_files() -> Scenario {
    let mut s = review_scenario("non-author-approval-with-write");
    let merge = "a".repeat(40);
    let c1 = "1".repeat(40);
    for p in [P2, P3] {
        s.history.insert(p.to_string(), vec![merge.clone()]);
        s.blobs.insert(format!("{merge}:{p}"), Some("5".repeat(40)));
        s.blobs.insert(format!("{c1}:{p}"), Some("5".repeat(40)));
    }
    s
}

/// Calls one file costs on a cold cache: PRs of the commit, reviews, the
/// approver's permission, the blob at the reviewed commit, the PR's commits,
/// and the one commit's files.
const CALLS_PER_FILE: u32 = 6;

#[test]
fn affected_files_are_derived_first_and_the_rest_are_rate_limited() {
    let scenario = three_files();
    let budget = CallBudget::new(CALLS_PER_FILE);
    let api = FixtureApi::new(&scenario, &budget);
    let repo = FixtureRepo::new(&scenario);
    let governed = GovernedSet::default();
    let ctx = ctx(&api, &repo, &governed, false);

    let paths = [P1.to_string(), P2.to_string(), P3.to_string()];
    let results = statement_approvals(&ctx, &paths, &[P3.to_string()]);
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].0, P3, "the affected file comes first");
    assert_eq!(results[0].1.as_ref().map(|a| a.approval), Ok(team(7, 'a')));
    for (path, result) in &results[1..] {
        let err = result.as_ref().unwrap_err();
        assert!(err.is_rate_limited(), "{path}: {err}");
    }
    assert_eq!(budget.remaining(), 0);
}

#[test]
fn a_warm_cache_stretches_the_budget() {
    let scenario = three_files();
    let dir = tempfile::tempdir().expect("tempdir");
    // Cold: one file, plus one blob lookup per further file (the rest is shared).
    let budget = CallBudget::new(CALLS_PER_FILE + 2);
    let inner = FixtureApi::new(&scenario, &budget);
    let api = CachedApi::new(&inner, Cache::at(dir.path()), NOW, 14 * DAY);
    let repo = FixtureRepo::new(&scenario);
    let governed = GovernedSet::default();
    let ctx = ctx(&api, &repo, &governed, false);

    let paths = [P1.to_string(), P2.to_string(), P3.to_string()];
    let results = statement_approvals(&ctx, &paths, &[]);
    for (path, result) in &results {
        assert_eq!(result.as_ref().map(|a| a.approval), Ok(team(7, 'a')), "{path}");
    }
    assert_eq!(budget.remaining(), 0, "exactly the calls predicted");

    // Warm: no call at all.
    let budget = CallBudget::new(0);
    let inner = FixtureApi::new(&scenario, &budget);
    let api = CachedApi::new(&inner, Cache::at(dir.path()), NOW, 14 * DAY);
    let ctx = common::ctx(&api, &repo, &governed, false);
    let results = statement_approvals(&ctx, &paths, &[]);
    for (path, result) in &results {
        assert_eq!(result.as_ref().map(|a| a.approval), Ok(team(7, 'a')), "{path}");
    }
}
