# The six assertions

The authoritative prose for the six assertions that define `Holds` and
`Waived`. Each is a `spec fn` in `src/specs.rs` and an `ensures` clause on an
exec function of this crate, checked by Verus under `--no-cheating` on every PR
to this repository. None is a test. If this file and `specs.rs` ever disagree,
this file is wrong about what is proven and `specs.rs` is wrong about what was
meant; fix both in one PR.

Notation: `s: Status` and `g: Governance` are `verdict`'s inputs and
`v: Verdict` its output; `intro: Introduction` and `allow_self_signed: bool`
are `eligibility`'s inputs and `a: Approval` its output. Types are in
`src/types.rs`.

## The assertions

| # | Prose | Spec predicate | Carried as `ensures` by |
|---|---|---|---|
| 1 | `Holds` only when the property file's current content was approved by an eligible review (`Team`; a self-signed merge never counts). | `statement_team_approved(s)` | `verdict` (U3): `v is Holds ==> statement_team_approved(*s)` |
| 2 | `Holds` only when the bound check's binding set, `proob/config.toml`, and the evidence workflow are each approved by an eligible review. | `check_and_governance_team_approved(s, g)` | `verdict` (U3): `v is Holds ==> check_and_governance_team_approved(*s, *g)` |
| 3 | `Holds` only when the property's kind is a verified kind, `proof` or `model`, the scope matches at least one file, and the latest default-branch primary evidence for that kind is fresh at the current scope and binding hashes and positive: verified for a proof, no violation for a model. A test's no-counterexample is positive evidence for `Checked (tested)`, never for `Holds`: a sampled run cannot speak for every input. | `verified_kind(s) && evidence_fresh_positive(s)`, with `positive(kind, outcome)` | `verdict`: `v is Holds ==> verified_kind(*s) && evidence_fresh_positive(*s)`; and `derive`, the only producer of `Primary::Fresh`: `binding.primary is Fresh(o) ==> exists record: latest_primary(records, id, kind, record) && record.scope_hash == scope_hash && record.binding_hash == binding_hash && record.measured == Primary(kind, o)` (`latest_primary`: a record of this property and kind that no such record postdates) |
| 4 | `Holds` only when the latest default-branch score at the current binding hash, within the freshness window, kills at least the floor (the floor is at least one; `NoMutants` and an errored run never count), and, for a model-backed property, no commit has touched the modelled scope since the spec's approval. | `strength_at_floor(s, g)`, with `score_at_floor(binding, floor)` and `model_undrifted(s)` | `verdict`: `v is Holds ==> strength_at_floor(*s, *g)`; and `derive`, the only producer of `Score::Fresh`: `binding.score is Fresh(o) ==> exists record: latest_score(records, id, record) && record.binding_hash == binding_hash && record.measured == Score(o) && record.at <= now && now - record.at <= freshness` (a record dated after `now` is never fresh) |
| 5 | `Waived` only when the waiver content was approved by an eligible review, the waiver's issue is open, and the window has not expired. | `waived_requirements(s)` | `verdict` (U3): `v is Waived ==> waived_requirements(*s)` |
| 6 | Eligibility is sound: `Team` only when some review on the introducing PR is eligible; `SelfSigned` only when the repository allows it and no review is eligible; a direct push is never approved; an approval names the PR and merge it came from. And no verdict is an input: `derive`, `verdict`, and `eligibility` take facts, and no file, cache entry, or payload type carries an `Approval`, `Status`, or `Verdict`. | `eligibility_sound(intro, allow_self_signed, a)`, with `eligible_review(f, r)` and `has_eligible_review(f)` | `eligibility` (U9): `ensures eligibility_sound(*intro, allow_self_signed, a)`. The second sentence is structural; see below. |

`holds_requirements(s, g)` is the conjunction of 1 to 4, and `verdict`'s first
`ensures` is `v is Holds ==> holds_requirements(*s, *g)`.

A property-based test is the falsifier: it finds counterexamples (`Broken`) and,
finding none, earns `Checked (tested)`. Only a verifier earns `Holds`. The two
are not alternatives for one slot; a property still binds one check, and the
word says which kind of evidence stands behind it.

## What the words mean in the types

- **Eligible review** (`eligible_review`): the review's state is `Approved`;
  its reviewer is `Human` (GitHub `type: User`, login not ending in `[bot]`)
  with write access at derivation time; the reviewer is not the PR author and
  is not among the authors or committers of any non-merge commit in the PR
  that added or modified a governed file (deletions exempt, co-author trailers
  ignored); no such author is unresolved (`unresolved_governed_author` fails
  closed); and the review is on a commit at which the governed file's blob
  equals its blob at merge (`blob_at_commit == Some(blob_at_merge)`; a blob
  that can no longer be fetched is `None` and does not count).
- **Approved by content**: `Approval` is per governed file and per content;
  the content hash (`content_hash`, `proob-content-v1`) is what the approval
  is about. A waiver is content, so `Decl::Waived` carries a `Sig` like
  `Decl::Approved`.
- **Fresh**: `Primary::Fresh` and `Score::Fresh` are constructors `derive`
  produces only under the hash (and, for scores, age) conditions in rows 3
  and 4. `Pending`, `Stale`, and `NoEvidence` are never positive.
- **Positive** (`positive`): `(Test, NoCounterexample)`, `(Proof, Verified)`,
  `(Model, NoViolation)`. Every other pairing, including a kind mismatch, is
  not positive, so a record of the wrong kind can never produce `Holds` or
  `Checked`.
  **Negative** (`negative`) is the mirror: `(Test, Counterexample)`,
  `(Proof, ProofFailed)`, `(Model, Violation)`; `verdict` renders a negative
  as `Broken(Negative(ThisPr | DefaultBranch))`.
- **Verified kind** (`verified_kind`): the property's kind is `Proof` or
  `Model`. `Test` is not one, whatever its score, so the strongest word a test
  can earn is `Checked { tested: true }`. Kani binds as `Proof` (its harness
  runs against the code, so the model drift rule in row 4 does not apply);
  TLA+ binds as `Model`.
- **Floor** (`score_at_floor`): `1 <= floor <= killed` on a
  `Score::Fresh(Scored { killed, .. })`. `Governance.floor` is at least one by
  `gate_config_wf`; the facts crate refuses a zero `kill_floor`.
- **Window** (`Window::Active { until, issue_open }`): `derive` sets `Active`
  only when the instant is before `until`; `issue_open` is the issues API's
  answer. `Expired` carries no issue state because it is `Broken` regardless.
- **Provisional** (`Provisional::Provisional(outcome)`): this PR's own run.
  It is not `Primary` and no predicate above reads it, so it cannot contribute
  to `Holds`; `verdict` turns a negative one into `Broken(Negative(ThisPr))`.

## The structural half of assertion 6

Nothing proves the absence of a code path; the layering makes it visible
instead.

- `proob-facts` defines `serde` types for property files, config, evidence
  records, and GitHub payloads, and none for `Approval`, `Status`, `Verdict`,
  or `Introduction`. A file or payload cannot contain a verdict because no
  parser exists for one.
- `Evidence` and `RunResult` are distinct types with no conversion between
  them in any crate; `RunResult` reaches `derive` only as `Provisional`.
- The fact cache (`proob-effects::cache`) stores fetched GitHub payloads
  keyed by commit sha and PR number, permission facts with a time to live,
  and evidence artifacts by run id. Its entry types are the facts crate's
  payload types, so it stores no `Approval`; every run re-derives approvals
  through `eligibility`.
- `tools/crate-graph-check.sh` keeps `proob-core` on `vstd` and
  `proob-trusted` alone, so nothing can be injected below the verified layer.

## Eligibility's answer, the preview, and evidence acceptance

Assertion 6 bounds `eligibility` from above; `eligibility.rs` also pins the
whole answer (`eligibility_reasoned`): an eligible review always yields
`Team`; an approver eligible in every respect but write access now yields
`Unreviewed(ReviewerAccessRevoked)` (A11: GitHub keeps no record of
permission at review time, so losing it is read as revocation, ranked above
a self-signed downgrade); then `SelfSigned` when the repository allows it;
otherwise `NoEligibleReview(reason)`, where the reason names the first rule
that stopped the approving review closest to eligible (identity, bot, write
access, governed edit, blob at the reviewed commit), or `NoReview`. Each
reason's `ensures` states what it says about every approving review.
`preview.rs` runs the same function on the facts a merge would have now and
pairs the approval with the one `Remedy` its table gives it, each remedy an
`ensures`. `evidence_accept.rs` decides which workflow runs may supply
`Evidence` (`spec_accept`): the evidence workflow's path, the default branch,
a push, schedule, or dispatch event, success, and the workflow blob that is
approved on the default branch now. `binding_hash.rs` is the third algorithm
tag, `proob-binding-v1`, over the scope hash of a binding set.

## Well-formedness predicates

These are invariants the types cannot state structurally. They are not
assertions, but the exec functions rely on them.

| Predicate | States | Established by |
|---|---|---|
| `property_wf(p)` | `scope` is non-empty and every glob satisfies `grammar::glob_ok`. | `proob-facts::convert::property`, through the verified `scope_ok_exec`. |
| `gate_config_wf(c)` | `floor >= 1`. | `proob-facts::config::parse_config` and `convert::gate_config`. |
| `verdict_wf(v)` | `Checked` carries at least one modifier: `tested`, `strength_pending`, or `self_signed`. `tested` is not a defect; the other two are. | `verdict` (U3), as an `ensures`. |
| `measured_wf(m)` | A primary outcome is one its kind can produce (`coherent`). | `proob-facts::convert::evidence`. |

## Scope hash

Not an assertion, but the premise of "at the current hashes" in rows 3 and 4:
`scope_hash::combine_sorted` computes `spec_scope_hash` of a canonical pair
list (U1); `scope_hash::sort_by_path` is a verified sort whose output is a
permutation of its input (`to_multiset()` equality) ordered by path;
`lemma_canonical_unique` proves the strictly sorted permutation of a multiset
unique, because `lex_lt` is a strict total order; and `scope_hash::scope_hash`
therefore computes `spec_scope_hash_of(pairs.to_multiset())`, a function of
the set of (path, digest) pairs rather than their order
(`lemma_scope_hash_order_independent`). Two pairs with one path have no
canonical form and answer `ScopeHash::DuplicatePath`; `derive` requires
`paths_distinct` of its resolved files (`paths_distinct_exec` decides it).

## Verdict precedence and policy

`verdict.rs` documents, as a table, which word wins when several apply; the
`ensures` on `verdict` carry the rows that matter for honesty (this PR's own
negative or errored run is `Broken`; a lapsed waiver and an empty scope are
`Broken`; a self-signed statement is never `Holds` nor `Proposed`; `Holds` and
`Checked` promise team-approved or self-signed governance, never unreviewed).
Rows 24 and 25 of that table read: 24, primary `Fresh(positive)`, kind `Proof`
or `Model`, no self-signature, score fresh at the floor, `Holds`; 25, primary
`Fresh(positive)`, `Checked { tested: kind is Test, strength_pending: score not
at floor, self_signed: ss }`. `Verdict::Checked` gains the `tested` field;
nothing in `Binding`, `derive`, `eligibility`, the facts crate, or the CLI's
one-check-per-property rule changes.
`policy.rs` carries R10 as `ensures`: `Block` only for a `Broken` property the
PR affects, whatever the configured table says elsewhere. Its `ensures` also
fix one row per verdict: a `Checked` whose only modifier is `tested`
(`tested_only`) takes the `tested` row, and any other `Checked` (one carrying
`strength_pending` or `self_signed`) the `checked` row, so `checked` means a
defect is pending.
