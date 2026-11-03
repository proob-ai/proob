---
name: declare
description: Turn new or existing behaviour into plain-language property proposals, one TOML file each under proob/properties/, validated with proob validate, and reviewed on GitHub where the team approves, amends, or rejects them. One command for every situation, no flags. On a repository with no proob/config.toml it runs proob init and proposes from the whole picture. Given a requirement (free text, an issue number, or a URL) it proposes for that requirement. On a feature branch it proposes for the new code no property covers and supersedes properties the change made wrong, committing on that branch so the proposals are reviewed with the feature. On the default branch with nothing changed it proposes from the whole picture on a new branch. Use after /proob:sense, when a new requirement lands, when the session line says changed files have no property, or when asked to propose, declare, or add properties. The agent proposes; only a merged review approves.
argument-hint: "[requirement: text, #issue, or URL]"
allowed-tools: Read Glob Grep Bash Write Edit Skill
---

# proob declare

You propose; a human approves. Every file you write here is a proposal until
the pull request that carries it merges with a review, and proob derives
that approval from GitHub's records at read time, never from anything stored
in the repository. Nothing you do in this skill can make a property read as
anything but `Proposed`.

`$ARGUMENTS` is optional. When given, it is the requirement to declare: free
text, an issue number (`#123`), or a URL.

## Rules that hold throughout

- Write only: new files under `proob/properties/`, the files `proob init`
  writes on a first run, and `proob/.cache/`. Never edit an existing property
  file, approved or not: to change one, propose a new file and say in the PR
  body which property it supersedes and why; the reviewer deletes the old one.
- Never write `reviewed`, `waiver`, or `status` into a property file.
- In your own sentences, never use "holds", "verified", "guaranteed",
  "proven", or say that anything is approved. Those words may appear only
  inside a line you quote from the binary. The words for a proposal are
  `Proposed` and, where no adapter exists, `Not checked (no adapter)`.
- Quote the binary; never restate its numbers from memory. When
  `proob status --json` reports `verification.state` other than `verified`,
  label everything taken from it "local, unverified".
- Never push to the default branch, never merge, never approve, never post
  an approving review or comment.

## 0. Decide the mode

Run step 1's snippet first, then pick the first mode that applies. The
situation decides; there is no flag.

| # | Situation | Mode |
|---|---|---|
| 1 | `proob/config.toml` is missing | **first run**: init, then the full-repository flow |
| 2 | `$ARGUMENTS` is not empty | **requirement**: propose 1 to 3 properties for it |
| 3 | the current branch is not `$default` and it, or the working tree, changes something | **branch**: propose for its uncovered code, supersede what it made wrong |
| 4 | otherwise (the default branch, nothing changed) | **full repository**: picture, rank, cap, new branch, PR |

```sh
branch=$(git branch --show-current)
changed=$(git diff --name-only "$(git merge-base "$default" HEAD 2>/dev/null || echo HEAD)" 2>/dev/null; git ls-files --others --exclude-standard)
```

Mode 3 applies when `branch` is not empty, not `$default`, and `changed` is
not empty. A detached HEAD counts as a branch other than the default.

Begin your reply with one line that names the mode and why, in plain words:

- `declare: first run on this repository; running proob init and proposing from the whole picture`
- `declare: proposing for the requirement in #123 (refunds never exceed the charge)`
- `declare: this branch adds src/refunds/ with no property; proposing for it`
- `declare: this branch makes "Sorting preserves length" Broken; reading the change before proposing anything`
- `declare: on main with nothing changed; proposing from the whole picture`

Then follow the mode's section below. Steps 1, 3, 4, and 5 are shared.

## 1. Root, binary, default branch, first run

```sh
root=$(git rev-parse --show-toplevel) && cd "$root"
proob="${CLAUDE_PLUGIN_ROOT}/scripts/proob"; [ -x "$proob" ] || proob="${PROOB_BIN:-proob}"
default=$(git symbolic-ref -q --short refs/remotes/origin/HEAD 2>/dev/null | sed 's#^origin/##')
default=${default:-$(gh repo view --json defaultBranchRef -q .defaultBranchRef.name 2>/dev/null)}
default=${default:-main}
```

If `proob/config.toml` is missing, run `"$proob" init` and keep its output.
It writes `proob/config.toml` (the detected adapters and the gate policy),
`.github/workflows/proob-gate.yml` (the pull-request check),
`.github/workflows/proob-evidence.yml` (the default-branch and nightly runs
whose results are the only evidence), merges the plugin recommendation into
`.claude/settings.json`, and adds `proob/.cache/` to `.gitignore`. These go
into the same PR as the proposals, and the PR body explains each one (the
template has the paragraph). Do not edit what `init` wrote unless the user
asks. An existing `proob/config.toml` is governed content: leave it alone.

## 2. Full repository (modes 1 and 4)

### 2a. The picture

Read `proob/.cache/sense/picture.md`. If it is absent, or its `commit:` line
names a commit that is not an ancestor of `HEAD`, invoke the `proob:sense`
skill, wait for it, then read the file.

### 2b. Choose

Collect what not to propose again (step 3). Candidates are the picture's
"Invariants (candidates for declare)" blocks. Rank by severity times
evidence: severity critical 4, high 3, medium 2, low 1; evidence is the
number of distinct cited sources (name, test, comment, doc, incident,
changelog); break ties toward the larger scope. Keep at most `proposal_cap`
from `proob/config.toml` (`grep '^proposal_cap' proob/config.toml`; 5 when
absent). Fewer is fine: propose nothing rather than something the picture
does not support.

Also treat every law in the picture's "Interfaces and their laws" table as a
candidate, and keep at least one interface-law property among the proposals
whenever that table is not empty: implementations most often keep these laws
only in part.

Then write (step 4), validate (step 5), and open a pull request from a new
branch (step 6, "new branch").

## 2R. Requirement (mode 2)

Read the requirement:

- `#123` or a bare number: `gh issue view 123 --json title,body,comments`.
  When `gh` cannot read it, say so in one line and stop; do not guess its text.
- a URL: an issue or pull request URL on GitHub through `gh issue view <url>`
  or `gh pr view <url>`; any other URL through your fetch tool when you have
  one, else ask the user to paste the text.
- anything else: the text is the requirement.

Scope the proposals to the files this branch changes when there are any
(`changed` from step 0, source files only); else to the files the
requirement names; else to the files the picture (step 2a) links to it. If
none of these gives a file, say so and propose nothing.

Propose 1 to 3 properties that state what the requirement promises, each a
thing that must never happen or must always hold. Collect what not to propose
again (step 3), write (step 4), validate (step 5). Then commit: on a branch
other than `$default`, on that branch (step 6, "current branch"); on
`$default`, on a new branch (step 6, "new branch").

## 2B. Branch (mode 3)

Read the binary's view of this branch:

```sh
"$proob" status --json > proob/.cache/declare/status.json 2>/dev/null || true
"$proob" check --base "$default" --head HEAD --json > proob/.cache/declare/check.json; rc=$?
```

(`mkdir -p proob/.cache/declare` first.) `check` exits 0 or 1 with a report;
exit 2 means it could not run (a base it cannot resolve, say): then read
`uncovered` and the rows from `status.json` instead. `check` judges commits;
`status.json`'s `uncovered` also counts uncommitted and untracked files, so
read both.

**(a) Uncovered code.** `uncovered` lists changed source files that no
property's scope matches and no bound check reads. Group them by directory or
module. For each group, read the code and the diff (`git diff
"$default"...HEAD -- <paths>`, plus the files themselves when untracked),
and propose the properties the new behaviour promises, scoped to that group.
New code with nothing worth promising (glue, wiring, a rename) gets no
proposal; say which groups you passed over and why. Respect `proposal_cap`
across all groups.

**(b) Broken properties.** For each row in `check.json` whose `affected` is
true and whose word is `Broken`, read the property's statement and the
change to its scope:

- The code is wrong: the change breaks something the team still wants. Say
  so in one line naming the property and the file, and propose nothing for it.
  The fix is the code, not the declaration.
- The requirement changed what correct means (the user, the requirement, an
  issue, or the commit messages say so): write a superseding property file
  with a new ULID whose `statement` says what is now promised and whose
  `rationale` begins `Supersedes <old ULID>: ` and says why. Never edit the
  old file. In the PR text, ask the reviewer to delete the old file when they
  approve the new one.
- You cannot tell which: ask the user one question naming the property and
  the change, and wait.

Collect what not to propose again (step 3), write (step 4), validate (step
5), then commit on the current branch (step 6, "current branch").

## 3. What not to propose again

Collect every statement already proposed, approved, deleted, or rejected,
with the snippets in [reference/dedupe.md](reference/dedupe.md): the
existing `proob/properties/*.toml`; files deleted from the default branch;
`reject <ULID>` replies on past pull requests that touched
`proob/properties/`, read through `gh`. When `gh` is missing, unauthenticated,
or offline, say in the PR body that rejected statements could not be
checked. A candidate that means the same as any collected statement, in any
wording, is out. When in doubt, it is a duplicate. A superseding property is
not a duplicate of the one it supersedes: its statement differs by design.

## 4. Write one file per proposal

A fresh ULID for each, which is also the file name:

```sh
ulid() { python3 -c 'import os,time;A="0123456789ABCDEFGHJKMNPQRSTVWXYZ";v=(int(time.time()*1000)<<80)|int.from_bytes(os.urandom(10),"big");print("".join(A[(v>>(5*i))&31] for i in range(25,-1,-1)))' 2>/dev/null || printf '0%s\n' "$(LC_ALL=C tr -dc '0-9A-HJKMNP-TV-Z' </dev/urandom | head -c 25)"; }
```

`proob/properties/<ULID>.toml` (`mkdir -p proob/properties` first; `init`
does not create it), these keys in this order and nothing else:

```toml
id = "<ULID>"
title = "<a few words, as a reviewer would name it>"
statement = "<one sentence>"
scope = ["<glob>", "<glob>"]
severity = "<low | medium | high | critical>"
rationale = "<why it matters, citing the evidence: files, tests, notes, the requirement>"
```

- `statement`: one sentence, present tense, plain language a teammate
  without a formal-methods background reads in one pass; it names a thing
  that must never happen or must always hold; no function, type, module,
  file, or framework names. "A withdrawal never leaves an account below
  zero", not "withdraw checks that balance covers amount".
- `scope`: root-relative globs that match tracked files today (in branch
  mode, committed files of this branch count). `*` does not cross `/`, `**`
  does; no leading `/` or `-`, no `..`; only letters, digits, and
  `/ . _ - * ? [ ] { } , @ + ~`. A scope that matches nothing renders
  `Broken` once approved, so check each file with `"$proob" --json hash
  <ULID>`: `scope_files` must list at least one file. An untracked file is
  not in `scope_files` until it is committed; in branch mode commit the code
  first (step 6) or `git add` it, then check.
- `severity`: the evidence's guess (the picture's, the requirement's).
- `rationale`: cites the evidence by path, test name, note, or issue number;
  never the picture file itself; never a credential or a long token. A
  superseding property's rationale begins `Supersedes <old ULID>: `.
- `kind`: when `proob/config.toml` has an adapter with `language = "kani"`,
  every property whose scope is Rust code gets `kind = "proof"` if
  `proob/.cache/kani-probe.toml` reads `kani_probe = "passed"`
  (`/proob:falsify` then writes a Kani harness, and falls back to a test only
  for a reason it shows the reviewer), and `kind = "test"` otherwise: the file
  is missing, or reads `kani_probe = "failed: …"`. Say why in one line in the
  report and in the PR body, quoting that `kani_probe` line or saying the file
  is missing (`proob init --kani` writes it). When the failed line shows the
  crate needs a cargo feature (a `compile_error!` asking for one), name the
  fix too: `features = ["<name>"]` on the kani adapter block, then `proob
  init --kani` again to re-probe; this skill does not make it. Kani is Rust only: a Python
  property never gets `kind = "proof"` from this rule. Without a kani adapter,
  `kind = "proof"` only when the user asked for the proof route; otherwise
  omit the key and the bound check's adapter decides.
- `kind = "model"`: only when `proob/config.toml` has an adapter with
  `language = "tla"`, and only for a protocol-shaped scope: concurrency,
  retries, idempotency, leader election, a state machine, distributed
  coordination; behaviour that lives in how steps interleave, repeat, or fail
  rather than in one function's inputs. This rule comes before the kani rule
  for such a scope. `/proob:falsify` then writes a TLA+ spec of the design
  the scoped code implements and binds the statement to its invariant. The
  statement is a design invariant in plain words ("A payment is charged at
  most once, however many times the client resends it"), and the scope is
  the code files that implement the protocol: those are what a later commit
  must touch to flag the spec for re-approval, so leave out files the design
  does not describe. Say in the report and the PR body that a model checks
  the design over small constants, not the code. Without a tla adapter, never
  write `kind = "model"`: when a scope is protocol-shaped, say in one line
  that a model would fit and that `proob init --tla` opts in, and give the
  property the kind the rules above give it.
- No `"` or `\` inside a value; rephrase instead.

## 5. Validate

```sh
"$proob" validate; rc=$?
```

Fix and rerun until it prints `proob validate: ok`, then check every new
id with `"$proob" --json hash <ULID>` for a non-empty `scope_files`. Then
run `"$proob" status --json` and keep `headline`, `verification`, and the
`word` of each new id (expected: `Proposed`) for the PR body and the report.
In branch mode also keep `uncovered` from `status --json`: the paths still
without a property after your proposals.

## 6. Commit, pull request

In shadow (the file `.proob-shadow` exists at the root): skip branch, commit,
push, and PR; print nothing about reviewers; the files stay in shadow.

Write the PR body (or, on the current branch, the PR-body section) to
`proob/.cache/declare/pr-body.md` from
[reference/pr-body.md](reference/pr-body.md): read that file first and keep
its three numbered reviewer instructions word for word.

### New branch (modes 1, 4, and 2 on the default branch)

```sh
new="proob/declare-$(date +%F)"
git rev-parse -q --verify "$new" >/dev/null 2>&1 && new="$new-$(date +%H%M)"
git switch -c "$new"
git add proob/properties/
# only when init ran in this invocation:
git add proob/config.toml .github/workflows/proob-gate.yml .github/workflows/proob-evidence.yml .claude/settings.json .gitignore
git commit -m "proob: propose <N> properties"
git push -u origin "$new" && gh pr create --base "$default" --title "proob: <N> property proposals" --body-file proob/.cache/declare/pr-body.md
```

If there is no remote, the push fails, or `gh` is missing or unauthenticated:
keep the branch and the body file, print the whole body in your reply, and
give the user those two commands. Do not try other remotes or tokens.

### Current branch (modes 3, and 2 on a feature branch)

The proposals belong to the feature: commit them on the branch you are on,
so the same pull request carries the code and its declaration.

```sh
git add proob/properties/<ULID>.toml ...   # only the files you wrote
git commit -m "proob: propose <N> properties for <what the branch adds>"
```

Do not create a branch. Do not push unless the user asked. Print the
PR-body section (the template's table, "How to review", "Status now", and
"Supersedes" when there is one) for the user to paste into the feature's
pull request description, and say that if the pull request is already open,
the gate's next run will list the proposals as `Proposed`.

## 7. Report

In this order: the mode line (step 0); the `proob validate` line; the
`headline` from `proob status --json` with its verification state ("local,
unverified" when it is not `verified`); each proposal as `<ULID> —
<statement> — <severity> — <scope>`, with its `kind` when it has one (and,
with a kani adapter, the `kani_probe` line behind it; for a model, what makes
the scope protocol-shaped), and for a superseding
one `(supersedes <old ULID>: delete the old file when approving)`; in branch mode, each
`Broken` property you judged a code bug, and the uncovered paths still
without a property, with why; the PR URL, or the body and the commands, or
on the current branch the commit and the PR-body section; the statements left
out as duplicates and why; the init files, when init ran. End with: these
are proposals; each renders `Proposed` until a reviewer merges the pull
request, and none is checked until `/proob:falsify` binds a test, a Kani
harness, or a TLA+ spec to it.
