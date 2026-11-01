---
name: guard
description: Where the team stands on its declared properties. Reports verdict counts, whether the view is verified, and the governance footer; proposes re-affirmation PRs for drifted or revoked-access properties; explains any verdict word; prints the teammate install steps. Use for "proob status", "what holds", "is the gate required", "what does Re-approve mean", or "how do teammates install proob".
argument-hint: "[vocabulary | install | reaffirm]"
allowed-tools: Bash(${CLAUDE_PLUGIN_ROOT}/scripts/proob *)
---

# /proob:guard

Guard is the deterministic stage of proob. The binary derives every verdict
from the working tree, GitHub's review records, and default-branch evidence;
you read its JSON and report it. You never judge, and nothing you do can make a
property look more approved than GitHub's records say.

## Rules that hold in every mode

- Numbers, words, and footer values come from the JSON. Never estimate,
  round, or infer a count; if a value is absent, say it is absent.
- Say the view is verified only when `verification.state` is `"verified"`.
  Otherwise lead with the `unverified` marker and its reason (`no_token`,
  `no_remote`, `github_unreachable`, `rate_limited`) and the cache age.
- Never write a `status` key anywhere. Status is derived, never stored.
- Never write or change `waiver`. Never edit `statement`, `scope`,
  `severity`, `rationale`, `kind`, or `id`. Never touch a workflow file.
- Write `reviewed` only inside a re-affirmation PR (below), and nothing else
  in that PR.
- Never approve, merge, or wait for an approval, a branch rule, a ruleset,
  or a CODEOWNERS change. State the step and the GitHub path; move on.
- Never say GitHub settings changed. You cannot change them and the binary
  only reads them.

## Dispatch on `$ARGUMENTS`

- `vocabulary` (or a question about a verdict word): answer from
  [reference/vocabulary.md](reference/vocabulary.md), word by word as asked.
- `install` (or "how does a teammate get proob"): print
  [reference/team-install.md](reference/team-install.md) as it is.
- `reaffirm`: run the report (step 1) silently, then go to step 4.
- empty: steps 1 to 4.

## Step 1. Derive

Run both, from the repository root:

```bash
${CLAUDE_PLUGIN_ROOT}/scripts/proob status --json > "${TMPDIR:-/tmp}/proob-status.json"
${CLAUDE_PLUGIN_ROOT}/scripts/proob status --summary
```

Then read `${TMPDIR:-/tmp}/proob-status.json`. Field map:

| What | Where in the JSON |
|---|---|
| counts | `counts.{holds,checked,not_checked,re_approve,broken,proposed,waived,unverified,total}` |
| verified? | `verification.state` (`verified`/`unverified`/`not_consulted`), `verification.reason`, `verification.cache_age_secs` |
| config in force | `config.in_force` (`approved`/`defaults`/`unknown`), `config.why` |
| footer | `footer.{approvals_verified,branch_requires_review,gate_required,codeowners,scheduled_evidence_age_secs}` |
| governance | `governance.{config,evidence_workflow,gate_workflow}.state` (+ `.sig`, `.pr`), `governance.floor` |
| a property | `properties[]`: `id`, `path`, `title`, `word`, `reason`, `verdict.word`, `verdict.why.kind`, `status.decl.state`, `status.decl.drift.{commits,days}` |
| new code without a property | `uncovered`: changed source files (this branch and the uncommitted work, against the default branch) that no property's scope or bound check covers |

If the command fails:

- exit 2 and stderr says `no proob/ directory`: there is no declaration yet.
  Say so and point to `/proob:sense`. Stop.
- a single `proob:` line on stderr (the binary is not installed and cannot be
  downloaded, or the shim is missing): repeat that line verbatim as the
  whole answer. Do not guess at status. Stop.
- the user hands you a saved `status --json` (CI output, another machine, a
  sandbox where the binary cannot run): read it the same way, open with "from
  a saved report at <`inputs.now` as a date>", and never present it as live.

## Step 2. Report

Print, in this order:

1. The `--summary` line, verbatim.
2. Verification: `verified (live)`, or `unverified: <reason>; cached facts
   from <age>` / `no cached facts`.
3. Counts, non-zero ones, using the vocabulary words: `Holds n · Checked n ·
   Not checked n · Re-approve n · Broken n · Proposed n · Waived n ·
   Unverified n (total n)`. `Checked` includes `Checked (tested)` rows; a
   test-backed property reaches `Checked (tested)` at best, never `Holds`.
4. Governance footer, one line each:
   - approvals verified via GitHub reviews: `footer.approvals_verified`
   - config / evidence workflow / gate workflow: `approved (team, PR #n)`,
     `approved (self-signed, PR #n)`, or `unreviewed (<reason.kind>)`
   - default branch requires review: `footer.branch_requires_review`
   - proob gate required: `footer.gate_required`
   - CODEOWNERS covers proob: `footer.codeowners`
   - kill floor in force: `governance.floor`
   - scheduled evidence last ran: age, or `never`
5. Properties that need a human: every row whose `word` is `Re-approve`,
   `Broken`, or `Proposed`, with its `reason`. Rows that hold, are checked,
   or are not checked within grace need nothing; list them only if asked.
6. New code without a property, only when `uncovered` is a non-empty list:
   `New code without a property: <paths, comma-separated, at most 10, then
   "and N more"> · run /proob:declare on this branch to propose some`. It is
   informational: it never changes a verdict or the gate.

## Step 3. Warn once per run on governance gaps

Each of these is a standing warning until an administrator acts. Print each
that applies once, exactly, and do not repeat it later in the conversation.

- `footer.branch_requires_review` is `no` or `unknown`:
  "The default branch does not require review (or proob could not tell). Any
  push to it bypasses approval. Fix: GitHub → repository Settings → Rules →
  Rulesets (or Branches → Branch protection rules) → rule for the default
  branch → Require a pull request before merging → Require approvals: 1.
  `unknown` means the token could not read branch rules; a repository admin
  can confirm."
- `footer.gate_required` is `no` or `unknown`:
  "The `proob gate` check is not required to merge (or proob could not tell).
  A PR can merge with a Broken property. Fix: same rule → Require status
  checks to pass → add `proob gate`. Also consider Require branches to be up
  to date, so an open PR's green check is not stale."
- `footer.codeowners` is `no`: print this snippet and say it goes in a PR
  (`.github/CODEOWNERS`, path/team to taste), and that the branch rule
  Require review from Code Owners must be ticked by an admin for it to bind:

  ```text
  # proob: the declaration and its gate need a reviewer who did not write the change
  /proob/                                 @ORG/TEAM
  /.github/workflows/proob-gate.yml       @ORG/TEAM
  /.github/workflows/proob-evidence.yml   @ORG/TEAM
  ```

State each gap and its fix in one breath; never ask the user to confirm an
administrative step before you finish the report.

## Step 4. Drift and re-affirmation

Read `drift_threshold` from `proob/config.toml` (default 10 when absent).
A property is a re-affirmation candidate when either holds:

- `status.decl.state` is `approved` and `status.decl.drift.commits` is
  greater than `drift_threshold`: the scope changed under an unchanged
  statement.
- `verdict.word` is `re_approve` and `verdict.why.kind` is
  `reviewer_access_revoked`: the approver no longer has write access, so
  their approval no longer vouches.
- `verdict.word` is `re_approve` and `verdict.why.kind` is `spec_drifted`:
  a model property's scope changed since approval (`verdict.why.commits`
  commits), whatever `drift_threshold` says, because nothing checks that the
  changed code still follows the spec. Say the reviewer must re-read the
  spec the property is bound to against the changed code, and that if the
  design changed, the spec needs amending (`/proob:falsify`) before the
  re-affirmation.

Not candidates: `statement_amended`, `check_amended`, `config_amended`,
`workflow_amended` (the amendment PR needs a review, not a new date),
`proposed` (needs its first review), `broken` (needs a fix).

List the candidates with id, title, and reason (`14 scope commits since
approval, threshold 10`, `approver lost write access`, or `spec drifted: 3
commits to the modelled scope`). If there is at
least one, propose the re-affirmation PR described in
[reference/reaffirmation.md](reference/reaffirmation.md) and, when the user
agrees, carry it out exactly as written there: one branch
`proob/reaffirm-<YYYY-MM-DD>`, one commit, each candidate file changed in its
`reviewed` line and nowhere else, a PR that a reviewer other than its author
approves by merging. You open the PR; you do not approve, merge, or wait.

If there are no candidates, say so in one line.
