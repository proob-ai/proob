//! Markdown goldens: the headline, the rows with the reason and this-PR
//! columns, the if-merged-now rows, the footer, and a one-line legend. The
//! goldens live in tests/fixtures/reports/; `PROOB_UPDATE_GOLDEN=1` rewrites them.
mod common;

use common::scenarios::*;
use common::*;
use serde_json::json;

fn legend_is_one_line(markdown: &str) {
    let legends: Vec<&str> = markdown.lines().filter(|l| l.starts_with("Legend:")).collect();
    assert_eq!(legends.len(), 1, "exactly one legend line");
    for word in ["Holds", "Checked", "Not checked", "Re-approve", "Broken", "Proposed", "Waived until d"] {
        assert!(legends[0].contains(word), "legend lacks {word}");
    }
}

#[test]
fn status_markdown_golden() {
    let c = composite();
    let out = run(c.proob().arg("status"));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let md = &out.stdout;
    assert!(
        md.starts_with("## proob: 1 holds, 2 checked, 1 not checked, 1 re-approve, 1 broken, 1 proposed, 1 waived\n"),
        "{md}"
    );
    assert!(md.contains("| Property | Status | Reason | This PR |\n"));
    assert!(
        md.contains(
            "| A holds `01J9Z3W1V8Q4K7N2M5R8T1X4A1` | Checked (tested) | _tested on sampled inputs, not proved_ | not run |"
        ),
        "{md}"
    );
    assert!(
        md.contains(
            "| J is proved `01J9Z3W1V8Q4K7N2M5R8T1X4J9` | Holds | _approved, proved, shown able to fail_ | not run |"
        ),
        "{md}"
    );
    assert_eq!(md.matches("| Holds |").count(), 1, "only the proof holds: {md}");
    assert!(
        md.contains(
            "| Checked (tested, strength pending) | _tested on sampled inputs, not proved; strength not yet measured_ |"
        ),
        "{md}"
    );
    assert!(md.contains("| Not checked | _no check bound_ |"), "{md}");
    assert!(md.contains("| Re-approve | _statement amended_ |"), "{md}");
    assert!(md.contains("| Broken | _scope matches nothing_ |"), "{md}");
    assert!(md.contains("| Waived until 2026-12-31 | _failing, human-waived with an open issue_ |"), "{md}");
    assert!(md.contains("- approvals verified via GitHub reviews: yes\n- default branch requires review: yes\n- proob gate required: yes\n- CODEOWNERS covers proob: yes\n- scheduled evidence last ran: 1 day ago\n"), "{md}");
    legend_is_one_line(md);
    assert_golden("status.md", md);
}

#[test]
fn check_markdown_golden_with_if_merged_now_rows() {
    let k = with_pr();
    let results = k.results(json!({ "result": "counterexample", "witness": "f(0)" }));
    let out = run(&mut k.check(Some(&results)));
    assert_eq!(out.code, 1, "{}", out.stderr);
    let md = &out.stdout;
    assert!(md.starts_with("## proob gate: block (4 affected of 8); "), "{md}");
    assert!(md.contains("| Broken **blocks** | _counterexample on this PR_ | counterexample |"), "{md}");
    assert!(md.contains("**If merged now** (governed files in this change)"), "{md}");
    assert!(md.contains("| `proob/config.toml` | unreviewed: the approval predates the file's last change | re-approve the latest commit |"), "{md}");
    assert!(!md.contains("F matches nothing"), "an unaffected Broken is a headline count, not a row");
    assert!(md.contains("1 broken") || md.contains("2 broken"), "{md}");
    legend_is_one_line(md);
    assert_golden("check.md", md);
}

#[test]
fn bootstrap_markdown_golden() {
    let repo = TempRepo::new();
    repo.write("src/lib.rs", "pub fn f() {}\n");
    repo.commit("bare", NOW - 3 * DAY);
    repo.branch("pr");
    repo.write("proob/config.toml", CONFIG_TOML);
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "A is proposed", &["src/lib.rs"], "test"));
    repo.commit("introduce proob", NOW - DAY);
    let out = run(repo.proob().args(["check", "--base", "main", "--head", "pr"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_golden("bootstrap.md", &out.stdout);
}

#[test]
fn offline_markdown_golden() {
    let repo = TempRepo::new();
    repo.write("src/lib.rs", "pub fn f() {}\n");
    repo.write("proob/config.toml", CONFIG_TOML);
    repo.write(&property_path(PROP_A), &property_toml(PROP_A, "A is on main", &["src/lib.rs"], "test"));
    repo.commit("introduce proob", NOW - 3 * DAY);
    repo.write(&property_path(PROP_D), &property_toml(PROP_D, "D is proposed", &["src/lib.rs"], "test"));
    repo.add_origin();
    let out = run(repo.proob().arg("status"));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("| Not checked | _no GitHub token_ |"), "{}", out.stdout);
    assert!(out.stdout.contains("| Proposed | _never reviewed (not on the default branch)_ |"), "{}", out.stdout);
    assert_golden("status-offline.md", &out.stdout);
}
