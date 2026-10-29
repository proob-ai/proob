//! Evidence acceptance: which workflow runs may supply `Evidence`.
//!
//! A record becomes `Evidence` (types.rs) only from an artifact of a run that
//! `accept_run` accepts: the evidence workflow's path, the default branch, an
//! event that is not a pull request (`Push`, `Schedule`, `WorkflowDispatch`),
//! a successful conclusion, and the workflow blob that is approved on the
//! default branch now. The last condition is what stops a PR that edits the
//! evidence workflow from manufacturing evidence: its runs execute a blob
//! that is not the approved one, and after it merges the workflow's approval
//! is re-derived like any governed file's.
//!
//! `spec_accept` is the definition; `accept_run` is proven to decide it. The
//! effects layer calls `accept_run` on every candidate run before reading its
//! artifact and never constructs `Evidence` from a run it rejected.
use crate::bytes::{bytes_eq, digest_eq};
use crate::types::*;
use vstd::prelude::*;

verus! {

/// Events whose runs may produce evidence.
pub open spec fn evidence_event(e: RunEvent) -> bool {
    e is Push || e is Schedule || e is WorkflowDispatch
}

/// The acceptance predicate.
pub open spec fn spec_accept(run: RunFacts, expected: RunExpectation) -> bool {
    &&& run.workflow_path@ == expected.workflow_path@
    &&& run.head_branch matches Some(b) && b@ == expected.default_branch@
    &&& evidence_event(run.event)
    &&& run.conclusion is Success
    &&& expected.workflow_approval is Approved
    &&& run.workflow_blob == Some(expected.workflow_blob)
}

/// Decides `evidence_event`.
fn is_evidence_event(e: RunEvent) -> (r: bool)
    ensures
        r == evidence_event(e),
{
    match e {
        RunEvent::Push => true,
        RunEvent::Schedule => true,
        RunEvent::WorkflowDispatch => true,
        RunEvent::PullRequest => false,
        RunEvent::Other => false,
    }
}

/// Decides `run.head_branch matches Some(b) && b@ == expected.default_branch@`.
fn on_default_branch(run: &RunFacts, expected: &RunExpectation) -> (r: bool)
    ensures
        r == (run.head_branch matches Some(b) && b@ == expected.default_branch@),
{
    match &run.head_branch {
        Some(b) => bytes_eq(b.as_slice(), expected.default_branch.as_slice()),
        None => false,
    }
}

/// Decides `run.workflow_blob == Some(expected.workflow_blob)`.
fn approved_blob(run: &RunFacts, expected: &RunExpectation) -> (r: bool)
    ensures
        r == (run.workflow_blob == Some(expected.workflow_blob)),
{
    match run.workflow_blob {
        Some(blob) => digest_eq(&blob, &expected.workflow_blob),
        None => false,
    }
}

/// Decides `spec_accept`.
// proob:01M622RQ791MXW80SMN13TWVSY
pub fn accept_run(run: &RunFacts, expected: &RunExpectation) -> (r: bool)
    ensures
        r <==> spec_accept(*run, *expected),
{
    bytes_eq(run.workflow_path.as_slice(), expected.workflow_path.as_slice()) && on_default_branch(run, expected)
        && is_evidence_event(run.event) && run.conclusion == RunConclusion::Success
        && matches!(expected.workflow_approval, Approval::Approved { .. }) && approved_blob(run, expected)
}

} // verus!
