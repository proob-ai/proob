//! The one constructor of `Governance`.
//!
//! `Governance` is plain data, but `verdict` relies on `floor >= 1`
//! (`specs::gate_config_wf`, carried into `specs::score_at_floor`). The facts
//! crate already refuses a zero `kill_floor`; this constructor refuses it
//! again at the last step before the verified functions read it, so the
//! invariant is established where it is used rather than trusted from afar.
//! `None` means, and only means, a zero floor.
use crate::types::*;
use vstd::prelude::*;

verus! {

/// Repository governance from its facts; `None` when `floor` is zero.
pub fn governance(
    config: Approval,
    evidence_workflow: Approval,
    gate_workflow: Approval,
    branch_requires_review: Known,
    gate_required: Known,
    codeowners: Known,
    floor: u32,
) -> (g: Option<Governance>)
    ensures
        g is Some <==> 1 <= floor,
        g matches Some(gv) ==> gv == (Governance {
            config,
            evidence_workflow,
            gate_workflow,
            branch_requires_review,
            gate_required,
            codeowners,
            floor,
        }),
        g matches Some(gv) ==> 1 <= gv.floor,
{
    if floor == 0 {
        None
    } else {
        Some(
            Governance { config, evidence_workflow, gate_workflow, branch_requires_review, gate_required, codeowners, floor },
        )
    }
}

} // verus!
