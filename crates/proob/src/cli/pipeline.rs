//! The derivation a run performs, in the order the budget wants it:
//! governance facts, the evidence expectation, one evidence retrieval, then
//! every property (affected ones first) through `gather`, the verified
//! `derive`, `verdict`, and, in check mode, `policy`; and the "if merged now"
//! rows through `introduction_if_merged` and the verified `preview`.
//!
//! Nothing here decides anything about a property. It classifies failures
//! (rate limited, no token, unreachable, or fatal) and assembles the `Report`.
use crate::cli::backend::{Backend, Bindings, OfflineWhy, Source};
use crate::cli::error::CliError;
use crate::cli::report::*;
use crate::cli::workspace::{LoadedProperty, Workspace};
use proob_core::affected::affected;
use proob_core::derive::derive;
use proob_core::policy::{grace_of, policy};
use proob_core::preview::preview;
use proob_core::types::{
    Approval, Decl, GateConfig, Governance, Grace, Kind, Known, Measured, Oid, PropertyId, RunResult, Status, Verdict,
};
use proob_core::verdict::{governance_verdicts, verdict};
use proob_effects::error::{ApiError, EvidenceError, FactsError, GatherError};
use proob_effects::evidence::{EvidenceQuery, EvidenceSet, retrieve};
use proob_effects::facts::introduction_if_merged;
use proob_effects::gather::{Context, PropertyRequest, evidence_expectation, gather, governance_facts, resolve_scope};
use proob_facts::config::Config;
use proob_facts::convert;
use proob_facts::github::PullRequest;
use proob_facts::hex;
use proob_facts::paths::CONFIG;
use proob_facts::results::RunResultRecord;
use std::collections::BTreeMap;

const HISTORY_DEPTH: usize = 8;
const MAX_EVIDENCE_RUNS: usize = 50;

/// What every run has.
pub struct Inputs<'a> {
    pub ws: &'a Workspace,
    pub backend: &'a Backend<'a>,
    pub properties: &'a [LoadedProperty],
    pub default_branch: &'a str,
    pub default_ref: &'a str,
    /// The config whose approval is judged: the working tree's for `status`,
    /// the base ref's for `check`.
    pub candidate: &'a Config,
    pub candidate_missing: bool,
}

/// What check mode adds.
pub struct CheckInputs<'a> {
    pub base: &'a str,
    pub head: &'a str,
    pub results: &'a BTreeMap<PropertyId, RunResultRecord>,
    pub pr: Option<u64>,
    /// `allow_self_signed` of the head's config, which judges the config's
    /// own "if merged now" row.
    pub head_allow_self_signed: bool,
}

// ---------------------------------------------------------------------------
// Failure classification
// ---------------------------------------------------------------------------

enum Blocker {
    Row(RowUnverified),
    Fatal(CliError),
}

fn classify_api(e: &ApiError) -> RowUnverified {
    match e {
        ApiError::RateLimited => RowUnverified::RateLimited,
        ApiError::NoToken => RowUnverified::NoToken,
        other => RowUnverified::Unreachable { message: other.to_string() },
    }
}

fn classify(e: GatherError) -> Blocker {
    match &e {
        GatherError::Api(a) | GatherError::Facts(FactsError::Api(a)) | GatherError::Evidence(EvidenceError::Api(a)) => {
            Blocker::Row(classify_api(a))
        }
        _ => Blocker::Fatal(CliError::failed(e)),
    }
}

fn classify_facts(e: FactsError) -> Blocker {
    match &e {
        FactsError::Api(a) => Blocker::Row(classify_api(a)),
        _ => Blocker::Fatal(CliError::failed(e)),
    }
}

fn unverified_reason(source: Source, blocker: &RowUnverified) -> UnverifiedReason {
    match (source, blocker) {
        (_, RowUnverified::RateLimited) => UnverifiedReason::RateLimited,
        (Source::Offline { why: OfflineWhy::NoRemote, cache_age }, _) => UnverifiedReason::NoRemote { cache_age },
        (Source::Offline { why: OfflineWhy::NoToken, cache_age }, _) => UnverifiedReason::NoToken { cache_age },
        (_, RowUnverified::NoToken) => UnverifiedReason::NoToken { cache_age: None },
        (_, RowUnverified::Unreachable { message }) => UnverifiedReason::GitHubUnreachable { message: message.clone() },
    }
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

fn kind_of(p: &LoadedProperty) -> Kind {
    p.file.kind.map_or(Kind::Test, convert::kind)
}

fn row(p: &LoadedProperty, verdict: RowVerdict, affected: Option<bool>, this_pr: Option<&RunResultRecord>) -> Row {
    Row {
        id: p.file.id.clone(),
        path: p.path.clone(),
        title: p.file.title.clone(),
        kind: kind_of(p),
        severity: convert::severity(p.file.severity),
        verdict,
        affected,
        this_pr: this_pr.cloned(),
    }
}

/// Whether the file is on the default branch at all: a local proposal is not.
fn on_default_branch(inp: &Inputs<'_>, path: &str) -> Result<bool, CliError> {
    Ok(!inp.backend.repo.touching_commits(inp.default_ref, &[path], 1).map_err(CliError::failed)?.is_empty())
}

/// The rows of a run whose GitHub facts are out of reach: proposals from git,
/// everything else unverified.
fn git_only_rows(
    inp: &Inputs<'_>,
    blocker: &RowUnverified,
    affected: &BTreeMap<String, bool>,
    results: Option<&BTreeMap<PropertyId, RunResultRecord>>,
) -> Result<Vec<Row>, CliError> {
    inp.properties
        .iter()
        .map(|p| {
            let verdict = if on_default_branch(inp, &p.path)? {
                RowVerdict::Unverified(blocker.clone())
            } else {
                RowVerdict::Proposal
            };
            Ok(row(p, verdict, affected.get(&p.path).copied(), results.and_then(|r| r.get(&p.id))))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

enum Evidence {
    Retrieved(EvidenceSet),
    /// No evidence workflow on the default branch: nothing to retrieve.
    NoWorkflow,
    /// The retrieval failed; bound properties cannot be judged.
    Failed(RowUnverified),
}

impl Evidence {
    fn set(&self) -> EvidenceSet {
        match self {
            Evidence::Retrieved(set) => set.clone(),
            _ => EvidenceSet::default(),
        }
    }
}

fn retrieve_evidence(
    ctx: &Context<'_>,
    g: &Governance,
    gate: &GateConfig,
    wanted: &[(PropertyId, Kind)],
) -> Result<Evidence, CliError> {
    let expectation = match evidence_expectation(ctx, g) {
        Ok(e) => e,
        Err(GatherError::MissingBlob { .. }) => return Ok(Evidence::NoWorkflow),
        Err(e) => {
            return match classify(e) {
                Blocker::Row(u) => Ok(Evidence::Failed(u)),
                Blocker::Fatal(e) => Err(e),
            };
        }
    };
    let query = EvidenceQuery {
        expectation: &expectation,
        now: ctx.now,
        freshness_secs: gate.freshness_secs,
        wanted,
        max_runs: MAX_EVIDENCE_RUNS,
    };
    match retrieve(ctx.api, ctx.repo, &query) {
        Ok(set) => Ok(Evidence::Retrieved(set)),
        Err(EvidenceError::Api(a)) => Ok(Evidence::Failed(classify_api(&a))),
        Err(e) => Ok(Evidence::Failed(RowUnverified::Unreachable { message: format!("evidence unreadable: {e}") })),
    }
}

fn scheduled_evidence_age(evidence: &Evidence, now: u64) -> Option<u64> {
    match evidence {
        Evidence::Retrieved(set) => set
            .records
            .iter()
            .filter(|r| matches!(r.measured, Measured::Score(_)))
            .map(|r| r.at)
            .max()
            .map(|at| now.saturating_sub(at)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// One property
// ---------------------------------------------------------------------------

struct Derivation<'a> {
    ctx: &'a Context<'a>,
    g: &'a Governance,
    gate: &'a GateConfig,
    waiver_max_days: u32,
    evidence: &'a Evidence,
    default_head: Oid,
    bindings: Option<&'a Bindings>,
    verified: bool,
}

fn provisional_of(record: Option<&RunResultRecord>, path: &str) -> Result<Option<RunResult>, CliError> {
    record.map(|r| convert::run_result(r).map_err(|e| CliError::invalid(path, format!("results: {e}")))).transpose()
}

fn grace_for(ctx: &Context<'_>, status: &Status, grace_secs: u64) -> Result<Option<Grace>, CliError> {
    let since = match status {
        Status::ScopeEmpty { decl } | Status::Scoped { decl, .. } => match decl {
            Decl::Approved { since, .. } | Decl::Waived { since, .. } => Some(*since),
            Decl::Proposed(_) => None,
        },
    };
    since
        .map(|s| ctx.repo.commit_time(&s).map(|at| grace_of(at, ctx.now, grace_secs)).map_err(CliError::failed))
        .transpose()
}

impl Derivation<'_> {
    fn derive_row(
        &self,
        p: &LoadedProperty,
        affected: Option<bool>,
        record: Option<&RunResultRecord>,
    ) -> Result<Row, CliError> {
        let binding = self.bindings.and_then(|b| b.by_id.get(&p.id));
        if let (Some(_), Evidence::Failed(u)) = (binding, self.evidence) {
            return Ok(row(p, RowVerdict::Unverified(u.clone()), affected, record));
        }
        let request = PropertyRequest {
            path: &p.path,
            file: &p.file,
            kind_default: Kind::Test,
            adapter_available: self.bindings.is_some_and(|b| !b.no_adapter.contains(&p.id)),
            binding,
            provisional: provisional_of(record, &p.path)?,
            waiver_max_days: self.waiver_max_days,
        };
        match gather(self.ctx, &request, &self.evidence.set(), self.default_head) {
            Ok(input) => self.judge(p, input, affected, record),
            Err(e) => match classify(e) {
                Blocker::Row(u) => Ok(row(p, RowVerdict::Unverified(u), affected, record)),
                Blocker::Fatal(e) => Err(e),
            },
        }
    }

    fn judge(
        &self,
        p: &LoadedProperty,
        input: proob_core::types::DeriveInput,
        affected: Option<bool>,
        record: Option<&RunResultRecord>,
    ) -> Result<Row, CliError> {
        let status = derive(input, self.gate, self.ctx.now);
        let v = verdict(&status, self.g);
        if !self.verified && matches!(v, Verdict::Holds | Verdict::Checked { .. }) {
            // Fresh evidence needs a live run listing, which the cache never
            // holds; reaching here means the cache held one. Fail rather than
            // render a word an unverified view may not carry.
            return Err(CliError::failed(format!(
                "{}: derived `{}` without a token; the fact cache held something it must not (report this)",
                p.path,
                crate::cli::vocab::word(&v)
            )));
        }
        let grace = grace_for(self.ctx, &status, self.gate.grace_secs)?;
        let outcome = affected.map(|a| policy(&v, a, grace.unwrap_or(Grace::After), &self.gate.policy));
        Ok(row(p, RowVerdict::Derived(Derived { status, verdict: v, grace, outcome }), affected, record))
    }
}

// ---------------------------------------------------------------------------
// The affected set
// ---------------------------------------------------------------------------

fn bytes(paths: &[String]) -> Vec<Vec<u8>> {
    paths.iter().map(|p| p.as_bytes().to_vec()).collect()
}

/// Which properties the changed paths affect, by property path. Scopes are
/// resolved against the working tree exactly as `gather` resolves them.
fn affected_set(
    inp: &Inputs<'_>,
    changed: &[String],
    waiver_max_days: u32,
) -> Result<BTreeMap<String, bool>, CliError> {
    affected_paths(inp.backend.repo, inp.backend.bindings, inp.properties, changed, inp.ws.now, waiver_max_days)
}

/// The affected set as the verified `affected` decides it, for `check` and
/// for the Action's execute job, which must run exactly the properties the
/// judge will call affected.
pub fn affected_paths(
    repo: &dyn proob_effects::git::Repo,
    bindings: Option<&Bindings>,
    properties: &[LoadedProperty],
    changed: &[String],
    now: u64,
    waiver_max_days: u32,
) -> Result<BTreeMap<String, bool>, CliError> {
    let changed = bytes(changed);
    let binding_files: BTreeMap<PropertyId, Vec<Vec<u8>>> =
        bindings.map(|b| b.by_id.iter().map(|(id, set)| (*id, bytes(&set.files))).collect()).unwrap_or_default();
    properties
        .iter()
        .map(|p| {
            let property =
                convert::property(&p.file, now, waiver_max_days).map_err(|e| CliError::invalid(&p.path, e))?;
            let scope = resolve_scope(repo, &property.scope).map_err(CliError::failed)?;
            let scope_files: Vec<Vec<u8>> = scope.into_iter().map(|pd| pd.path).collect();
            let empty = Vec::new();
            let binding = binding_files.get(&p.id).unwrap_or(&empty);
            Ok((p.path.clone(), affected(&changed, &scope_files, binding)))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// If merged now
// ---------------------------------------------------------------------------

fn pr_number_from_env() -> Option<u64> {
    let git_ref = std::env::var("GITHUB_REF").ok()?;
    git_ref.strip_prefix("refs/pull/")?.split('/').next()?.parse().ok()
}

/// The open pull request this head belongs to.
fn find_pr(inp: &Inputs<'_>, head_sha: &Oid, flag: Option<u64>) -> Result<Option<PullRequest>, RowUnverified> {
    let api = inp.backend.api;
    if let Some(n) = flag.or_else(pr_number_from_env) {
        return api.pull(n).map(Some).map_err(|e| classify_api(&e));
    }
    let pulls = api.pulls_for_commit(&hex::encode_oid(head_sha)).map_err(|e| classify_api(&e))?;
    Ok(pulls.into_iter().find(|p| !p.is_merged() && p.base.name == inp.default_branch))
}

fn if_merged_rows(
    inp: &Inputs<'_>,
    ctx: &Context<'_>,
    chk: &CheckInputs<'_>,
    changed: &[String],
    head_sha: &Oid,
    gate_allow: bool,
    notes: &mut Vec<String>,
) -> Result<(Vec<IfMerged>, Option<u64>), CliError> {
    let governed: Vec<&String> = changed.iter().filter(|p| ctx.governed.is_governed(p)).collect();
    if governed.is_empty() {
        return Ok((Vec::new(), None));
    }
    let pr = match find_pr(inp, head_sha, chk.pr) {
        Ok(Some(pr)) => pr,
        Ok(None) => {
            notes.push(format!(
                "no open pull request found for {}; pass --pr to preview approvals",
                hex::encode_oid(head_sha)
            ));
            return Ok((Vec::new(), None));
        }
        Err(u) => {
            return Ok((
                governed.into_iter().map(|p| IfMerged { path: p.clone(), preview: Err(u.clone()) }).collect(),
                None,
            ));
        }
    };
    let mut rows = Vec::with_capacity(governed.len());
    for path in governed {
        if ctx.repo.read_file(path).map_err(CliError::failed)?.is_none() {
            notes.push(format!("{path}: deleted in this change (retirement needs no approval)"));
            continue;
        }
        let allow = if path == CONFIG { chk.head_allow_self_signed } else { gate_allow };
        let preview = match introduction_if_merged(ctx.api, ctx.governed, ctx.default_branch, &[path.as_str()], &pr) {
            Ok(intro) => Ok(preview(&intro, allow)),
            Err(e) => match classify_facts(e) {
                Blocker::Row(u) => Err(u),
                Blocker::Fatal(e) => return Err(e),
            },
        };
        rows.push(IfMerged { path: path.clone(), preview });
    }
    Ok((rows, Some(pr.number)))
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

fn approvals_verified(verification: &Verification, rows: &[Row], evidence: &Evidence) -> ApprovalsVerified {
    let limited = rows.iter().any(|r| matches!(r.verdict, RowVerdict::Unverified(RowUnverified::RateLimited)))
        || matches!(evidence, Evidence::Failed(RowUnverified::RateLimited));
    match (verification.is_verified(), limited) {
        (_, true) => ApprovalsVerified::RateLimited,
        (true, false) => ApprovalsVerified::Yes,
        (false, false) => ApprovalsVerified::No,
    }
}

fn footer(verification: &Verification, rows: &[Row], g: Option<&Governance>, evidence: &Evidence, now: u64) -> Footer {
    Footer {
        approvals: approvals_verified(verification, rows, evidence),
        branch_requires_review: g.map_or(Known::Unknown, |g| g.branch_requires_review),
        gate_required: g.map_or(Known::Unknown, |g| g.gate_required),
        codeowners: g.map_or(Known::Unknown, |g| g.codeowners),
        scheduled_evidence_age: scheduled_evidence_age(evidence, now),
    }
}

fn verified_source(source: Source) -> Verification {
    match source {
        Source::Live => Verification::Verified { source: VerifiedSource::Live },
        Source::Fixture => Verification::Verified { source: VerifiedSource::Fixture },
        Source::Offline { why: OfflineWhy::NoRemote, cache_age } => {
            Verification::Unverified(UnverifiedReason::NoRemote { cache_age })
        }
        Source::Offline { why: OfflineWhy::NoToken, cache_age } => {
            Verification::Unverified(UnverifiedReason::NoToken { cache_age })
        }
    }
}

fn config_in_force(g: &Governance, candidate_missing: bool) -> (ConfigInForce, bool) {
    match (g.config, candidate_missing) {
        (_, true) => (ConfigInForce::Defaults(DefaultsWhy::Missing), false),
        (Approval::Approved { .. }, false) => (ConfigInForce::Approved, true),
        (Approval::Unreviewed(_), false) => (ConfigInForce::Defaults(DefaultsWhy::Unapproved), false),
    }
}

/// A report whose governance facts could not be read.
fn blocked_report(
    inp: &Inputs<'_>,
    mode: Mode,
    default_head: Option<Oid>,
    blocker: RowUnverified,
    affected: &BTreeMap<String, bool>,
    results: Option<&BTreeMap<PropertyId, RunResultRecord>>,
) -> Result<Report, CliError> {
    let verification = Verification::Unverified(unverified_reason(inp.backend.source, &blocker));
    let rows = git_only_rows(inp, &blocker, affected, results)?;
    let evidence = Evidence::Failed(blocker);
    let footer = footer(&verification, &rows, None, &evidence, inp.ws.now);
    Ok(Report {
        mode,
        verification,
        default_branch: inp.default_branch.to_string(),
        default_head,
        config: ConfigInForce::Unknown,
        amendments: Vec::new(),
        governance: None,
        rows,
        if_merged_now: Vec::new(),
        pull_request: None,
        footer,
        notes: Vec::new(),
        calls_spent: inp.backend.budget.spent(),
        uncovered: None,
    })
}

/// `status`, or `check` when `chk` is given.
pub fn run(inp: &Inputs<'_>, chk: Option<&CheckInputs<'_>>) -> Result<Report, CliError> {
    let repo = inp.backend.repo;
    let default_head = repo.head(inp.default_ref).map_err(|e| {
        CliError::Usage(format!("cannot resolve the default branch at {}: {e}; pass --base", inp.default_ref))
    })?;
    let mode = match chk {
        Some(c) => Mode::Check { base: c.base.to_string(), head: c.head.to_string() },
        None => Mode::Status,
    };
    let empty_governed = proob_effects::facts::GovernedSet::default();
    let governed = inp.backend.bindings.map_or(&empty_governed, |b| &b.governed);
    let ctx0 = Context {
        api: inp.backend.api,
        repo,
        default_branch: inp.default_branch,
        default_ref: inp.default_ref,
        governed,
        allow_self_signed: inp.candidate.allow_self_signed,
        now: inp.ws.now,
        history_depth: HISTORY_DEPTH,
    };
    let (changed, head_sha) = match chk {
        Some(c) => (
            repo.changed_paths(c.base, c.head).map_err(CliError::failed)?,
            Some(repo.head(c.head).map_err(CliError::failed)?),
        ),
        None => (Vec::new(), None),
    };
    let affected_by_path = match chk {
        Some(_) => affected_set(inp, &changed, inp.candidate.waiver_max_days)?,
        None => BTreeMap::new(),
    };
    let results = chk.map(|c| c.results);

    let g = match governance_facts(&ctx0, inp.candidate, GATE_CHECK_NAME) {
        Ok(g) => g,
        Err(e) => {
            return match classify(e) {
                Blocker::Row(u) => blocked_report(inp, mode, Some(default_head), u, &affected_by_path, results),
                Blocker::Fatal(e) => Err(e),
            };
        }
    };
    let (config, approved) = config_in_force(&g, inp.candidate_missing);
    let governing = if approved { inp.candidate.clone() } else { Config::defaults() };
    let gate = convert::gate_config(&governing).map_err(|e| CliError::invalid(CONFIG, e))?;
    let ctx = Context { allow_self_signed: gate.allow_self_signed, ..ctx0 };

    let wanted: Vec<(PropertyId, Kind)> = inp.properties.iter().map(|p| (p.id, kind_of(p))).collect();
    let evidence = retrieve_evidence(&ctx, &g, &gate, &wanted)?;
    let mut notes = Vec::new();
    if matches!(evidence, Evidence::NoWorkflow) {
        notes.push(format!(
            "no evidence workflow on {}: nothing can be Checked until one is approved there",
            inp.default_branch
        ));
    }

    let verification = verified_source(inp.backend.source);
    let derivation = Derivation {
        ctx: &ctx,
        g: &g,
        gate: &gate,
        waiver_max_days: governing.waiver_max_days,
        evidence: &evidence,
        default_head,
        bindings: inp.backend.bindings,
        verified: verification.is_verified(),
    };
    // Affected properties first, so a spent budget lands on the rest.
    let (first, rest): (Vec<&LoadedProperty>, Vec<&LoadedProperty>) =
        inp.properties.iter().partition(|p| affected_by_path.get(&p.path) == Some(&true));
    let mut rows = Vec::with_capacity(inp.properties.len());
    for p in first.into_iter().chain(rest) {
        let affected = chk.map(|_| affected_by_path.get(&p.path) == Some(&true));
        rows.push(derivation.derive_row(p, affected, results.and_then(|r| r.get(&p.id)))?);
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));

    let (if_merged_now, pull_request) = match (chk, head_sha) {
        (Some(c), Some(head)) => if_merged_rows(inp, &ctx, c, &changed, &head, gate.allow_self_signed, &mut notes)?,
        _ => (Vec::new(), None),
    };
    let footer = footer(&verification, &rows, Some(&g), &evidence, inp.ws.now);
    Ok(Report {
        mode,
        verification,
        default_branch: inp.default_branch.to_string(),
        default_head: Some(default_head),
        config,
        amendments: governance_verdicts(&g),
        governance: Some(g),
        rows,
        if_merged_now,
        pull_request,
        footer,
        notes,
        calls_spent: inp.backend.budget.spent(),
        uncovered: None,
    })
}

/// The bootstrap report: the base ref has no `proob/config.toml`, so nothing
/// is governed yet and every property is a proposal.
pub fn bootstrap(inp: &Inputs<'_>, base: &str, head: &str) -> Report {
    let rows: Vec<Row> = inp.properties.iter().map(|p| row(p, RowVerdict::Proposal, Some(false), None)).collect();
    Report {
        mode: Mode::Bootstrap { base: base.to_string(), head: head.to_string() },
        verification: Verification::NotConsulted,
        default_branch: inp.default_branch.to_string(),
        default_head: None,
        config: ConfigInForce::Defaults(DefaultsWhy::Bootstrap),
        amendments: Vec::new(),
        governance: None,
        rows,
        if_merged_now: Vec::new(),
        pull_request: None,
        footer: Footer {
            approvals: ApprovalsVerified::No,
            branch_requires_review: Known::Unknown,
            gate_required: Known::Unknown,
            codeowners: Known::Unknown,
            scheduled_evidence_age: None,
        },
        notes: vec![format!("{base} has no {CONFIG}: the gate is not in force yet")],
        calls_spent: 0,
        uncovered: None,
    }
}
