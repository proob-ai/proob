---
name: try
description: Try proob on this repository without changing it. proob makes a private shadow copy, then senses the code, proposes up to five properties, writes and runs their tests, and reports each result with the exact steps to bring them into the real pipeline. Use when someone wants to evaluate proob, see what it would find, or try it before adopting it. Nothing in the repository, its branches, or its git history changes until the user runs proob shadow adopt themselves.
argument-hint: "[path or focus]"
allowed-tools: Read Glob Grep Bash Write Edit
---

# /proob:try

The one entry point for someone evaluating proob. Everything happens in a shadow: a private clone
of the repository under `~/.proob/shadow/`. The user's working tree, index, branches, and `.git`
stay exactly as they are. `$ARGUMENTS` is an optional path or focus (a directory, a module, a
concern).

## Shadow rules

These override anything in the skills this one follows.

- Never branch, commit, push, or open a pull request, in the shadow or in the repository.
- Never write outside `$work` (set in step 1). Every file you create or edit is under it.
- Write files only in `$work`. proob captures them into the shadow's overlay by itself; never
  copy them anywhere else.
- At most 5 properties.
- A finding is only what a check recorded: a counterexample, a failed proof, or a model
  violation, as `proob shadow report` shows it. What you notice while reading the code is not
  a finding, and you do not report it, however likely it looks. A proposal you cannot give a
  test or harness that its adapter runs stays unchecked, and the report says so; never describe
  what its check would have found.
- Run every check and score as `"$proob" --shadow run check <id> --json` and
  `"$proob" --shadow run score <id> --json`. Shadow records every run; the report reads them.
- In your own sentences never use "holds", "verified", "guaranteed", "proven", or say anything is
  approved. In shadow every property is `Proposed`: no one has approved it, and the results are
  local.

## 1. Read the environment, then open the shadow

`shadow open` looks for a project at the repository root only. Where the projects are is yours
to read first. From the repository the user is in (these commands write nothing):

```sh
git rev-parse --show-prefix
git ls-files | grep -E '(^|/)(Cargo\.toml|pyproject\.toml|setup\.py|requirements[^/]*\.txt|pytest\.ini|conftest\.py)$'
```

proob's workflows install a Python project from `pyproject.toml`, `setup.py`, or
`requirements.txt` at its root. Another requirements file (`requirements-test.txt`) marks the
project and is not installed.

A project's root is the directory its tests run from: the one that holds `pyproject.toml`,
`setup.py`, or `requirements.txt` (Python), or `Cargo.toml` (Rust). A Cargo workspace is one
project at its workspace root; never name a member crate. A Python directory with none of those
files but with its own tests (a `conftest.py`, a `tests/` directory) is a project too. Then:

- A project at the repository root needs no flag: proob finds it.
- A project below the root is named with `--project python:<dir>` or `--project rust:<dir>`,
  the directory relative to the repository root. Name the project this try is about: the one
  that contains `$ARGUMENTS` when that is a path; else the one that contains the directory the
  user is in (the prefix printed above); else, when the root has no project, the one with the
  most tracked source files. Name a second only when the focus spans two.
- When you chose the project for the user, say which in one line, and that `/proob:try <dir>`
  picks another. That directory is the focus for the steps below when `$ARGUMENTS` names none.

proob then decides the rest from what is at the named root: the adapter, the runner, the
mutation tool, and what the workflows install.

```sh
proob="${CLAUDE_PLUGIN_ROOT}/scripts/proob"; [ -x "$proob" ] || proob="${PROOB_BIN:-proob}"
"$proob" shadow open; rc=$?    # below the root: "$proob" shadow open --project python:services/chatbot
work=$("$proob" shadow path)
```

Show the lines `shadow open` printed, verbatim. If it exits 2, show its one `shadow:` line and
stop: the line names the fix, or says proob cannot check this repository (the read above found
no Rust or Python project at any depth, so none was named). Add nothing to it. From here on, every command runs with `$work` as its working
directory (`cd "$work"` first, every time), and every file path you read or write is under
`$work`.

In a Rust repository `shadow open` sets Kani up only when the pinned `cargo kani` is already
installed (it never installs it), and then runs Kani's build probe; its result is the
`kani_probe` line in `$work/proob/.cache/kani-probe.toml`, which declare reads. The probe
builds with the kani block's cargo features. When it failed because the crate needs one (the
line quotes a `compile_error!` asking for a feature, and `init` left its features hint in the
block), set `features = ["<name>"]` on both the rust and the kani block of
`$work/proob/config.toml`, run `"$proob" init --kani` in `$work` to re-probe, and say so in the
report.

## 2. Sense, declare, falsify, in the shadow

Read each skill's SKILL.md and follow it in `$work`, under the shadow rules above. They are
`${CLAUDE_PLUGIN_ROOT}/skills/sense/SKILL.md`, then `declare` and `falsify` in the same
directory (and `verify` in step 4). Read the files rather than invoking the skills, so that everything stays in
`$work`.

1. **sense**, with `$ARGUMENTS` (or the project step 1 chose) as its focus. It writes `$work/proob/.cache/sense/picture.md`.
2. **declare**: at most 5 proposals, written under `$work/proob/properties/`. `proob shadow open`
   has already run `proob init` in the shadow, so skip init. Use its full-repository mode (its
   step 2) whatever its step 0 would pick: in a shadow there is no feature branch to review
   with, and `$ARGUMENTS` is a focus, not a requirement. Skip its step 6 (commit, PR).
   If `gh` cannot read past pull requests, skip the rejected-statements check without comment.
   Run `"$proob" validate` in `$work` until it prints `proob validate: ok`. With a Kani adapter
   and a passed probe, declare marks Rust proposals `kind = "proof"`.
3. **falsify**: one property-based test per proposal, in the team's own test layout under
   `$work`, each with its `proob:<ULID>` token. Run each with
   `"$proob" --shadow run check <id> --json`. A counterexample is a finding. Keep it, and never
   weaken the test or edit the property to make it pass. Skip its step 6 (pull request).
   When a check errors because a Python tool is missing, run the command its reason gives in
   `$work`, which makes proob's own environment `proob/.cache/python`, then install the
   project's test dependencies with that environment's pip; never install into the user's
   Python or their `.venv`.
   A `kind = "proof"` proposal gets one Kani harness instead, under falsify's harness rules; a
   Kani failure is a finding like a counterexample. When Kani cannot take a proposal for one of
   falsify's fallback reasons, follow its fallback (the one property edit it allows) and keep the
   reason line for the report. A `kind = "model"` proposal (only with a tla adapter) gets one
   TLA+ spec under falsify's step 3M; a violation is a finding like a counterexample. Run every property's check before any score.

## 3. Show the checks, before any score

```sh
cd "$work" && "$proob" shadow report
```

Show its terminal output verbatim, in a code block, and add nothing to it. A row whose check
passed and has no score yet says `not scored yet`; that is the honest state, not a gap to fill.
The user sees what passed and what failed now, before the slower scores start.

## 4. Score

**verify**: for each property whose check passed, run `"$proob" --shadow run score <id> --json`
once, so the report can say whether the test can fail. A score runs at most `max_mutants`
mutants of the scope (default 30 for rust and python, 10 for a Kani harness in the shadow, since
each mutant is a full Kani run), a sample fixed by the property id; when it sampled, its JSON has
`unsampled` > 0 and the report says how many were not run. Never call a sampled score exhaustive.
A Kani harness covers only the inputs its bounds admit: when you describe one in your own words,
say "checks locally within the harness's bounds" and name the bounds. A TLA+ model checks the
design, not the code: say "model-checked locally over the constants in the cfg" and name them.

## 5. Report

```sh
cd "$work" && "$proob" shadow report
```

Show its terminal output verbatim, in a code block, and add nothing to it. Then say in one
sentence what the next step to make it real is: step 0 when the report has one (fix the code, or check or
drop a property), otherwise `proob shadow adopt`, which puts everything on a new branch
of their repository without switching their checkout. Before that sentence, if any property fails, list the failures in the report's order (most severe first), each in one line with its severity and the consequence its `rationale` names (from `report.json`); within one severity, those that use realistic inputs come before those that fail only on an extreme input (see falsify's step 5); never drop either kind. If any proposal fell back from Kani to a test, say which, quoting its reason line.

When the headline says `nothing was checked`, say in one sentence that proob checked nothing in
this repository, and stop. For a summary in your own words, restate only what
`"$proob" shadow report --json` holds (also saved as
`report.json` beside `$work`), schema_version 1:

| field | meaning |
|---|---|
| `headline` | the first line of the terminal report |
| `verdict` | `{ "word": "Proposed", "why": "no one has approved it: shadow results are local" }` |
| `counts` | `tried`, `fails`, `passes`, `not_run`, `errored` |
| `properties[]` | `id`, `title`, `statement`, `verdict` (`Proposed`), `severity`, `rationale`, `result`, `strength`, `line`; failures first, most severe first |
| `properties[].result.state` | `no_test`, `not_run`, `passes` (`cases`, `seed`), `checks`, `fails` (`input`, `seed`), `errored` (`reason`) |
| `properties[].strength.state` | `not_scored`, `can_fail` (`killed`, `mutants`, `unsampled` when sampled), `cannot_fail` (`mutants`; every mutant in scope ran), `none_killed_in_sample` (`mutants`, `unsampled`: a sample killed none, which is not "cannot fail"), `nothing_to_mutate`, `errored` (`reason`) |
| `make_it_real[]` | the numbered steps, as printed |
| `adopted_branch` | the branch `proob shadow adopt` made, or null |

## Never

- Run `proob shadow adopt`, `git push`, or `gh pr create`. Those are the user's decisions; the
  report tells them how.
- Run `proob shadow clean` unless the user asks.
- Report a bug, risk, or finding that no check recorded (see the shadow rules).
- Write to, stage in, or commit in the user's repository.
