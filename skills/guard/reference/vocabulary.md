# The verdict words

Every rendered status is one of these words. The binary chooses the word; the
PR check, the session line, and guard all use the same one.

| Word | Meaning | Reader's action |
|---|---|---|
| **Holds** | approved statement, approved check, approved config; positive evidence on the default branch; the check proven able to fail (kill count at or above the floor, within the freshness window) | none |
| **Checked** | as Holds, but strength not yet measured (`strength pending`), or the approval is self-signed (`self-signed`) | none; wait for the scheduled run, or get a second reviewer with write access |
| **Not checked** | approved, but no check bound, no adapter for the language, evidence pending or stale, the runner errored, or GitHub unreachable | bind a check (`/proob:falsify` or `/proob:verify`), or wait for the next run |
| **Re-approve** | the statement, check, config, or workflow changed since its last approval; the approver lost write access; or a model spec drifted | review the amendment PR, or re-affirm (guard proposes the PR) |
| **Broken** | a counterexample, a failed proof, or a runner error on this PR; a scope that matches nothing; or an expired waiver | fix the code, fix the runner, or amend the statement |
| **Proposed** | never approved: not yet on the default branch, or merged without an eligible review | review and merge, or reject |
| **Waived until d** | approved, failing, human-waived with an open tracking issue until date d | track the issue |

Modifiers that may follow a word: `(strength pending)`, `(self-signed)`,
`(no adapter)`, `(no check)`, `(evidence pending)`, `(evidence stale)`,
`(rate limited)`, `(statement amended)`, `(reviewer access revoked)`.

Two markers travel with any rendering:

- `unverified: <reason>`: GitHub's records were not read in this run (no
  token, no remote, unreachable, or rate-limited). Nothing can read Holds in
  an unverified view; what you see is the tree plus cached facts of the age
  shown.
- `Re-approve (config amended; workflow amended)` in the headline: governed
  content changed since its approval, so the built-in policy applies until it
  is re-approved.

What approval means: a governed file counts as approved only when the pull
request that introduced its current content into the default branch merged
with an eligible review: a human with write access at derivation time, not
the author, not someone who edited a governed file in that PR. With
`allow_self_signed = true`, a merge with no eligible review is a
self-signature: Checked (self-signed) at best, never Holds.

What drift means: the number of commits that touched a property's scope since
its approving merge. Drift alone never blocks and never demotes Holds for a
test-backed or proof-backed property; above `drift_threshold` guard proposes
a re-affirmation so a human re-reads the statement against the changed code.
