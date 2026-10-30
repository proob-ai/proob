//! The Markdown view: the headline, the property table in the plan's
//! vocabulary (word, reason, this PR), the "if merged now" rows, the footer,
//! and a one-line legend. The same text goes to a terminal, a job summary,
//! and a `::notice::` annotation, so it is plain Markdown without HTML.
use crate::cli::report::*;
use crate::cli::vocab::*;
use proob_core::types::{Kind, Outcome, ReApproveWhy};
use std::fmt::Write as _;

const LEGEND: &str = "Legend: Holds (approved, proved, shown able to fail) · Checked (tested, strength pending, or self-signed) · \
                      Not checked (approved, nothing checked yet) · Re-approve (changed since approval) · \
                      Broken (negative result, empty scope, or expired waiver) · Proposed (never approved) · \
                      Waived until d (failing, human-waived)";

/// The counts as words, non-zero ones only.
fn counts_words(c: Counts) -> String {
    let parts: Vec<String> = [
        (c.holds, "holds"),
        (c.checked, "checked"),
        (c.not_checked, "not checked"),
        (c.re_approve, "re-approve"),
        (c.broken, "broken"),
        (c.proposed, "proposed"),
        (c.waived, "waived"),
        (c.unverified, "unverified"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, w)| format!("{n} {w}"))
    .collect();
    if parts.is_empty() { "no properties".to_string() } else { parts.join(", ") }
}

/// The marker every unverified rendering carries (R12).
pub fn marker(v: &Verification) -> Option<String> {
    let age = |cache_age: Option<u64>| match cache_age {
        Some(a) => format!("; cached facts from {}", days_ago(a)),
        None => "; no cached facts".to_string(),
    };
    match v {
        Verification::Verified { .. } => None,
        Verification::Unverified(UnverifiedReason::NoToken { cache_age }) => {
            Some(format!("unverified: no GitHub token{}", age(*cache_age)))
        }
        Verification::Unverified(UnverifiedReason::NoRemote { cache_age }) => {
            Some(format!("unverified: no GitHub remote{}", age(*cache_age)))
        }
        Verification::Unverified(UnverifiedReason::GitHubUnreachable { message }) => {
            Some(format!("unverified: GitHub unreachable ({message})"))
        }
        Verification::Unverified(UnverifiedReason::RateLimited) => Some("unverified: rate limited".to_string()),
        Verification::NotConsulted => None,
    }
}

fn amendments_words(amendments: &[ReApproveWhy]) -> Option<String> {
    if amendments.is_empty() {
        return None;
    }
    let names: Vec<String> = amendments.iter().map(|a| re_approve_reason(*a)).collect();
    Some(format!("Re-approve ({})", names.join("; ")))
}

fn gate_word(r: &Report) -> &'static str {
    if r.blocks() {
        "block"
    } else if r.warns() {
        "warn"
    } else {
        "pass"
    }
}

/// The one line `--summary` and the session hook print.
pub fn summary_line(r: &Report) -> String {
    let mut line = match &r.mode {
        Mode::Status => format!("proob: {}", counts_words(r.counts())),
        Mode::Check { .. } => format!(
            "proob gate: {} ({} affected of {}); {}",
            gate_word(r),
            r.affected_rows().count(),
            r.rows.len(),
            counts_words(r.counts())
        ),
        Mode::Bootstrap { .. } => format!("proob gate takes effect after this PR merges; {}", counts_words(r.counts())),
    };
    if let Some(a) = amendments_words(&r.amendments) {
        let _ = write!(line, "; {a}");
    }
    if let Some(m) = marker(&r.verification) {
        let _ = write!(line, " [{m}]");
    }
    line
}

/// The session hook's line: the summary, plus a pointer to `/proob:declare`
/// when the branch or the uncommitted work adds code no property covers.
pub fn hook_line(r: &Report) -> String {
    match r.uncovered.as_deref().map(<[String]>::len) {
        None | Some(0) => summary_line(r),
        Some(1) => format!("{} · 1 changed file has no property: /proob:declare", summary_line(r)),
        Some(n) => format!("{} · {n} changed files have no property: /proob:declare", summary_line(r)),
    }
}

/// The check report's informational line about new code, when there is
/// some; `status` keeps it to `--json` and the session hook.
fn uncovered_line(r: &Report) -> Option<String> {
    let paths = r.uncovered.as_deref().filter(|p| !p.is_empty() && !matches!(r.mode, Mode::Status))?;
    Some(format!(
        "New code without a property: {} · run /proob:declare on this branch to propose some\n",
        crate::cli::uncovered::listing(paths)
    ))
}

fn property_cell(row: &Row) -> String {
    format!("{} `{}`", row.title.replace('|', "\\|"), row.id)
}

fn table_row(row: &Row) -> String {
    let kind: Option<Kind> = Some(row.kind);
    let outcome = match &row.verdict {
        RowVerdict::Derived(Derived { outcome: Some(Outcome::Block), .. }) => " **blocks**",
        RowVerdict::Derived(Derived { outcome: Some(Outcome::Warn), .. }) => " (warn)",
        _ => "",
    };
    format!(
        "| {} | {}{} | _{}_ | {} |",
        property_cell(row),
        row_word(&row.verdict),
        outcome,
        row_reason(&row.verdict, kind).replace('|', "\\|"),
        this_pr(row.this_pr.as_ref())
    )
}

fn table<'a>(rows: impl Iterator<Item = &'a Row>) -> String {
    let mut out = String::from("| Property | Status | Reason | This PR |\n|---|---|---|---|\n");
    for row in rows {
        let _ = writeln!(out, "{}", table_row(row));
    }
    out
}

fn if_merged_table(rows: &[IfMerged]) -> String {
    let mut out = String::from(
        "**If merged now** (governed files in this change)\n\n| File | Approval if merged now | Remedy |\n|---|---|---|\n",
    );
    for r in rows {
        let (approval, remedy) = match &r.preview {
            Ok(p) => (approval_words(p.approval), remedy_words(p.remedy).to_string()),
            Err(u) => (format!("unverified ({})", u.reason()), "retry with GitHub reachable".to_string()),
        };
        let _ = writeln!(out, "| `{}` | {approval} | {remedy} |", r.path);
    }
    out
}

fn footer_lines(f: &Footer) -> String {
    let approvals = match f.approvals {
        ApprovalsVerified::Yes => "yes",
        ApprovalsVerified::No => "no",
        ApprovalsVerified::RateLimited => "rate-limited",
    };
    let evidence = match f.scheduled_evidence_age {
        Some(age) => days_ago(age),
        None => "never".to_string(),
    };
    format!(
        "- approvals verified via GitHub reviews: {approvals}\n- default branch requires review: {}\n- proob gate required: {}\n- CODEOWNERS covers proob: {}\n- scheduled evidence last ran: {evidence}\n",
        known_words(f.branch_requires_review),
        known_words(f.gate_required),
        known_words(f.codeowners),
    )
}

fn inputs_line(r: &Report) -> String {
    let head = r.default_head.map(|h| proob_facts::hex::encode_oid(&h)[..12].to_string());
    match (&r.mode, head) {
        (Mode::Status, Some(h)) => format!(
            "Inputs: working tree, {} at {h}, GitHub review records and default-branch evidence.",
            r.default_branch
        ),
        (Mode::Status, None) => format!("Inputs: working tree, {}.", r.default_branch),
        (Mode::Check { base, head: pr_head }, _) => format!(
            "Inputs: head {pr_head} against base {base}; config from {}; GitHub review records and default-branch evidence; this PR's own results.",
            match r.config {
                ConfigInForce::Approved => "the approved base config",
                ConfigInForce::Defaults(_) => "the built-in defaults",
                ConfigInForce::Unknown => "the candidate (approval unknown)",
            }
        ),
        (Mode::Bootstrap { base, head: pr_head }, _) => {
            format!("Inputs: head {pr_head} against base {base}; GitHub not consulted.")
        }
    }
}

/// The whole Markdown report.
pub fn markdown(r: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## {}\n", summary_line(r));
    if let Some(a) = amendments_words(&r.amendments) {
        let _ = writeln!(
            out,
            "{a}: governed content changed since its approval; the built-in policy applies until it is re-approved.\n"
        );
    }
    let _ = writeln!(out, "{}\n", inputs_line(r));
    match &r.mode {
        Mode::Status | Mode::Bootstrap { .. } => out.push_str(&table(r.rows.iter())),
        Mode::Check { .. } => {
            let affected = r.affected_rows().count();
            if affected == 0 {
                let _ = writeln!(out, "No property is affected by this change; the headline counts every property.");
            } else {
                out.push_str(&table(r.affected_rows()));
            }
        }
    }
    out.push('\n');
    if let Some(line) = uncovered_line(r) {
        out.push_str(&line);
        out.push('\n');
    }
    if !r.if_merged_now.is_empty() {
        out.push_str(&if_merged_table(&r.if_merged_now));
        out.push('\n');
    }
    for note in &r.notes {
        let _ = writeln!(out, "- note: {note}");
    }
    if !r.notes.is_empty() {
        out.push('\n');
    }
    out.push_str(&footer_lines(&r.footer));
    let _ = write!(out, "\n{LEGEND}\n");
    out
}
