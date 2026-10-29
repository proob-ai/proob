# The re-affirmation PR

A re-affirmation asks a human to re-read a statement against code that moved
under it (drift above `drift_threshold`) or to replace an approval whose
reviewer lost write access. It changes one thing per property file: the
`reviewed` date. The new content needs an eligible review to be approved
again; that review is the re-affirmation, and it resets drift to the new
approving merge.

## What changes

For each candidate `proob/properties/<id>.toml`, exactly one line:

- if a `reviewed = ...` line exists, its value becomes today's date;
- otherwise one line `reviewed = YYYY-MM-DD` is added among the top-level
  keys (after `rationale` or `kind`), and before any `[waiver]` table, so it
  does not land inside the table.

The value is a bare TOML date (`reviewed = 2026-10-29`), not a string. No
other byte of the file changes: not the statement, not the scope, not the
severity, not the rationale, not a waiver, never a `status` key. No other
file changes.

## Steps

1. From an up-to-date default branch:
   `git switch -c proob/reaffirm-<YYYY-MM-DD> <default branch>`.
2. Edit each candidate file as above (one `Edit` per file; the PreToolUse
   hook denies anything that is not valid schema).
3. Check: `proob validate` passes; `git diff --stat` lists only the candidate
   files; `git diff` shows one changed or added `reviewed` line per file and
   nothing else. If anything else moved, restore the file and redo the edit.
4. Commit: `proob: re-affirm <n> property(ies)` with a body listing each id,
   title, and reason (`<n> scope commits since approval, threshold <t>`, or
   `approver lost write access`).
5. Push and open the PR (`gh pr create`) with this body:

   ```text
   Re-affirmation of <n> propert(y|ies). Only the `reviewed` date changes.

   | Property | Why |
   |---|---|
   | <title> `<id>` | <reason> |

   Approving and merging this PR re-approves the statement(s) as written.
   The approver must have write access and must not be the author of this
   PR; proob derives that from GitHub's review records. Until it merges the
   property keeps its current verdict. Please re-read each statement against
   the current code before approving; if a statement is no longer what the
   team means, request changes instead and amend it in a separate PR.
   ```

6. Report the PR URL and the one person who needs to act (a reviewer with
   write access who is not the author). Stop there: do not approve, merge,
   or wait, and do not re-run status expecting a change before the merge.

## Never

- Never re-affirm a `statement_amended`, `check_amended`, `config_amended`,
  or `workflow_amended` property this way; those need the amendment PR
  reviewed.
- Never add `reviewed` to a proposed (never-approved) property.
- Never combine a re-affirmation with any other change in the same PR.
