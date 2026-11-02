//! Gathering the facts for one property into a `DeriveInput`, and the
//! repository's `Governance`. This is where the working tree, git history,
//! GitHub's records, and the evidence set meet; every judgement inside is a
//! verified function (`eligibility`, `paths_distinct_exec`, `governance`),
//! and the result is facts for the verified `derive`.
//!
//! Approval of a governed file or set (`approval_of`): git names the
//! introducing commit on the default branch and the content's blob there;
//! `facts::introduction` assembles the PR's records; `eligibility` judges.
//! `previously_approved` walks back through the earlier first-parent commits
//! that touched the same paths (at most `history_depth`) and is true when
//! any earlier content was approved, team or self-signed. A file absent from
//! the default branch (a local proposal) is unreviewed with no review, and so
//! is a file whose working-tree content differs from the branch's
//! (`tree_approval`): approval is by content, and the tree is what `derive`
//! judges.
//!
//! Drift: the first-parent commits on the default branch after the approving
//! merge that touched any resolved scope file, and the days from that merge
//! to the latest of them.
//!
//! Call order per run, for the budget: the caller derives the affected files
//! first (`statement_approvals`), then the rest; a spent budget leaves a file
//! `Err(RateLimited)`, which renders `Unverified (rate limited)`.
use crate::error::GatherError;
use crate::evidence::EvidenceSet;
use crate::facts::{GovernedSet, IntroducingCommit, introduction, set_oid};
use crate::git::Repo;
use crate::github::GitHubApi;
use proob_core::binding_hash::binding_hash;
use proob_core::eligibility::eligibility;
use proob_core::governance::governance;
use proob_core::scope_hash::{PathDigest, paths_distinct_exec};
use proob_core::types::{
    Approval, BindingFacts, DeriveInput, Drift, Governance, Kind, Known, Oid, Property, RunExpectation, RunResult, Secs,
};
use proob_facts::config::Config;
use proob_facts::convert;
use proob_facts::date::SECS_PER_DAY;
use proob_facts::github::IssueState;
use proob_facts::hex;
use proob_facts::paths::{CONFIG, EVIDENCE_WORKFLOW, GATE_WORKFLOW};
use proob_facts::property::{PropertyFile, file_digest};

/// Everything the gatherers share for one run.
pub struct Context<'a> {
    pub api: &'a dyn GitHubApi,
    pub repo: &'a dyn Repo,
    /// The default branch's name on GitHub, such as `main`.
    pub default_branch: &'a str,
    /// The revision git reads it at, such as `origin/main`.
    pub default_ref: &'a str,
    pub governed: &'a GovernedSet,
    pub allow_self_signed: bool,
    pub now: Secs,
    /// How many earlier introductions to consult for `previously_approved`.
    pub history_depth: usize,
}

/// The approval of one governed file or set, with what came before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileApproval {
    pub approval: Approval,
    pub previously_approved: bool,
    /// The committer time of the approving merge, when approved.
    pub approved_at: Option<Secs>,
}

/// A bound check's binding set: the files whose content gives the check its meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingSet {
    pub files: Vec<String>,
}

/// One property to gather.
pub struct PropertyRequest<'a> {
    /// The property file's root-relative path.
    pub path: &'a str,
    pub file: &'a PropertyFile,
    /// The kind when the file names none: the adapter's.
    pub kind_default: Kind,
    pub adapter_available: bool,
    pub binding: Option<&'a BindingSet>,
    pub provisional: Option<RunResult>,
    pub waiver_max_days: u32,
}

// ---------------------------------------------------------------------------
// Approvals
// ---------------------------------------------------------------------------

/// The approval of `paths` as they are on the default branch now.
pub fn approval_of(ctx: &Context<'_>, paths: &[&str]) -> Result<FileApproval, GatherError> {
    let commits = ctx.repo.touching_commits(ctx.default_ref, paths, ctx.history_depth + 1)?;
    let Some((current, earlier)) = commits.split_first() else {
        return Ok(FileApproval { approval: never_reviewed(), previously_approved: false, approved_at: None });
    };
    let approval = approval_at(ctx, paths, current)?.unwrap_or_else(never_reviewed);
    let approved_at = match approval {
        Approval::Approved { merge_sha, .. } => Some(ctx.repo.commit_time(&merge_sha)?),
        Approval::Unreviewed(_) => None,
    };
    let previously_approved = earlier
        .iter()
        .map(|sha| approval_at(ctx, paths, sha))
        .find_map(|r| match r {
            Ok(Some(Approval::Approved { .. })) => Some(Ok(true)),
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        })
        .transpose()?
        .unwrap_or(false);
    Ok(FileApproval { approval, previously_approved, approved_at })
}

fn never_reviewed() -> Approval {
    Approval::Unreviewed(proob_core::types::UnreviewedReason::NoEligibleReview(proob_core::types::Ineligible::NoReview))
}

/// The approval of `paths` for the content in the working tree. Approval is
/// by content: when every path's normalised content equals the default
/// branch's, the branch's approval is the tree's; otherwise the tree holds an
/// amendment (or a new file) nobody has reviewed, and `previously_approved`
/// says whether the branch's content, or one before it, was approved. A PR
/// that edits a property file or a binding-set member therefore reads
/// `Re-approve (statement amended)` or `Re-approve (check amended)` until the
/// amendment itself merges with an eligible review.
pub fn tree_approval(ctx: &Context<'_>, paths: &[&str]) -> Result<FileApproval, GatherError> {
    let on_branch = approval_of(ctx, paths)?;
    if tree_matches_branch(ctx, paths)? {
        return Ok(on_branch);
    }
    let was_approved = matches!(on_branch.approval, Approval::Approved { .. }) || on_branch.previously_approved;
    Ok(FileApproval { approval: never_reviewed(), previously_approved: was_approved, approved_at: None })
}

/// Every path's normalised content is the same in the tree and at the default ref.
fn tree_matches_branch(ctx: &Context<'_>, paths: &[&str]) -> Result<bool, GatherError> {
    for path in paths {
        let in_tree = ctx.repo.read_file(path)?.map(|b| file_digest(&b));
        let on_branch = ctx.repo.file_at(ctx.default_ref, path)?.map(|b| file_digest(&b));
        if in_tree != on_branch {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The approval of the content `paths` had at `commit`; `None` when the set
/// did not fully exist there (a deletion, or a member added later).
fn approval_at(ctx: &Context<'_>, paths: &[&str], commit: &Oid) -> Result<Option<Approval>, GatherError> {
    let rev = hex::encode_oid(commit);
    let mut pairs = Vec::with_capacity(paths.len());
    for path in paths {
        match ctx.repo.blob_at(&rev, path)? {
            Some(blob) => pairs.push((path.to_string(), blob)),
            None => return Ok(None),
        }
    }
    let blob_at_merge = set_oid(&pairs).ok_or_else(|| GatherError::DuplicatePath { path: first_duplicate(paths) })?;
    let intro = introduction(
        ctx.api,
        ctx.governed,
        ctx.default_branch,
        paths,
        &IntroducingCommit { sha: *commit, blob_at_merge },
    )?;
    Ok(Some(eligibility(&intro, ctx.allow_self_signed)))
}

fn first_duplicate(paths: &[&str]) -> String {
    let mut seen = std::collections::BTreeSet::new();
    paths.iter().find(|p| !seen.insert(**p)).map_or_else(String::new, |p| p.to_string())
}

/// Approvals of many governed files, affected ones first so the budget is
/// spent where this PR needs it. Each file's result stands alone: a spent
/// budget is `Err` for the files after it, never a panic and never a verdict.
pub fn statement_approvals(
    ctx: &Context<'_>,
    paths: &[String],
    affected: &[String],
) -> Vec<(String, Result<FileApproval, GatherError>)> {
    let (first, rest): (Vec<&String>, Vec<&String>) = paths.iter().partition(|p| affected.contains(p));
    first.into_iter().chain(rest).map(|p| (p.clone(), approval_of(ctx, &[p.as_str()]))).collect()
}

// ---------------------------------------------------------------------------
// Scope and binding
// ---------------------------------------------------------------------------

/// The tracked files a property's globs match, with their content digests,
/// in `git ls-files` order. Globs passed the verified grammar at parse time.
pub fn resolve_scope(repo: &dyn Repo, globs: &[Vec<u8>]) -> Result<Vec<PathDigest>, GatherError> {
    let set = glob_set(globs)?;
    let files: Vec<String> = repo.ls_files()?.into_iter().filter(|f| set.is_match(f)).collect();
    digests_of(repo, &files)
}

pub use globset::GlobSet;

/// Scope globs compiled as `resolve_scope` matches them: `*` stops at `/`.
pub fn glob_set(globs: &[Vec<u8>]) -> Result<GlobSet, GatherError> {
    let mut builder = globset::GlobSetBuilder::new();
    for glob in globs {
        let pattern = String::from_utf8_lossy(glob).into_owned();
        let compiled = globset::GlobBuilder::new(&pattern)
            .literal_separator(true)
            .build()
            .map_err(|e| GatherError::Glob { pattern: pattern.clone(), message: e.to_string() })?;
        builder.add(compiled);
    }
    builder.build().map_err(|e| GatherError::Glob { pattern: String::new(), message: e.to_string() })
}

/// (path, digest) pairs for files of the working tree; a listed file that is
/// missing from the tree (deleted but still tracked) is left out.
pub fn digests_of(repo: &dyn Repo, files: &[String]) -> Result<Vec<PathDigest>, GatherError> {
    let mut pairs = Vec::with_capacity(files.len());
    for path in files {
        if let Some(bytes) = repo.read_file(path)? {
            pairs.push(PathDigest { path: path.as_bytes().to_vec(), digest: file_digest(&bytes) });
        }
    }
    Ok(pairs)
}

fn distinct(pairs: &Vec<PathDigest>) -> Result<(), GatherError> {
    if paths_distinct_exec(pairs) {
        return Ok(());
    }
    let mut seen = std::collections::BTreeSet::new();
    let dup = pairs.iter().find(|p| !seen.insert(&p.path)).map(|p| String::from_utf8_lossy(&p.path).into_owned());
    Err(GatherError::DuplicatePath { path: dup.unwrap_or_default() })
}

fn clone_pairs(pairs: &[PathDigest]) -> Vec<PathDigest> {
    pairs.iter().map(|p| PathDigest { path: p.path.clone(), digest: p.digest }).collect()
}

/// The binding set's facts: its hash, its approval, and what came before.
fn binding_facts(ctx: &Context<'_>, set: &BindingSet) -> Result<BindingFacts, GatherError> {
    let pairs = digests_of(ctx.repo, &set.files)?;
    distinct(&pairs)?;
    let binding_hash = binding_hash(pairs).ok_or_else(|| GatherError::DuplicatePath { path: String::new() })?;
    let paths: Vec<&str> = set.files.iter().map(String::as_str).collect();
    let approval = tree_approval(ctx, &paths)?;
    Ok(BindingFacts { binding_hash, approval: approval.approval, previously_approved: approval.previously_approved })
}

// ---------------------------------------------------------------------------
// Drift and waivers
// ---------------------------------------------------------------------------

/// Commits on the default branch after `since` touching any of `paths`, and
/// the days from `since` to the latest of them.
pub fn drift_since(ctx: &Context<'_>, since: &Oid, paths: &[&str]) -> Result<Drift, GatherError> {
    let touching = ctx.repo.commits_since(since, ctx.default_ref, paths)?;
    let commits = touching.len() as u64;
    let days = match touching.iter().map(|(_, t)| *t).max() {
        Some(latest) => latest.saturating_sub(ctx.repo.commit_time(since)?) / SECS_PER_DAY,
        None => 0,
    };
    Ok(Drift { commits, days })
}

/// Pure: drift since the later of two approvals. On the default branch's
/// linear history the commits after the later merge are a subset of those
/// after the earlier one, so the later approval is the one with fewer.
pub fn latest_drift(statement: Option<Drift>, spec: Option<Drift>) -> Drift {
    [statement, spec].into_iter().flatten().min_by_key(|d| d.commits).unwrap_or(Drift { commits: 0, days: 0 })
}

/// The issue number in `#123`, `123`, or an issue URL.
pub fn issue_number(text: &str) -> Option<u64> {
    let tail = text.trim().rsplit(['/', '#']).next()?;
    tail.parse().ok()
}

fn waiver_issue_open(ctx: &Context<'_>, path: &str, issue: &str) -> Result<bool, GatherError> {
    let number = issue_number(issue)
        .ok_or_else(|| GatherError::WaiverIssue { path: path.to_string(), issue: issue.to_string() })?;
    Ok(ctx.api.issue(number)?.state == IssueState::Open)
}

// ---------------------------------------------------------------------------
// gather
// ---------------------------------------------------------------------------

/// The facts `derive` reads about one property.
pub fn gather(
    ctx: &Context<'_>,
    req: &PropertyRequest<'_>,
    evidence: &EvidenceSet,
    default_head: Oid,
) -> Result<DeriveInput, GatherError> {
    let statement = tree_approval(ctx, &[req.path])?;
    let property: Property =
        convert::property(req.file, statement.approved_at.unwrap_or(ctx.now), req.waiver_max_days)?;
    let kind = property.kind.unwrap_or(req.kind_default);

    let scope = resolve_scope(ctx.repo, &property.scope)?;
    distinct(&scope)?;
    let scope_paths: Vec<String> = scope.iter().map(|p| String::from_utf8_lossy(&p.path).into_owned()).collect();
    let scope_refs: Vec<&str> = scope_paths.iter().map(String::as_str).collect();

    let binding = match (req.adapter_available, req.binding) {
        (true, Some(set)) => Some(binding_facts(ctx, set)?),
        _ => None,
    };
    let since = |approval: &Approval| match approval {
        Approval::Approved { merge_sha, .. } if !scope_refs.is_empty() => {
            drift_since(ctx, merge_sha, &scope_refs).map(Some)
        }
        _ => Ok(None),
    };
    let statement_drift = since(&statement.approval)?;
    // A model's spec is the design: re-approving it re-affirms the design against the code.
    let spec_drift = match (kind, &binding) {
        (Kind::Model, Some(b)) => since(&b.approval)?,
        _ => None,
    };
    let drift = latest_drift(statement_drift, spec_drift);
    let waiver_issue_open = match &req.file.waiver {
        Some(w) => waiver_issue_open(ctx, req.path, &w.issue)?,
        None => false,
    };
    let records = evidence.records_for(property.id);
    Ok(DeriveInput {
        property,
        kind,
        scope: clone_pairs(&scope),
        statement: statement.approval,
        previously_approved: statement.previously_approved,
        drift,
        waiver_issue_open,
        binding,
        adapter_available: req.adapter_available,
        records,
        evidence_run_head: evidence.latest_run_head,
        default_head,
        provisional: req.provisional,
    })
}

// ---------------------------------------------------------------------------
// Governance
// ---------------------------------------------------------------------------

/// Repository governance: the config's and the two workflows' approvals
/// (the config judged with `allow_self_signed` from the candidate config
/// itself), whether the default branch requires review, whether the gate is
/// a required check, and whether CODEOWNERS exists. GitHub facts it cannot
/// read are `Unknown`, a standing warning.
pub fn governance_facts(
    ctx: &Context<'_>,
    candidate: &Config,
    gate_check_name: &str,
) -> Result<Governance, GatherError> {
    let config_ctx = Context { allow_self_signed: candidate.allow_self_signed, ..*ctx };
    let config = approval_of(&config_ctx, &[CONFIG])?.approval;
    let evidence_workflow = approval_of(ctx, &[EVIDENCE_WORKFLOW])?.approval;
    let gate_workflow = approval_of(ctx, &[GATE_WORKFLOW])?.approval;
    let (branch_requires_review, gate_required) = branch_facts(ctx, gate_check_name);
    let codeowners = match ctx.api.codeowners_present(ctx.default_branch) {
        Ok(true) => Known::Yes,
        Ok(false) => Known::No,
        Err(_) => Known::Unknown,
    };
    governance(
        config,
        evidence_workflow,
        gate_workflow,
        branch_requires_review,
        gate_required,
        codeowners,
        candidate.kill_floor,
    )
    .ok_or(GatherError::FloorZero)
}

/// From the branch's `protected` flag and its rules: a `pull_request` rule
/// means review is required; a `required_status_checks` rule naming the gate
/// means the gate is required. A protected branch whose rules say neither
/// may carry classic protection the default token cannot read: `Unknown`.
fn branch_facts(ctx: &Context<'_>, gate_check_name: &str) -> (Known, Known) {
    let protected = ctx.api.branch(ctx.default_branch).map(|b| b.protected);
    let rules = ctx.api.branch_rules(ctx.default_branch);
    let (Ok(protected), Ok(rules)) = (protected, rules) else { return (Known::Unknown, Known::Unknown) };
    let review = rules.iter().any(|r| r.kind == "pull_request");
    let gate = rules.iter().any(|r| {
        r.kind == "required_status_checks"
            && r.parameters
                .as_ref()
                .is_some_and(|p| p.required_status_checks.iter().any(|c| c.context == gate_check_name))
    });
    let known = |found: bool| {
        if found {
            Known::Yes
        } else if protected {
            Known::Unknown
        } else {
            Known::No
        }
    };
    (known(review), known(gate))
}

/// What an accepted evidence run must match: the evidence workflow's blob on
/// the default branch and its approval from `governance`.
pub fn evidence_expectation(ctx: &Context<'_>, g: &Governance) -> Result<RunExpectation, GatherError> {
    let workflow_blob = ctx.repo.blob_at(ctx.default_ref, EVIDENCE_WORKFLOW)?.ok_or_else(|| {
        GatherError::MissingBlob { rev: ctx.default_ref.to_string(), path: EVIDENCE_WORKFLOW.to_string() }
    })?;
    Ok(RunExpectation {
        workflow_path: EVIDENCE_WORKFLOW.as_bytes().to_vec(),
        default_branch: ctx.default_branch.as_bytes().to_vec(),
        workflow_blob,
        workflow_approval: g.evidence_workflow,
    })
}
