# Review fixtures

One scenario per rule of the approval derivation (plan: "One approval rule
for every governed file"). Each file is a set of GitHub payloads in the
shapes `proob-facts::github` reads (what the REST API would answer) plus the
repository facts git would give (`history`, `blobs`, `heads`, `commit_times`).
A fixture holds facts only; the expected approval and remedy of each scenario
live in `crates/proob-effects/tests/eligibility_fixtures.rs`, so no fixture
can carry a verdict. `proob-effects::fixture` serves them as a `GitHubApi`
and a `Repo`.

Cast: `alice` (1001, PR author), `bob` (1002, write), `carol` (1003, read),
`dave` (1004, write unless removed), `dependabot[bot]` (2001, Bot),
`web-flow` (19864447, GitHub's web committer). Shas are one hex digit
repeated 40 times: base `0`, commits `1` `2` `3`, merge commit `a`, squash
`b`, rebased `c` `d`, direct push `e`; blobs `5` and `6`. The governed file is
`proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4C6.toml`.

| Scenario | Rule | Approval (allow_self_signed = false) | Remedy |
|---|---|---|---|
| non-author-approval-with-write | the baseline | `Team` | none |
| author-only | only the author's (comment) review | `NoEligibleReview(NoReview)`; `SelfSigned` when allowed | add reviewer |
| approve-then-changes-requested | latest state per reviewer wins | `NoReview` | add reviewer |
| dismissed | a dismissed review never approves | `NoReview` | add reviewer |
| earlier-commit-blob-unchanged | review on an earlier commit, same blob at merge | `Team` | none |
| earlier-commit-blob-changed | review on an earlier commit, blob changed | `ApprovalPredatesChange` | re-approve latest commit |
| reviewed-commit-blob-unavailable | the reviewed commit's blob is gone (404) | `ApprovalPredatesChange` | re-approve latest commit |
| bot-approver | bots never count | `BotOnly` | add reviewer |
| approver-without-write | write is checked before the blob | `NoWriteAccess` | add reviewer |
| reviewer-pushed-governed-edit | an approver who edited governed content | `ApproverEditedGoverned` | ask author to commit suggestion |
| reviewer-only-deleted-proposal | deletions are exempt | `Team` | none |
| reviewer-update-branch | merge commits are exempt | `Team` | none |
| author-committed-suggestion | suggest-then-commit keeps the reviewer eligible | `Team` | none |
| unresolvable-author | a governed commit with no resolvable author fails closed | `UnknownIdentity` | resolve identity |
| non-default-base | a PR into another branch is not the introduction | `DirectPush` | open pull request |
| merge-commit-merge | merge-commit strategy | `Team`, merge `a` | none |
| squash-merge | squash strategy | `Team`, merge `b` | none |
| rebase-merge | rebase strategy (see below) | `Team`, merge `c` | none |
| no-reviews | no review at all | `NoReview`; `SelfSigned` when allowed | add reviewer |
| direct-push | no associated PR | `DirectPush` | open pull request |
| reintroduced-by-direct-push | approval follows the introducing commit, not the content; `previously_approved` is true | `DirectPush` | open pull request |
| approver-permission-removed | an otherwise eligible approver without write now | `ReviewerAccessRevoked` (also when self-signed is allowed) | add reviewer |
| open-pr-approval-predates-head | the "if merged now" row | `ApprovalPredatesChange` | re-approve latest commit |
| open-pr-ready | the "if merged now" row | `Team` | none |

Two scenarios share a reason word on purpose: a reviewed commit whose blob
cannot be fetched reads `ApprovalPredatesChange`, as a changed blob does,
because neither review is about the content that merged. And GitHub keeps
no record of permission at review time, so `approver-permission-removed`
and a reviewer who never had write access are the same facts; both read
`ReviewerAccessRevoked` (plan assumption A11), while `approver-without-write`
exercises the precedence that makes `NoWriteAccess` reachable at all: the
approver also reviewed a commit whose blob later changed.

## Rebase merges

`rebase-merge` encodes GitHub's documented behaviour for
`GET /repos/{owner}/{repo}/commits/{sha}/pulls`: a rebased commit on the
default branch lists the pull request it came from, and the PR's
`merge_commit_sha` is the last rebased commit. The introducing commit for a
file is whichever rebased commit last touched it, so the approval's
`merge_sha` is that commit (`c` here), not `merge_commit_sha` (`d`). This
must be confirmed against a live rebase-merged PR in U14; if GitHub does not
associate rebased commits, the fallback is the merge-commit-range walk noted
in the plan's risks table.
