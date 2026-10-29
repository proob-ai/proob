---
name: falsify
description: Bind an approved property to one property-based test (proptest for Rust, Hypothesis for Python) in the team's own test layout, run it through proob with the adapter's seed and budget, and open a PR whose merge approves the check. Use when a property reads "Not checked (no check bound)", when the user asks to write, bind, or falsify a property test, or after /proob:declare has merged.
argument-hint: "[id ...|all]"
allowed-tools: Read Grep Glob
---

# /proob:falsify

Arguments: `$ARGUMENTS` (one or more property ids, or `all`; nothing means every property no check binds).

You write the check; the binary runs it; the team approves it. You never approve anything, never
flip a status, and never make a test pass by weakening it or by editing the property it binds.

The binary is `"${CLAUDE_PLUGIN_ROOT}/scripts/proob"` (a shim that honours `PROOB_BIN`). Call it
that way in every command below; if it prints a line starting `proob:` and exits 1, stop and show
the human that line. Every result from this session is a local run: proob's own text calls it
"local run, unverified (not evidence)", and so do you.

## 1. Select the properties

```
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" validate
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" status --json
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" hash <id> --json
```

- `validate` must pass before you start; a problem it names is the human's to fix unless it is a
  token you wrote in this session.
- `status --json` lists every property under `.properties[]` (`id`, `path`, `title`, `kind`,
  `word`, `reason`). Rows carry a derived `status` only when GitHub was reachable; offline they
  read `Not checked` with reason `no GitHub token` and `status: null`, which says nothing about
  bindings. So decide bindings with `hash <id> --json`: `binding_hash: null` and an empty
  `binding_files` mean no check binds the property.
- With no argument or `all`, take every property with `binding_hash: null` whose `kind` is absent
  or `"test"`. A `kind = "proof"` property is `/proob:verify`'s proof route; a `kind = "model"`
  property has no adapter in this release: name both and move on.
- An explicit id that is already bound gets no second test: skip to step 4 and run its check.
- If `proob/config.toml` lists no adapter for the repo's language, every property renders
  `Not checked (no adapter)`; say so and stop. Do not fake a check in another language.

## 2. Read before writing

For each property, read in this order: the property file (statement, scope, rationale, kind);
`scope_files` from `hash --json` and the source they name; the tests that already sit next to the
scoped module (layout, naming, shared strategy helpers such as `tests/common/mod.rs`); the adapter
block of `proob/config.toml` (`language`, `root`). A statement is one plain sentence; your test is
its translation, so understand the code's actual domain (types, ranges, invariants the callers rely
on) before choosing generators.

## 3. Write one test

Read `conventions.md` in this directory for the exact binding convention and a correct and a
vacuous example per language. The rules that hold in every language:

- One property, one test, one token: the line `// proob:<ULID>` (Rust) or `# proob:<ULID>`
  (Python) directly above the test's attributes or decorators, holding that id and nothing else.
- The test encodes the statement and nothing weaker. If the statement says "never", the assertion
  must fail on one violation; if it quantifies over a domain, the generators cover that domain
  (negative numbers, empty collections, equal bounds, the full range the types admit) rather than
  the comfortable middle of it.
- The test reads the code in its scope. A test that cannot fail when the scoped code is wrong is
  vacuous; verify will score it `killed 0` and the property will never leave
  `Checked (strength pending)`.
- No seed, no case count, no `ProptestConfig`, no `@settings(max_examples=…)`, no `derandomize`,
  no `#[ignore]`, no skip marker. proob passes the seed and budget to the runner itself; anything
  you hard-code is overridden or, worse, hides a counterexample from the seed that replays it.
- Place it where the team places tests of that module (the same `tests/` crate or `#[cfg(test)]`
  module, the same pytest package), reusing their strategy helpers. Do not create a new test
  layout for proob.
- Assertion messages name the inputs and the observed value, so the shrunk witness reads on its
  own.

## 4. Run it

```
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" validate
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" run check <id> --json --record
```

`validate` catches a malformed or misplaced token (`binding token … is not proob:<ULID>`, `names no
property`, `is already bound at`); fix the token, not the property. `run check` compiles and runs
the one bound test through the language runner with the adapter's seed and budget, prints one JSON
line, and with `--record` appends the same line to `proob/.cache/results/results.jsonl` (an
untracked file). Keep the line verbatim; quote from it rather than retyping numbers. Exit codes:
0 passed, 1 counterexample, 2 errored or usage.

## 5. Report the outcome, in proob's words

Quote the JSON `outcome` and `replay` fields. The wording per `outcome.result`:

- `no_counterexample`: "passed locally (seed `replay.seed`, `outcome.budget` cases): unverified
  until the default-branch run". Not "holds", not "verified", not "proven". A local pass is a
  reason to open the PR, nothing more.
- `counterexample`: give `outcome.witness` and `replay.seed` verbatim, the replay command
  `proob run check <id> --seed <seed>`, and exactly two exits for the human: fix the code, or amend
  the statement as a new PR to the property file, which re-enters review. Then stop. You do not
  pick an exit, do not edit the property file, do not loosen the generators or the assertion, and
  do not add `#[ignore]`. Until an exit is taken the gate will show this property as
  `Broken (counterexample on this PR)` on any PR that carries the binding.
  Then say, in one plain sentence, whether the input is one the code meets in real use. When it
  only fails on an extreme input (a timestamp near 2^53, an infinity or NaN, a megabyte-long
  string, a count no caller can reach), say "fails only on an extreme input" and name the
  realistic range the callers or the docs imply; the human decides whether that range belongs in
  the statement. Do not narrow the generator yourself: an unexpected input is exactly what a
  property test is for.
- `errored`: quote `outcome.reason`. A missing tool, a wrong toolchain, or a test that did not
  compile is an environment or syntax problem you may fix; the test's strength is not.

## 6. Open the pull request

In shadow (the file `.proob-shadow` exists at the root): skip branch, commit, push, and PR; print
nothing about reviewers; the files stay in shadow.

Branch `proob/falsify-<YYYY-MM-DD>`; commit only the test files (never anything under
`proob/properties/`, never `proob/.cache/`). Use `gh pr create` with the body in `pr-template.md`
in this directory: one row per binding with the statement, the check reference from the run's text
output, the local result labelled unverified, and the binding set, plus the reviewer note that
merging approves the check and its binding set. If the human asked for no PR, print the body
instead.

## The binding set, explained to the reviewer

Approval attaches to content hashes. For a Rust test the binding set is the test file, every file
its `mod` declarations reach (the shared `tests/common/mod.rs` included), and
`.config/nextest.toml` and `.cargo/mutants.toml` when they exist; `Cargo.toml` and `Cargo.lock` are
not members, so a dependency bump does not reopen the approval. `hash <id> --json` lists the exact
members under `binding_files`. After the PR merges, any edit to a member changes the binding hash
and the property renders `Re-approve (check amended)` until a reviewer approves the amendment;
the statement's own approval is untouched. Say this in the PR so nobody edits a strategy helper
expecting nothing to move.

## Words

Verdicts are only: Holds, Checked, Not checked, Re-approve, Broken, Proposed, Waived until d, with
modifiers in parentheses (`Checked (strength pending)`, `Not checked (no check bound)`). A property
you bound today is `Not checked` until the PR merges with an eligible review and the default-branch
run produces evidence, then `Checked (strength pending)` until a score run shows the test can fail.
`Holds` is the binary's word for the default branch, never yours for a local run.

## Never

- Edit a file under `proob/properties/`, or write a waiver.
- Weaken a test, narrow a generator, or skip a case to turn a counterexample into a pass.
- Hard-code a seed, a case count, or a settings profile in a bound test.
- Bind one property to two tests, or two properties to one test.
- Say a property holds, is verified, or is approved because of anything that ran here.
