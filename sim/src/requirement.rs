//! A new requirement lands in a team that has adopted proob, over the Rust
//! fixture: a feature branch adds a module no property covers, and `check`
//! says so without blocking; a property for it on the same branch empties the
//! list. A second branch changes what an approved property covers, and the
//! gate's real jobs render it `Broken`; a superseding property, with the old
//! file deleted in the same pull request, leaves nothing `Broken` and the new
//! property `Proposed` until a reviewer approves it.

use crate::github::ALICE;
use crate::lifecycle::{A, INTRODUCED, PROPERTY, declare, merge_declaration, pre_proob};
use crate::verdicts::Truth::{Unknown, Violated};
use crate::verdicts::{self, Expect, Log, Observed, observe, run_steps};
use crate::world::{DAY, Event, Fallible, NOW, Run, World, run};

/// The new requirement's property: refunds never exceed the charge.
const REFUNDS: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XRF";
/// The property that supersedes A once sorting drops duplicates.
const A2: &str = "01K8V5NQ4A0F3R7Y2M9D6P1XS2";
const MODULE: &str = "src/refunds.rs";

pub type Step = verdicts::Step<World>;

/// Truth: `dedup` makes A's statement false on the PR's code (a real
/// counterexample); the other steps' properties are unapproved, so `Unknown`.
pub const STEPS: [(Expect, Step); 4] = [
    (
        Expect::status("a feature branch adds a module (check, exit 0)", "uncovered: src/refunds.rs", Unknown),
        feature_branch,
    ),
    (Expect::status("a property for it on the same branch", "uncovered: none", Unknown), property_for_it),
    (
        Expect::blocks("a second branch changes what A covers (gate judge)", "Broken", Violated)
            .because("counterexample on this PR"),
        changed_requirement,
    ),
    (Expect::status("a superseding property, the old file deleted (gate judge)", "Proposed", Unknown), superseded),
];

pub fn run_scenario(ctx: &crate::world::Ctx, log: &Log) -> Fallible<Vec<String>> {
    let mut world = World::new(ctx)?;
    pre_proob(&world)?;
    declare(&world)?;
    merge_declaration(&mut world, &INTRODUCED)?;
    run_steps("requirement", log, &mut world, &STEPS)
}

/// `proob check --base main --head HEAD --json` in the developer's checkout.
fn check(w: &World) -> Fallible<Run> {
    run(w.proob(&w.repo.dir)?.args(["check", "--base", "main", "--head", "HEAD", "--json"]))
}

/// The uncovered list as the step's word; the rest of the observation is the report's.
fn observe_uncovered(out: &Run) -> Fallible<Observed> {
    let report = out.json()?;
    let paths: Vec<&str> = report["uncovered"]
        .as_array()
        .ok_or("the report has no uncovered list")?
        .iter()
        .filter_map(|p| p.as_str())
        .collect();
    let word = match paths.is_empty() {
        true => "uncovered: none".to_owned(),
        false => format!("uncovered: {}", paths.join(", ")),
    };
    let base = observe(out, A)?;
    Ok(Observed { word, reason: report["headline"].as_str().unwrap_or("").to_owned(), ..base })
}

fn property_toml(id: &str, title: &str, statement: &str, scope: &str, rationale: &str) -> String {
    format!(
        "id = \"{id}\"\ntitle = \"{title}\"\nstatement = \"{statement}\"\nscope = [\"{scope}\"]\nseverity = \"high\"\nrationale = \"{rationale}\"\n"
    )
}

// ---------------------------------------------------------------------------
// The steps
// ---------------------------------------------------------------------------

fn feature_branch(w: &mut World) -> Fallible<Observed> {
    w.repo.checkout(&["-b", "refunds"])?;
    w.repo.write(MODULE, "/// The refund for a charge, never more than the charge.\npub fn refund(charge: u64, asked: u64) -> u64 {\n    asked.min(charge)\n}\n")?;
    w.repo.commit("refunds: a new module", &ALICE, NOW - 3 * DAY)?;
    observe_uncovered(&check(w)?)
}

fn property_for_it(w: &mut World) -> Fallible<Observed> {
    let path = format!("proob/properties/{REFUNDS}.toml");
    let text = property_toml(
        REFUNDS,
        "Refunds never exceed the charge",
        "A refund is never larger than the charge it refunds.",
        MODULE,
        "The refunds module caps the amount at the charge.",
    );
    w.repo.write(&path, &text)?;
    w.repo.commit("proob: propose a property for refunds", &ALICE, NOW - 3 * DAY + 600)?;
    let out = check(w)?;
    let uncovered = observe_uncovered(&out)?;
    match observe(&out, REFUNDS)?.word.as_str() {
        "Proposed" => Ok(uncovered),
        other => Ok(Observed { word: format!("the new property renders {other}"), ..uncovered }),
    }
}

/// Sorting now drops duplicates: the team changed what correct means, and
/// A's bound test fails on this PR.
fn changed_requirement(w: &mut World) -> Fallible<Observed> {
    w.repo.checkout(&["main"])?;
    let base = w.repo.rev("main")?;
    w.repo.checkout(&["-b", "dedup"])?;
    let code = w.repo.read("src/ordering.rs")?;
    w.repo.write("src/ordering.rs", &code.replace("out.sort_unstable();", "out.sort_unstable();\n    out.dedup();"))?;
    let head = w.repo.commit("sorted: keep one copy of each value", &ALICE, NOW - 2 * DAY)?;
    w.github.open_pr(2, &ALICE, &head, &base, &["src/ordering.rs"]);
    let (judge, _) = w.workflow_run(&Event::pull_request(2, &base, &head, NOW - 2 * DAY + 600), 301)?;
    observe(&judge, A)
}

/// The same PR: a new property says what sorting now promises, its rationale
/// names the one it supersedes, the bound test moves to it, and A's file is
/// deleted, as the reviewer is asked to approve.
fn superseded(w: &mut World) -> Fallible<Observed> {
    let parent = w.repo.rev("HEAD")?;
    let a2_path = format!("proob/properties/{A2}.toml");
    let rationale =
        format!("Supersedes {A}: sorting now keeps one copy of each value, so the length may shrink; it never grows.");
    w.repo.write(
        &a2_path,
        &property_toml(
            A2,
            "Sorting never adds an element",
            "Sorting a list never adds an element to it.",
            "src/ordering.rs",
            &rationale,
        ),
    )?;
    w.repo.git(&["rm", "-q", PROPERTY])?;
    let tests = w.repo.read("tests/props.rs")?;
    let moved = tests
        .replace(&format!("// proob:{A}"), &format!("// proob:{A2}"))
        .replace("fn sorted_preserves_length", "fn sorted_never_grows")
        .replace("prop_assert_eq!(sorted(&v).len(), v.len());", "prop_assert!(sorted(&v).len() <= v.len());");
    w.repo.write("tests/props.rs", &moved)?;
    let head = w.repo.commit("proob: supersede the length property", &ALICE, NOW - DAY)?;
    w.github.open_pr(2, &ALICE, &head, &parent, &[a2_path.as_str(), PROPERTY, "tests/props.rs"]);
    let (judge, _) = w.workflow_run(&Event::pull_request(2, &w.repo.rev("main")?, &head, NOW - DAY + 600), 302)?;
    let a2 = observe(&judge, A2)?;
    match observe(&judge, A) {
        Err(_) => Ok(a2),
        Ok(old) => Ok(Observed { word: format!("the superseded property is still listed as {}", old.word), ..a2 }),
    }
}
