//! The user-facing vocabulary: the plan's seven words, their modifiers, the
//! reason column, and the "this PR" column. Both renderers (Markdown and
//! JSON) read these, so the words agree everywhere they appear (R9).
use crate::cli::report::{RowUnverified, RowVerdict};
use proob_core::preview::Remedy;
use proob_core::types::{
    Approval, BrokenWhy, Ineligible, Kind, Known, NotCheckedWhy, ReApproveWhy, Secs, Sig, Source, UnreviewedReason,
    Verdict,
};
use proob_facts::date::Date;
use proob_facts::evidence::OutcomeRecord;
use proob_facts::results::RunResultRecord;

/// The status column: the word with its modifiers.
pub fn word(v: &Verdict) -> String {
    match v {
        Verdict::Holds => "Holds".to_string(),
        Verdict::Checked { strength_pending, self_signed } => {
            with_modifiers("Checked", &[(*strength_pending, "strength pending"), (*self_signed, "self-signed")])
        }
        Verdict::NotChecked { self_signed, .. } => with_modifiers("Not checked", &[(*self_signed, "self-signed")]),
        Verdict::Proposed => "Proposed".to_string(),
        Verdict::ReApprove(_) => "Re-approve".to_string(),
        Verdict::Broken(_) => "Broken".to_string(),
        Verdict::Waived { until } => format!("Waived until {}", date(*until)),
    }
}

fn with_modifiers(word: &str, modifiers: &[(bool, &str)]) -> String {
    let on: Vec<&str> = modifiers.iter().filter(|(set, _)| *set).map(|(_, m)| *m).collect();
    if on.is_empty() { word.to_string() } else { format!("{word} ({})", on.join(", ")) }
}

/// The modifiers alone, for JSON.
pub fn modifiers(v: &Verdict) -> Vec<&'static str> {
    match v {
        Verdict::Checked { strength_pending, self_signed } => {
            [(*strength_pending, "strength_pending"), (*self_signed, "self_signed")]
                .into_iter()
                .filter(|(set, _)| *set)
                .map(|(_, m)| m)
                .collect()
        }
        Verdict::NotChecked { self_signed: true, .. } => vec!["self_signed"],
        _ => Vec::new(),
    }
}

/// The reason column.
pub fn reason(v: &Verdict, kind: Option<Kind>) -> String {
    match v {
        Verdict::Holds => "approved, checked, proven able to fail".to_string(),
        Verdict::Checked { strength_pending, self_signed } => {
            [(*strength_pending, "strength not yet measured"), (*self_signed, "no second reviewer")]
                .into_iter()
                .filter(|(set, _)| *set)
                .map(|(_, r)| r)
                .collect::<Vec<_>>()
                .join("; ")
        }
        Verdict::NotChecked { reason, .. } => not_checked_reason(*reason).to_string(),
        Verdict::Proposed => "never reviewed".to_string(),
        Verdict::ReApprove(why) => re_approve_reason(*why),
        Verdict::Broken(why) => broken_reason(*why, kind),
        Verdict::Waived { .. } => "failing, human-waived with an open issue".to_string(),
    }
}

pub fn not_checked_reason(why: NotCheckedWhy) -> &'static str {
    match why {
        NotCheckedWhy::NoAdapter => "no adapter",
        NotCheckedWhy::NoCheck => "no check bound",
        NotCheckedWhy::CheckUnreviewed => "check unreviewed",
        NotCheckedWhy::EvidencePending => "evidence pending",
        NotCheckedWhy::EvidenceStale => "evidence stale",
        NotCheckedWhy::AdapterErrored => "adapter errored",
        NotCheckedWhy::GitHubUnreachable => "GitHub unreachable",
        NotCheckedWhy::RateLimited => "rate limited",
    }
}

pub fn re_approve_reason(why: ReApproveWhy) -> String {
    match why {
        ReApproveWhy::StatementAmended => "statement amended".to_string(),
        ReApproveWhy::CheckAmended => "check amended".to_string(),
        ReApproveWhy::ConfigAmended => "config amended".to_string(),
        ReApproveWhy::WorkflowAmended => "workflow amended".to_string(),
        ReApproveWhy::ReviewerAccessRevoked => "reviewer access revoked".to_string(),
        ReApproveWhy::SpecDrifted(n) => format!("spec drifted: {n} commit{} to the modelled scope", plural(n)),
    }
}

fn broken_reason(why: BrokenWhy, kind: Option<Kind>) -> String {
    match why {
        BrokenWhy::Negative(source) => format!("{} on {}", negative_word(kind), source_words(source)),
        BrokenWhy::RunnerErrored => "runner error on this PR".to_string(),
        BrokenWhy::ScopeEmpty => "scope matches nothing".to_string(),
        BrokenWhy::WaiverExpired => "waiver expired or its issue closed".to_string(),
    }
}

fn negative_word(kind: Option<Kind>) -> &'static str {
    match kind {
        Some(Kind::Test) => "counterexample",
        Some(Kind::Proof) => "proof failed",
        Some(Kind::Model) => "violation",
        None => "negative result",
    }
}

fn source_words(source: Source) -> &'static str {
    match source {
        Source::ThisPr => "this PR",
        Source::DefaultBranch => "the default branch",
    }
}

/// The status column of a row that reached no verdict.
pub fn row_word(v: &RowVerdict) -> String {
    match v {
        RowVerdict::Derived(d) => word(&d.verdict),
        RowVerdict::Proposal => "Proposed".to_string(),
        RowVerdict::Unverified(RowUnverified::RateLimited) => "Unverified (rate limited)".to_string(),
        RowVerdict::Unverified(_) => "Not checked".to_string(),
    }
}

pub fn row_reason(v: &RowVerdict, kind: Option<Kind>) -> String {
    match v {
        RowVerdict::Derived(d) => reason(&d.verdict, kind),
        RowVerdict::Proposal => "never reviewed (not on the default branch)".to_string(),
        RowVerdict::Unverified(u) => u.reason(),
    }
}

/// The "this PR" column, from the execute job's raw record.
pub fn this_pr(record: Option<&RunResultRecord>) -> String {
    match record.map(|r| &r.outcome) {
        None => "not run".to_string(),
        Some(OutcomeRecord::NoCounterexample { budget }) => format!("passed {budget} example{}", plural(*budget)),
        Some(OutcomeRecord::Counterexample { .. }) => "counterexample".to_string(),
        Some(OutcomeRecord::Verified { .. }) => "verified".to_string(),
        Some(OutcomeRecord::ProofFailed { .. }) => "proof failed".to_string(),
        Some(OutcomeRecord::NoViolation) => "no violation".to_string(),
        Some(OutcomeRecord::Violation { .. }) => "violation".to_string(),
        Some(OutcomeRecord::Errored { .. }) => "runner error".to_string(),
        Some(OutcomeRecord::Scored { killed, survived, .. }) => format!("scored {killed} killed, {survived} survived"),
        Some(OutcomeRecord::NoMutants) => "no mutants".to_string(),
    }
}

/// An approval, as the footer and the if-merged-now rows say it.
pub fn approval_words(a: Approval) -> String {
    match a {
        Approval::Approved { sig: Sig::Team, pr, .. } => format!("team-approved (PR #{pr})"),
        Approval::Approved { sig: Sig::SelfSigned, pr, .. } => format!("self-signed (PR #{pr}, no eligible review)"),
        Approval::Unreviewed(UnreviewedReason::DirectPush) => "unreviewed: direct push, no pull request".to_string(),
        Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked) => {
            "unreviewed: the approver lost write access".to_string()
        }
        Approval::Unreviewed(UnreviewedReason::NoEligibleReview(why)) => {
            format!("unreviewed: {}", ineligible_words(why))
        }
    }
}

fn ineligible_words(why: Ineligible) -> &'static str {
    match why {
        Ineligible::NoReview => "no approving review",
        Ineligible::ApproverEditedGoverned => "every approver edited a governed file in the PR",
        Ineligible::ApprovalPredatesChange => "the approval predates the file's last change",
        Ineligible::BotOnly => "only bots approved",
        Ineligible::NoWriteAccess => "no approver has write access",
        Ineligible::UnknownIdentity => "a reviewer or commit author cannot be resolved",
    }
}

/// The remedy column of an if-merged-now row.
pub fn remedy_words(r: Remedy) -> &'static str {
    match r {
        Remedy::None => "none: the merge will be team-approved",
        Remedy::OptionalSecondReviewer => {
            "optional: a second reviewer with write access who did not edit a governed file makes it team-approved"
        }
        Remedy::AddReviewerWithWriteAccess => {
            "request a review from a human with write access who did not edit a governed file"
        }
        Remedy::AskAuthorToCommitSuggestion => {
            "redo the reviewer's edit as a suggestion the author commits, then re-approve; or add a reviewer who did not edit"
        }
        Remedy::ReApproveLatestCommit => "re-approve the latest commit",
        Remedy::ResolveIdentity => "link the commit email to a GitHub account, or add a resolvable reviewer",
        Remedy::OpenPullRequest => "open a pull request: a direct push is never approved",
    }
}

pub fn known_words(k: Known) -> &'static str {
    match k {
        Known::Yes => "yes",
        Known::No => "no",
        Known::Unknown => "unknown",
    }
}

pub fn date(secs: Secs) -> String {
    Date::from_secs(secs).map_or_else(|| format!("+{secs}s"), |d| d.to_string())
}

pub fn days_ago(age: Secs) -> String {
    let days = age / proob_facts::date::SECS_PER_DAY;
    match days {
        0 => "today".to_string(),
        1 => "1 day ago".to_string(),
        n => format!("{n} days ago"),
    }
}

pub fn plural(n: u64) -> &'static str {
    if n == 1 { "" } else { "s" }
}
