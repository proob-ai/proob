//! Spec validation for U3: property-based checks that the verified functions
//! mean what the plan's prose says. Proofs establish each function against
//! its `ensures`; these tests catch an `ensures` that says the wrong thing,
//! and witness totality at runtime.
mod common;

use common::*;
use proob_core::affected::affected;
use proob_core::derive::derive;
use proob_core::policy::{grace_of, policy};
use proob_core::scope_hash::{PathDigest, ScopeHash, is_strictly_sorted, scope_hash, sort_by_path};
use proob_core::types::*;
use proob_core::verdict::{governance_verdicts, verdict};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn is_team(a: Approval) -> bool {
    matches!(a, Approval::Approved { sig: Sig::Team, .. })
}

fn binding_approval(s: &Status) -> Option<Approval> {
    match s {
        Status::Scoped { binding: Binding::Bound { approval, .. }, .. } => Some(*approval),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Holds is unreachable without every approval (assertions 1 and 2)
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn statement_not_team_approved_never_holds(s in status(), g in governance()) {
        let decl = match s { Status::ScopeEmpty { decl } | Status::Scoped { decl, .. } => decl };
        prop_assume!(!matches!(decl, Decl::Approved { sig: Sig::Team, .. }));
        prop_assert_ne!(verdict(&s, &g), Verdict::Holds);
        if let Status::Scoped { kind, .. } = s {
            prop_assert_ne!(verdict(&s, &g), top(kind));
        }
    }

    #[test]
    fn unapproved_binding_config_or_workflow_never_holds(s in status(), g in governance()) {
        let binding_team = binding_approval(&s).is_some_and(is_team);
        prop_assume!(!binding_team || !is_team(g.config) || !is_team(g.evidence_workflow));
        prop_assert_ne!(verdict(&s, &g), Verdict::Holds);
        if let Status::Scoped { kind, .. } = s {
            prop_assert_ne!(verdict(&s, &g), top(kind));
        }
    }

    /// The prose of assertions 1 to 4, written independently of `specs.rs`:
    /// what `Holds` needs, and what a test's top verdict needs but the kind.
    #[test]
    fn holds_means_what_the_prose_says(s in status(), g in governance()) {
        let v = verdict(&s, &g);
        let at_top = matches!(s, Status::Scoped { kind, .. } if v == top(kind));
        if v == Verdict::Holds || at_top {
            let Status::Scoped { kind, decl, binding, .. } = s else { return Err(TestCaseError::fail("Holds on ScopeEmpty")) };
            prop_assert!(v != Verdict::Holds || matches!(kind, Kind::Proof | Kind::Model), "a test samples its inputs, so it never holds: {:?}", kind);
            prop_assert!(matches!(decl, Decl::Approved { sig: Sig::Team, .. }), "statement team-approved: {:?}", decl);
            let Binding::Bound { approval, primary, score, .. } = binding else { return Err(TestCaseError::fail("Holds unbound")) };
            prop_assert!(is_team(approval) && is_team(g.config) && is_team(g.evidence_workflow));
            prop_assert_eq!(primary, Primary::Fresh(positive_for(kind)));
            prop_assert!(
                matches!(score, Score::Fresh(ScoreOutcome::Scored { killed, .. }) if g.floor >= 1 && killed >= g.floor),
                "fresh score at the floor: {:?} vs floor {}",
                score,
                g.floor
            );
            if kind == Kind::Model {
                prop_assert!(matches!(decl, Decl::Approved { drift: Drift { commits: 0, .. }, .. }), "model undrifted: {:?}", decl);
            }
        }
    }

    #[test]
    fn checked_always_carries_a_modifier(s in status(), g in governance()) {
        if let Verdict::Checked { tested, strength_pending, self_signed } = verdict(&s, &g) {
            prop_assert!(tested || strength_pending || self_signed);
            let Status::Scoped { kind, .. } = s else { return Err(TestCaseError::fail("Checked on ScopeEmpty")) };
            prop_assert_eq!(tested, kind == Kind::Test, "tested is exactly the test kind");
        }
    }

    /// Assertion 3's kind conjunct: the same facts hold for a proof or a model
    /// and read `Checked (tested)` for a test.
    #[test]
    fn holds_ready_holds_iff_the_kind_is_not_test(kind in kind()) {
        let v = verdict(&holds_ready(kind), &approved_governance());
        prop_assert_eq!(v == Verdict::Holds, kind != Kind::Test);
        prop_assert_eq!(v, top(kind));
    }

    #[test]
    fn waived_means_team_waiver_active_with_open_issue(s in status(), g in governance()) {
        if let Verdict::Waived { until } = verdict(&s, &g) {
            let decl = match s { Status::ScopeEmpty { decl } | Status::Scoped { decl, .. } => decl };
            prop_assert_eq!(
                decl,
                Decl::Waived { sig: Sig::Team, since: match decl { Decl::Waived { since, .. } => since, _ => [0; 32] }, window: Window::Active { until, issue_open: true } }
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Strength and freshness (assertions 3 and 4)
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn weak_or_old_strength_demotes_holds_to_checked_strength_pending(kind in kind(), weak in prop_oneof![
        Just(Score::Fresh(ScoreOutcome::NoMutants)),
        Just(Score::Fresh(ScoreOutcome::Scored { killed: 0, survived: 3, timeout: 0, unviable: 0 })),
        Just(Score::Fresh(ScoreOutcome::Errored)),
        Just(Score::Stale),
        Just(Score::NoEvidence),
    ]) {
        let g = approved_governance();
        prop_assert_eq!(verdict(&holds_ready(kind), &g), top(kind));
        let weakened = with_binding(holds_ready(kind), |b| set_score(b, weak));
        prop_assert_eq!(verdict(&weakened, &g), Verdict::Checked { tested: kind == Kind::Test, strength_pending: true, self_signed: false });
    }

    #[test]
    fn stale_or_pending_primary_is_not_checked(kind in kind()) {
        let g = approved_governance();
        let stale = with_binding(holds_ready(kind), |b| set_primary(b, Primary::Stale));
        prop_assert_eq!(verdict(&stale, &g), Verdict::NotChecked { reason: NotCheckedWhy::EvidenceStale, self_signed: false });
        let pending = with_binding(holds_ready(kind), |b| set_primary(b, Primary::Pending));
        prop_assert_eq!(verdict(&pending, &g), Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: false });
        let none = with_binding(holds_ready(kind), |b| set_primary(b, Primary::NoEvidence));
        prop_assert_eq!(verdict(&none, &g), Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: false });
        let errored = with_binding(holds_ready(kind), |b| set_primary(b, Primary::Fresh(PrimaryOutcome::Errored)));
        prop_assert_eq!(verdict(&errored, &g), Verdict::NotChecked { reason: NotCheckedWhy::AdapterErrored, self_signed: false });
    }

    #[test]
    fn a_default_branch_negative_is_broken_unless_this_pr_fixes_it(kind in kind()) {
        let g = approved_governance();
        let failing = with_binding(holds_ready(kind), |b| set_primary(b, Primary::Fresh(negative_for(kind))));
        prop_assert_eq!(verdict(&failing, &g), Verdict::Broken(BrokenWhy::Negative(Source::DefaultBranch)));
        let fixed = with_binding(failing, |b| set_provisional(b, Provisional::Provisional(positive_for(kind))));
        prop_assert_eq!(verdict(&fixed, &g), Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: false });
    }
}

// ---------------------------------------------------------------------------
// This PR's run (provisional)
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn provisional_results_block_only_when_negative_or_errored(kind in kind(), s in status(), g in governance()) {
        let g_ok = approved_governance();
        let negative = with_binding(holds_ready(kind), |b| set_provisional(b, Provisional::Provisional(negative_for(kind))));
        prop_assert_eq!(verdict(&negative, &g_ok), Verdict::Broken(BrokenWhy::Negative(Source::ThisPr)));
        let errored = with_binding(holds_ready(kind), |b| set_provisional(b, Provisional::Provisional(PrimaryOutcome::Errored)));
        prop_assert_eq!(verdict(&errored, &g_ok), Verdict::Broken(BrokenWhy::RunnerErrored));
        // A positive provisional changes nothing on its own: without fresh primary
        // evidence the property is still not checked.
        let only_provisional = with_binding(holds_ready(kind), |b| set_provisional(set_primary(b, Primary::Pending), Provisional::Provisional(positive_for(kind))));
        prop_assert_eq!(verdict(&only_provisional, &g_ok), Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: false });
        // Across random statuses: a positive provisional never makes Holds out of a
        // status that is not Holds without it.
        if let Status::Scoped { kind, .. } = s {
            let without = with_binding(s, |b| set_provisional(b, Provisional::None));
            let with = with_binding(s, |b| set_provisional(b, Provisional::Provisional(positive_for(kind))));
            if verdict(&without, &g) != Verdict::Holds {
                prop_assert_ne!(verdict(&with, &g), Verdict::Holds);
            }
        }
    }

    #[test]
    fn an_active_team_waiver_shields_this_prs_negative(kind in kind()) {
        let g = approved_governance();
        let waived = Decl::Waived { sig: Sig::Team, since: [7; 32], window: Window::Active { until: NOW + DAY, issue_open: true } };
        let s = with_binding(with_decl(holds_ready(kind), waived), |b| set_provisional(b, Provisional::Provisional(negative_for(kind))));
        prop_assert_eq!(verdict(&s, &g), Verdict::Waived { until: NOW + DAY });
        // A self-signed waiver shields nothing.
        let self_waived = Decl::Waived { sig: Sig::SelfSigned, since: [7; 32], window: Window::Active { until: NOW + DAY, issue_open: true } };
        let s = with_decl(s, self_waived);
        prop_assert_eq!(verdict(&s, &g), Verdict::Broken(BrokenWhy::Negative(Source::ThisPr)));
    }
}

// ---------------------------------------------------------------------------
// Self-signed, drift, waivers, amendments, governance
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn self_signed_is_checked_or_not_checked_never_proposed_or_holds(kind in kind(), s in status(), g in governance()) {
        let g_ok = approved_governance();
        let statement = with_decl(holds_ready(kind), Decl::Approved { sig: Sig::SelfSigned, since: [7; 32], drift: Drift { commits: 0, days: 0 } });
        prop_assert_eq!(verdict(&statement, &g_ok), Verdict::Checked { tested: kind == Kind::Test, strength_pending: false, self_signed: true });
        let check = with_binding(holds_ready(kind), |b| set_binding_approval(b, self_signed()));
        prop_assert_eq!(verdict(&check, &g_ok), Verdict::Checked { tested: kind == Kind::Test, strength_pending: false, self_signed: true });
        let unbound = with_binding(statement, |_| Binding::Unbound(UnboundWhy::NoCheck));
        prop_assert_eq!(verdict(&unbound, &g_ok), Verdict::NotChecked { reason: NotCheckedWhy::NoCheck, self_signed: true });
        let config = Governance { config: self_signed(), ..g_ok };
        prop_assert_eq!(verdict(&holds_ready(kind), &config), Verdict::Checked { tested: kind == Kind::Test, strength_pending: false, self_signed: true });
        // Across random statuses.
        let decl = match s { Status::ScopeEmpty { decl } | Status::Scoped { decl, .. } => decl };
        if matches!(decl, Decl::Approved { sig: Sig::SelfSigned, .. } | Decl::Waived { sig: Sig::SelfSigned, .. }) {
            let v = verdict(&s, &g);
            prop_assert_ne!(v, Verdict::Holds);
            prop_assert_ne!(v, Verdict::Proposed);
            if let Status::Scoped { kind, .. } = s {
                prop_assert_ne!(v, top(kind));
            }
        }
    }

    #[test]
    fn a_model_with_scope_commits_since_approval_needs_re_approval(commits in 1u64..5, days in 0u64..30) {
        let g = approved_governance();
        let drifted = with_decl(holds_ready(Kind::Model), Decl::Approved { sig: Sig::Team, since: [7; 32], drift: Drift { commits, days } });
        prop_assert_eq!(verdict(&drifted, &g), Verdict::ReApprove(ReApproveWhy::SpecDrifted(commits)));
        // Drift never changes a test- or proof-backed verdict.
        for kind in [Kind::Test, Kind::Proof] {
            let drifted = with_decl(holds_ready(kind), Decl::Approved { sig: Sig::Team, since: [7; 32], drift: Drift { commits, days } });
            prop_assert_eq!(verdict(&drifted, &g), top(kind));
        }
    }

    #[test]
    fn waivers_render_waived_while_active_and_open_else_broken(kind in kind(), until in any::<Secs>()) {
        let g = approved_governance();
        let active = Decl::Waived { sig: Sig::Team, since: [7; 32], window: Window::Active { until, issue_open: true } };
        prop_assert_eq!(verdict(&with_decl(holds_ready(kind), active), &g), Verdict::Waived { until });
        let closed = Decl::Waived { sig: Sig::Team, since: [7; 32], window: Window::Active { until, issue_open: false } };
        prop_assert_eq!(verdict(&with_decl(holds_ready(kind), closed), &g), Verdict::Broken(BrokenWhy::WaiverExpired));
        let expired = Decl::Waived { sig: Sig::Team, since: [7; 32], window: Window::Expired { until } };
        prop_assert_eq!(verdict(&with_decl(holds_ready(kind), expired), &g), Verdict::Broken(BrokenWhy::WaiverExpired));
        prop_assert_eq!(verdict(&Status::ScopeEmpty { decl: expired }, &g), Verdict::Broken(BrokenWhy::WaiverExpired));
    }

    #[test]
    fn amendments_and_revoked_access_need_re_approval(kind in kind()) {
        let g = approved_governance();
        let amended = with_decl(holds_ready(kind), Decl::Proposed(ProposedWhy::Amended));
        prop_assert_eq!(verdict(&amended, &g), Verdict::ReApprove(ReApproveWhy::StatementAmended));
        let never = with_decl(holds_ready(kind), Decl::Proposed(ProposedWhy::NeverReviewed));
        prop_assert_eq!(verdict(&never, &g), Verdict::Proposed);
        let revoked = with_decl(holds_ready(kind), Decl::Proposed(ProposedWhy::ReviewerAccessRevoked));
        prop_assert_eq!(verdict(&revoked, &g), Verdict::ReApprove(ReApproveWhy::ReviewerAccessRevoked));
        let check_amended = with_binding(holds_ready(kind), |b| set_binding_approval(b, unreviewed()));
        prop_assert_eq!(verdict(&check_amended, &g), Verdict::ReApprove(ReApproveWhy::CheckAmended));
        let check_revoked = with_binding(holds_ready(kind), |b| set_binding_approval(b, Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked)));
        prop_assert_eq!(verdict(&check_revoked, &g), Verdict::ReApprove(ReApproveWhy::ReviewerAccessRevoked));
        let check_never = with_binding(holds_ready(kind), |b| match set_binding_approval(b, unreviewed()) {
            Binding::Bound { binding_hash, approval, primary, provisional, score, .. } => Binding::Bound { binding_hash, approval, previously_approved: false, primary, provisional, score },
            other => other,
        });
        prop_assert_eq!(verdict(&check_never, &g), Verdict::NotChecked { reason: NotCheckedWhy::CheckUnreviewed, self_signed: false });
    }

    #[test]
    fn unreviewed_governance_demotes_holds_and_is_listed_once(kind in kind(), s in status()) {
        let g = approved_governance();
        let config = Governance { config: unreviewed(), ..g };
        prop_assert_eq!(verdict(&holds_ready(kind), &config), Verdict::ReApprove(ReApproveWhy::ConfigAmended));
        prop_assert_eq!(governance_verdicts(&config), vec![ReApproveWhy::ConfigAmended]);
        let workflow = Governance { evidence_workflow: unreviewed(), ..g };
        prop_assert_eq!(verdict(&holds_ready(kind), &workflow), Verdict::ReApprove(ReApproveWhy::WorkflowAmended));
        prop_assert_eq!(governance_verdicts(&workflow), vec![ReApproveWhy::WorkflowAmended]);
        let both = Governance { config: unreviewed(), evidence_workflow: unreviewed(), ..g };
        prop_assert_eq!(governance_verdicts(&both), vec![ReApproveWhy::ConfigAmended, ReApproveWhy::WorkflowAmended]);
        prop_assert_eq!(governance_verdicts(&g), Vec::<ReApproveWhy>::new());
        // A property that is not Holds-ready keeps its own word under amended config.
        let own = verdict(&s, &g);
        if !matches!(own, Verdict::Holds | Verdict::Checked { .. }) {
            prop_assert_eq!(verdict(&s, &config), own);
        }
    }
}

// ---------------------------------------------------------------------------
// Policy and grace
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn scope_empty_blocks_when_affected_and_counts_otherwise(decl in decl()) {
        let g = approved_governance();
        let v = verdict(&Status::ScopeEmpty { decl }, &g);
        if matches!(decl, Decl::Approved { .. }) {
            prop_assert_eq!(v, Verdict::Broken(BrokenWhy::ScopeEmpty));
            prop_assert_eq!(policy(&v, true, Grace::After, &default_policy()), Outcome::Block);
            prop_assert_eq!(policy(&v, false, Grace::After, &default_policy()), Outcome::Pass);
        }
    }

    #[test]
    fn policy_blocks_only_broken_and_affected(v in any_verdict(), affected in any::<bool>(), grace in grace(), p in any_policy()) {
        let o = policy(&v, affected, grace, &p);
        if o == Outcome::Block {
            prop_assert!(matches!(v, Verdict::Broken(_)) && affected);
        }
        if matches!(v, Verdict::Broken(_)) && affected {
            prop_assert_eq!(o, p.broken_affected);
        }
        if !affected {
            prop_assert_ne!(o, Outcome::Block);
        }
    }

    #[test]
    fn default_policy_matches_the_plans_table(v in any_verdict(), affected in any::<bool>(), grace in grace()) {
        let o = policy(&v, affected, grace, &default_policy());
        let expected = match (v, affected, grace) {
            (Verdict::Holds | Verdict::Checked { .. } | Verdict::Proposed | Verdict::Waived { .. }, _, _) => Outcome::Pass,
            (Verdict::NotChecked { .. }, _, Grace::Within) => Outcome::Pass,
            (Verdict::NotChecked { .. }, _, Grace::After) => Outcome::Warn,
            (Verdict::ReApprove(_), _, _) => Outcome::Warn,
            (Verdict::Broken(_), true, _) => Outcome::Block,
            (Verdict::Broken(_), false, _) => Outcome::Pass,
        };
        prop_assert_eq!(o, expected);
    }

    #[test]
    fn grace_window_is_strict_at_its_end(approved_at in 0u64..u64::MAX / 2, grace_secs in 0u64..u64::MAX / 4, offset in 0u64..u64::MAX / 4) {
        let now = approved_at + offset;
        let expected = if offset < grace_secs { Grace::Within } else { Grace::After };
        prop_assert_eq!(grace_of(approved_at, now, grace_secs), expected);
        // Approval in the future is inside the window.
        prop_assert_eq!(grace_of(now + 1, now, grace_secs), Grace::Within);
    }
}

// ---------------------------------------------------------------------------
// derive: scope, hashes, freshness
// ---------------------------------------------------------------------------

fn scoped_hash(s: &Status) -> Option<[u8; 32]> {
    match s {
        Status::Scoped { scope_hash, .. } => Some(*scope_hash),
        Status::ScopeEmpty { .. } => None,
    }
}

fn bound(s: &Status) -> Option<(Primary, Provisional, Score)> {
    match s {
        Status::Scoped { binding: Binding::Bound { primary, provisional, score, .. }, .. } => {
            Some((*primary, *provisional, *score))
        }
        _ => None,
    }
}

proptest! {
    #[test]
    fn derive_is_scope_empty_exactly_when_no_file_matches(kind in kind(), scope in scope()) {
        let s = derive(base_input(kind, clone_scope(&scope)), &gate_config(), NOW);
        prop_assert_eq!(matches!(s, Status::ScopeEmpty { .. }), scope.is_empty());
        if let Status::Scoped { kind: k, .. } = s {
            prop_assert_eq!(k, kind);
        }
    }

    #[test]
    fn scope_hash_is_order_independent_and_digest_sensitive(kind in kind(), scope in scope().prop_filter("non-empty", |s| !s.is_empty()), seed in any::<u64>()) {
        // A deterministic shuffle from the seed.
        let mut shuffled = clone_scope(&scope);
        let n = shuffled.len();
        for i in (1..n).rev() {
            shuffled.swap(i, (seed as usize).wrapping_mul(i + 1) % (i + 1));
        }
        let one = derive(base_input(kind, clone_scope(&scope)), &gate_config(), NOW);
        let other = derive(base_input(kind, shuffled), &gate_config(), NOW);
        prop_assert_eq!(one, other);
        let mut changed = clone_scope(&scope);
        changed[0].digest[0] ^= 1;
        let changed = derive(base_input(kind, changed), &gate_config(), NOW);
        prop_assert_ne!(scoped_hash(&one), scoped_hash(&changed));
    }

    #[test]
    fn primary_is_fresh_only_at_the_current_hashes_and_latest_wins(kind in kind(), scope in scope().prop_filter("non-empty", |s| !s.is_empty()), other_binding in digest(), other_scope in digest()) {
        prop_assume!(other_binding != BINDING_HASH);
        let current = scope_hash_of(&scope);
        prop_assume!(other_scope != current);
        let cfg = gate_config();
        let positive = Measured::Primary(kind, positive_for(kind));
        let negative = Measured::Primary(kind, negative_for(kind));

        // No records, run at head: no evidence; run outstanding: pending.
        let s = derive(base_input(kind, clone_scope(&scope)), &cfg, NOW);
        prop_assert_eq!(bound(&s).map(|b| b.0), Some(Primary::NoEvidence));
        let mut input = base_input(kind, clone_scope(&scope));
        input.evidence_run_head = None;
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.0), Some(Primary::Pending));

        // At the current hashes: fresh, and the latest record decides.
        let mut input = base_input(kind, clone_scope(&scope));
        input.records = vec![record(positive, current, BINDING_HASH, NOW - DAY), record(negative, current, BINDING_HASH, NOW)];
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.0), Some(Primary::Fresh(negative_for(kind))));
        let mut input = base_input(kind, clone_scope(&scope));
        input.records = vec![record(negative, current, BINDING_HASH, NOW - DAY), record(positive, current, BINDING_HASH, NOW)];
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.0), Some(Primary::Fresh(positive_for(kind))));

        // The latest record at other hashes: stale, or pending while a run is out.
        let mut input = base_input(kind, clone_scope(&scope));
        input.records = vec![record(positive, current, BINDING_HASH, NOW - DAY), record(positive, other_scope, BINDING_HASH, NOW)];
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.0), Some(Primary::Stale));
        let mut input = base_input(kind, clone_scope(&scope));
        input.records = vec![record(positive, current, other_binding, NOW)];
        input.evidence_run_head = Some([4; 32]);
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.0), Some(Primary::Pending));

        // Records of another kind or another property never count.
        let other_kind = match kind { Kind::Test => Kind::Proof, Kind::Proof => Kind::Model, Kind::Model => Kind::Test };
        let mut input = base_input(kind, clone_scope(&scope));
        let mut foreign = record(positive, current, BINDING_HASH, NOW);
        foreign.property = ID + 1;
        input.records = vec![record(Measured::Primary(other_kind, positive_for(other_kind)), current, BINDING_HASH, NOW), foreign];
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.0), Some(Primary::NoEvidence));
    }

    #[test]
    fn score_is_fresh_only_at_the_binding_hash_within_the_window(kind in kind(), scope in scope().prop_filter("non-empty", |s| !s.is_empty()), age in 0u64..(30 * DAY)) {
        let cfg = gate_config();
        let current = scope_hash_of(&scope);
        let scored = Measured::Score(ScoreOutcome::Scored { killed: 2, survived: 1, timeout: 0, unviable: 0 });
        let mut input = base_input(kind, clone_scope(&scope));
        input.records = vec![record(scored, current, BINDING_HASH, NOW - age)];
        let expected = if age <= cfg.freshness_secs { Score::Fresh(ScoreOutcome::Scored { killed: 2, survived: 1, timeout: 0, unviable: 0 }) } else { Score::Stale };
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.2), Some(expected));
        // Another binding hash is stale whatever its age; a future-dated record is not fresh.
        let mut input = base_input(kind, clone_scope(&scope));
        input.records = vec![record(scored, current, [5; 32], NOW)];
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.2), Some(Score::Stale));
        let mut input = base_input(kind, clone_scope(&scope));
        input.records = vec![record(scored, current, BINDING_HASH, NOW + 1)];
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.2), Some(Score::Stale));
        let s = derive(base_input(kind, clone_scope(&scope)), &cfg, NOW);
        prop_assert_eq!(bound(&s).map(|b| b.2), Some(Score::NoEvidence));
    }

    /// A commit to the scope: with primary evidence at the new hash and a score
    /// within the window the property reaches `top(kind)`; before the push run reports, it is
    /// not checked (evidence pending).
    #[test]
    fn a_scope_commit_holds_once_evidence_catches_up(kind in prop_oneof![Just(Kind::Test), Just(Kind::Proof)], scope in scope().prop_filter("non-empty", |s| !s.is_empty())) {
        let cfg = gate_config();
        let g = approved_governance();
        let mut touched = clone_scope(&scope);
        touched[0].digest[1] ^= 1;
        let before = scope_hash_of(&scope);
        let after = scope_hash_of(&touched);
        let scored = Measured::Score(ScoreOutcome::Scored { killed: 1, survived: 0, timeout: 0, unviable: 0 });
        let positive = Measured::Primary(kind, positive_for(kind));
        // Push run done at the new hash.
        let mut input = base_input(kind, clone_scope(&touched));
        input.drift = Drift { commits: 1, days: 0 };
        input.records = vec![record(positive, before, BINDING_HASH, NOW - 2 * DAY), record(scored, before, BINDING_HASH, NOW - DAY), record(positive, after, BINDING_HASH, NOW)];
        prop_assert_eq!(verdict(&derive(input, &cfg, NOW), &g), top(kind));
        // Push run not yet reported.
        let mut input = base_input(kind, clone_scope(&touched));
        input.drift = Drift { commits: 1, days: 0 };
        input.records = vec![record(positive, before, BINDING_HASH, NOW - 2 * DAY), record(scored, before, BINDING_HASH, NOW - DAY)];
        input.evidence_run_head = Some([4; 32]);
        prop_assert_eq!(verdict(&derive(input, &cfg, NOW), &g), Verdict::NotChecked { reason: NotCheckedWhy::EvidencePending, self_signed: false });
    }

    #[test]
    fn provisional_counts_negatives_anywhere_and_positives_only_at_current_hashes(kind in kind(), scope in scope().prop_filter("non-empty", |s| !s.is_empty()), other in digest()) {
        let cfg = gate_config();
        let current = scope_hash_of(&scope);
        prop_assume!(other != current);
        let mut input = base_input(kind, clone_scope(&scope));
        input.provisional = Some(run_result(Measured::Primary(kind, negative_for(kind)), other, BINDING_HASH));
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.1), Some(Provisional::Provisional(negative_for(kind))));
        let mut input = base_input(kind, clone_scope(&scope));
        input.provisional = Some(run_result(Measured::Primary(kind, positive_for(kind)), other, BINDING_HASH));
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.1), Some(Provisional::None));
        let mut input = base_input(kind, clone_scope(&scope));
        input.provisional = Some(run_result(Measured::Primary(kind, positive_for(kind)), current, BINDING_HASH));
        prop_assert_eq!(bound(&derive(input, &cfg, NOW)).map(|b| b.1), Some(Provisional::Provisional(positive_for(kind))));
    }

    #[test]
    fn derive_declaration_axis(kind in kind(), scope in scope(), until in any::<Secs>(), issue_open in any::<bool>(), previously in any::<bool>()) {
        let cfg = gate_config();
        let decl_of = |s: Status| match s { Status::ScopeEmpty { decl } | Status::Scoped { decl, .. } => decl };
        let mut input = base_input(kind, clone_scope(&scope));
        input.statement = unreviewed();
        input.previously_approved = previously;
        let expected = if previously { ProposedWhy::Amended } else { ProposedWhy::NeverReviewed };
        prop_assert_eq!(decl_of(derive(input, &cfg, NOW)), Decl::Proposed(expected));
        let mut input = base_input(kind, clone_scope(&scope));
        input.statement = Approval::Unreviewed(UnreviewedReason::ReviewerAccessRevoked);
        prop_assert_eq!(decl_of(derive(input, &cfg, NOW)), Decl::Proposed(ProposedWhy::ReviewerAccessRevoked));
        let mut input = base_input(kind, clone_scope(&scope));
        input.property = property(Some(kind), Some(Waiver { until }));
        input.waiver_issue_open = issue_open;
        let window = if NOW < until { Window::Active { until, issue_open } } else { Window::Expired { until } };
        prop_assert_eq!(decl_of(derive(input, &cfg, NOW)), Decl::Waived { sig: Sig::Team, since: [7; 32], window });
        let mut input = base_input(kind, clone_scope(&scope));
        input.adapter_available = false;
        if let Status::Scoped { binding, .. } = derive(input, &cfg, NOW) {
            prop_assert_eq!(binding, Binding::Unbound(UnboundWhy::NoAdapter));
        }
        let mut input = base_input(kind, clone_scope(&scope));
        input.binding = None;
        if let Status::Scoped { binding, .. } = derive(input, &cfg, NOW) {
            prop_assert_eq!(binding, Binding::Unbound(UnboundWhy::NoCheck));
        }
    }
}

// ---------------------------------------------------------------------------
// Sort, hash, affected: runtime witnesses of the proven properties
// ---------------------------------------------------------------------------

fn key(p: &PathDigest) -> (Vec<u8>, [u8; 32]) {
    (p.path.clone(), p.digest)
}

proptest! {
    #[test]
    fn sort_output_is_sorted_and_a_permutation(pairs in prop::collection::vec(path_digest(), 0..8)) {
        // The proven property, exactly: the output is a permutation of the input
        // (equal multisets of (path, digest)) and is ordered by path. Entries that
        // share a path may come out in any order, so the comparison is on sorted
        // keys, not element by element.
        let mut expected: Vec<_> = pairs.iter().map(key).collect();
        expected.sort();
        let sorted = sort_by_path(pairs);
        let mut got: Vec<_> = sorted.iter().map(key).collect();
        got.sort();
        prop_assert_eq!(got, expected);
        prop_assert!(sorted.windows(2).all(|w| w[0].path <= w[1].path));
        let distinct = sorted.iter().map(|p| p.path.clone()).collect::<BTreeSet<_>>().len() == sorted.len();
        prop_assert_eq!(is_strictly_sorted(&sorted), distinct);
    }

    #[test]
    fn scope_hash_is_ok_exactly_when_paths_are_distinct(pairs in prop::collection::vec(path_digest(), 0..8)) {
        let distinct = pairs.iter().map(|p| p.path.clone()).collect::<BTreeSet<_>>().len() == pairs.len();
        prop_assert_eq!(matches!(scope_hash(pairs), ScopeHash::Ok(_)), distinct);
    }

    #[test]
    fn affected_agrees_with_a_naive_intersection(changed in prop::collection::vec(path(), 0..6), scope_files in prop::collection::vec(path(), 0..6), binding_files in prop::collection::vec(path(), 0..6)) {
        let union: BTreeSet<_> = scope_files.iter().chain(binding_files.iter()).cloned().collect();
        let expected = changed.iter().any(|c| union.contains(c));
        prop_assert_eq!(affected(&changed, &scope_files, &binding_files), expected);
    }
}

#[test]
fn a_binding_helper_change_affects_exactly_its_property() {
    let a_scope = vec![b"src/a.rs".to_vec()];
    let a_binding = vec![b"tests/a.rs".to_vec(), b"tests/helpers/strategy.rs".to_vec()];
    let b_scope = vec![b"src/b.rs".to_vec()];
    let b_binding = vec![b"tests/b.rs".to_vec()];
    let changed = vec![b"tests/helpers/strategy.rs".to_vec()];
    assert!(affected(&changed, &a_scope, &a_binding));
    assert!(!affected(&changed, &b_scope, &b_binding));
    let unrelated = vec![b"README.md".to_vec()];
    assert!(!affected(&unrelated, &a_scope, &a_binding));
}
