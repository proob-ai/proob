//! The `View`: what a run of `status` or `check` produced, before rendering.
//!
//! A report is `Verified` only when GitHub's records were read in this run
//! (a token, or the fixture backend); every other run is `Unverified` with the
//! reason, and a bootstrap check never consults GitHub at all. Rows carry the
//! verified `Status` and `Verdict` when the facts for them were gathered, and
//! say why not otherwise; a row whose facts never reached `derive` has no
//! verdict, so no code path here can render a word the core did not produce.
//! `Holds` needs fresh default-branch evidence, which only a live run can list,
//! so it is reachable only through `Verified`; `derive_row` in the pipeline
//! refuses it anywhere else.
use proob_core::preview::Preview;
use proob_core::types::{Governance, Grace, Kind, Known, Oid, Outcome, ReApproveWhy, Secs, Severity, Status, Verdict};
use proob_facts::results::RunResultRecord;

pub const GATE_CHECK_NAME: &str = "proob gate";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Status,
    Check {
        base: String,
        head: String,
    },
    /// The base ref has no `proob/config.toml`: the gate is not yet in force.
    Bootstrap {
        base: String,
        head: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedSource {
    Live,
    Fixture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnverifiedReason {
    /// No token was found; cached facts of this age were consulted, if any.
    NoToken { cache_age: Option<Secs> },
    /// No GitHub remote could be named for this repository.
    NoRemote { cache_age: Option<Secs> },
    /// GitHub answered with something other than facts.
    GitHubUnreachable { message: String },
    /// The call budget or GitHub's rate limit stopped the run before any
    /// governance fact was read.
    RateLimited,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    Verified {
        source: VerifiedSource,
    },
    Unverified(UnverifiedReason),
    /// Bootstrap: GitHub was not consulted because nothing is governed yet.
    NotConsulted,
}

impl Verification {
    pub fn is_verified(&self) -> bool {
        matches!(self, Verification::Verified { .. })
    }
}

/// Why one row has no derived verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowUnverified {
    RateLimited,
    NoToken,
    Unreachable { message: String },
}

impl RowUnverified {
    /// The words for the reason column.
    pub fn reason(&self) -> String {
        match self {
            RowUnverified::RateLimited => "rate limited".to_string(),
            RowUnverified::NoToken => "no GitHub token".to_string(),
            RowUnverified::Unreachable { message } => format!("GitHub unreachable: {}", shorten(message, 72)),
        }
    }
}

fn shorten(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

/// The facts for the row reached the verified core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    pub status: Status,
    pub verdict: Verdict,
    /// The grace window, when the declaration has an approving merge.
    pub grace: Option<Grace>,
    /// The gate's outcome, check mode only.
    pub outcome: Option<Outcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowVerdict {
    Derived(Derived),
    /// The file is not on the default branch, a fact git alone establishes:
    /// never approved, so `Proposed` without consulting GitHub.
    Proposal,
    Unverified(RowUnverified),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub path: String,
    pub title: String,
    pub kind: Kind,
    pub severity: Severity,
    pub verdict: RowVerdict,
    /// Check mode: some changed path is a scope or binding file of this property.
    pub affected: Option<bool>,
    /// This PR's own result for the property, when the execute job produced one.
    pub this_pr: Option<RunResultRecord>,
}

impl Row {
    pub fn blocks(&self) -> bool {
        matches!(&self.verdict, RowVerdict::Derived(Derived { outcome: Some(Outcome::Block), .. }))
    }
}

/// An "if merged now" row for a governed file in the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfMerged {
    pub path: String,
    pub preview: Result<Preview, RowUnverified>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalsVerified {
    Yes,
    No,
    RateLimited,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footer {
    pub approvals: ApprovalsVerified,
    pub branch_requires_review: Known,
    pub gate_required: Known,
    pub codeowners: Known,
    /// Age of the newest score record (scores come from the scheduled run).
    pub scheduled_evidence_age: Option<Secs>,
}

/// Which config governed this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigInForce {
    /// The approved `proob/config.toml` of the base (or default) ref.
    Approved,
    /// The built-in defaults, because the config is not approved, is absent,
    /// or the gate is not in force yet.
    Defaults(DefaultsWhy),
    /// GitHub was not consulted, so the config's approval is unknown; the
    /// candidate's values are shown, nothing is gated.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultsWhy {
    Unapproved,
    Missing,
    Bootstrap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub mode: Mode,
    pub verification: Verification,
    pub default_branch: String,
    pub default_head: Option<Oid>,
    pub config: ConfigInForce,
    /// Repository-wide amendments for the headline (`governance_verdicts`).
    pub amendments: Vec<ReApproveWhy>,
    pub governance: Option<Governance>,
    pub rows: Vec<Row>,
    pub if_merged_now: Vec<IfMerged>,
    pub pull_request: Option<u64>,
    pub footer: Footer,
    pub notes: Vec<String>,
    pub calls_spent: u32,
    /// Changed source files no property covers (`uncovered`), informational
    /// only; `None` only before the command attaches it.
    pub uncovered: Option<Vec<String>>,
}

/// How many rows carry each word.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub holds: usize,
    pub checked: usize,
    pub not_checked: usize,
    pub re_approve: usize,
    pub broken: usize,
    pub proposed: usize,
    pub waived: usize,
    pub unverified: usize,
}

impl Counts {
    fn add(mut self, row: &Row) -> Counts {
        match &row.verdict {
            RowVerdict::Derived(d) => match d.verdict {
                Verdict::Holds => self.holds += 1,
                Verdict::Checked { .. } => self.checked += 1,
                Verdict::NotChecked { .. } => self.not_checked += 1,
                Verdict::ReApprove(_) => self.re_approve += 1,
                Verdict::Broken(_) => self.broken += 1,
                Verdict::Proposed => self.proposed += 1,
                Verdict::Waived { .. } => self.waived += 1,
            },
            RowVerdict::Proposal => self.proposed += 1,
            RowVerdict::Unverified(_) => self.unverified += 1,
        }
        self
    }

    pub fn total(&self) -> usize {
        self.holds
            + self.checked
            + self.not_checked
            + self.re_approve
            + self.broken
            + self.proposed
            + self.waived
            + self.unverified
    }
}

impl Report {
    pub fn counts(&self) -> Counts {
        self.rows.iter().fold(Counts::default(), Counts::add)
    }

    pub fn blocks(&self) -> bool {
        self.rows.iter().any(Row::blocks)
    }

    pub fn warns(&self) -> bool {
        self.rows
            .iter()
            .any(|r| matches!(&r.verdict, RowVerdict::Derived(Derived { outcome: Some(Outcome::Warn), .. })))
    }

    pub fn affected_rows(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|r| r.affected == Some(true))
    }
}
