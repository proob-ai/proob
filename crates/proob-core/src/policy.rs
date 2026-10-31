//! The gate policy: a total table from the verdict to `Pass`, `Warn`, or
//! `Block`, read from an approved `proob/config.toml` or the built-in defaults.
//!
//! R10 fixes what the table may not say: a PR is blocked only by a `Broken`
//! property it affects. `policy` enforces that whatever the table holds: every
//! row but `broken_affected` is capped at `Warn`, so a table that says `block`
//! elsewhere warns instead (the facts crate also refuses such a table at parse
//! time, so the cap is a proof obligation, not a path a user sees).
//!
//! `Checked` splits in two rows: `tested` for a `Checked` whose only modifier is
//! `tested`, `checked` for one carrying a defect (`strength_pending` or
//! `self_signed`).
//!
//! `grace_of` decides the grace window: an unchecked property is silent while
//! `now < approved_at + grace_secs`, strictly, so at exactly the window's
//! length it is after.
use crate::types::*;
use vstd::prelude::*;

verus! {

/// `Block` becomes `Warn`; `Pass` and `Warn` stay.
pub open spec fn capped(o: Outcome) -> Outcome {
    match o {
        Outcome::Block => Outcome::Warn,
        Outcome::Pass => Outcome::Pass,
        Outcome::Warn => Outcome::Warn,
    }
}

/// Computes `capped`.
fn cap(o: Outcome) -> (r: Outcome)
    ensures
        r == capped(o),
{
    match o {
        Outcome::Block => Outcome::Warn,
        Outcome::Pass => Outcome::Pass,
        Outcome::Warn => Outcome::Warn,
    }
}

/// `Checked` whose only modifier is `tested`: a test-backed property with no
/// defect pending. Any other `Checked` carries a defect and takes the `checked` row.
pub open spec fn tested_only(v: Verdict) -> bool {
    v matches Verdict::Checked { tested: true, strength_pending: false, self_signed: false }
}

/// The gate's outcome for one verdict.
// proob:01M622RQ781KT06FB0MKKCZNPF
pub fn policy(v: &Verdict, affected: bool, grace: Grace, p: &Policy) -> (o: Outcome)
    ensures
        o is Block ==> v is Broken && affected,
        v is Broken && affected ==> o == p.broken_affected,
        !affected ==> o != Outcome::Block,
        !(v is Broken) ==> o != Outcome::Block,
        v is Holds ==> o == capped(p.holds),
        tested_only(*v) ==> o == capped(p.tested),
        v is Checked && !tested_only(*v) ==> o == capped(p.checked),
        v is Proposed ==> o == capped(p.proposed),
        v is Waived ==> o == capped(p.waived),
        v is NotChecked && grace is Within ==> o == capped(p.not_checked_within_grace),
        v is NotChecked && grace is After ==> o == capped(p.not_checked_after_grace),
        v is ReApprove ==> o == capped(p.re_approve),
        v is Broken && !affected ==> o == capped(p.broken_unaffected),
{
    match *v {
        Verdict::Holds => cap(p.holds),
        Verdict::Checked { tested: true, strength_pending: false, self_signed: false } => cap(p.tested),
        Verdict::Checked { .. } => cap(p.checked),
        Verdict::Proposed => cap(p.proposed),
        Verdict::Waived { .. } => cap(p.waived),
        Verdict::NotChecked { .. } => match grace {
            Grace::Within => cap(p.not_checked_within_grace),
            Grace::After => cap(p.not_checked_after_grace),
        },
        Verdict::ReApprove(_) => cap(p.re_approve),
        Verdict::Broken(_) => if affected { p.broken_affected } else { cap(p.broken_unaffected) },
    }
}

/// Whether `now` is inside the grace window that starts at `approved_at`.
pub fn grace_of(approved_at: Secs, now: Secs, grace_secs: Secs) -> (g: Grace)
    ensures
        g is Within <==> now < approved_at + grace_secs,
{
    // `||` short-circuits, so the subtraction runs only when it cannot underflow.
    if now < approved_at || now - approved_at < grace_secs {
        Grace::Within
    } else {
        Grace::After
    }
}

} // verus!
