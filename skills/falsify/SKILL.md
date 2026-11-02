---
name: falsify
description: Bind an approved property to one property-based test (proptest for Rust, Hypothesis for Python) in the team's own test layout, or a kind = "proof" Rust property to one Kani harness with its bounds traced to the statement, or a kind = "model" property to an invariant of a TLA+ spec of the design the code implements, run it through proob, and open a PR whose merge approves the check. When Kani cannot take a property for a reason it names, land on a test and show the reviewer why. Use when a property reads "Not checked (no check bound)", when the user asks to write, bind, or falsify a property test, or after /proob:declare has merged.
argument-hint: "[id ...|all]"
allowed-tools: Read Grep Glob
---

# /proob:falsify

Arguments: `$ARGUMENTS` (one or more property ids, or `all`; nothing means every property no check binds).

You write the check; the binary runs it; the team approves it. You never approve anything, never
flip a status, and never make a check pass by weakening it or by editing the property it binds.
The one edit you may make to a property file is a Kani fallback's `kind` (step 5K).

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
  or `"test"`, and every `kind = "proof"` property whose scope is ordinary Rust when
  `proob/config.toml` has an adapter with `language = "kani"`: that one gets a Kani harness
  (step 3K), and every `kind = "model"` property when `proob/config.toml` has an adapter with
  `language = "tla"`: that one gets a TLA+ spec (step 3M). A `kind = "proof"` property on a crate
  under Verus, or with no kani adapter, is `/proob:verify`'s proof route; a `kind = "model"`
  property with no tla adapter stays unbound until the human opts in with `proob init --tla`:
  name them and move on. Kani is Rust only: never write a harness, or claim a proof, for Python.
- An explicit id that is already bound gets no second test: skip to step 4 and run its check.
- If `proob/config.toml` lists no adapter for the repo's language, every property renders
  `Not checked (no adapter)`; say so and stop. Do not fake a check in another language. A tla
  adapter is the exception for `kind = "model"` properties: a TLA+ spec models code in any
  language, so bind those (step 3M) even when the code's own language has no adapter.

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
  `Checked (tested, strength pending)`.
- No seed, no case count, no `ProptestConfig`, no `@settings(max_examples=…)`, no `derandomize`,
  no `#[ignore]`, no skip marker. proob passes the seed and budget to the runner itself; anything
  you hard-code is overridden or, worse, hides a counterexample from the seed that replays it.
- Place it where the team places tests of that module (the same `tests/` crate or `#[cfg(test)]`
  module, the same pytest package), reusing their strategy helpers. Do not create a new test
  layout for proob.
- Assertion messages name the inputs and the observed value, so the shrunk witness reads on its
  own.

## 3K. Or write one Kani harness

For a `kind = "proof"` Rust property, read the Kani section of `conventions.md` first. The
harness is a proof over every input it admits, and only those, so its bounds are what the team
approves:

- One `#[kani::proof]` fn in a `#[cfg(kani)]` module of the library or binary crate (under
  `src/`, not `tests/`), with `// proob:<ULID>` directly above it.
- Inputs from `kani::any()`. Every `kani::assume` is a precondition the statement states, and
  you can quote the words it comes from. An assume the statement does not state narrows the
  proof: avoid it, and when you cannot, say so in the PR's "Proved for" block.
- A size bound (a slice length, a collection size) is a named constant with a comment saying why
  that size; it narrows the proof too, and the "Proved for" block says so.
- A `kani::cover!` for each boundary case the statement names (equal bounds, an empty input, the
  value at a limit), so a harness that cannot reach the case fails as `vacuous` instead of
  verifying nothing.
- Assertions encode the statement, as a test's would. `#[kani::unwind(N)]` only when a loop needs
  it, with N derived from the size bound. No stubs, no contracts (`proof_for_contract`), no
  `#[kani::should_panic]`.

## 3M. Or write one TLA+ model

For a `kind = "model"` property, read the TLA+ section of `conventions.md` first. A model checks
the design the scoped code implements, not the code: TLC explores every state of your spec over
the constants in its `.cfg`, and nothing else. So the spec, its constants, and what it leaves
out are what the team approves:

- Read the scoped code until you can name its protocol: the processes or actors, the state each
  keeps, the messages or steps that change it, what can interleave, fail, or repeat. Model what
  the code does, not what the statement hopes; a spec of the intended design that the code does
  not follow checks nothing the team ships.
- Write `proob/specs/<Module>.tla` and `proob/specs/<Module>.cfg` beside it. One module per
  protocol: properties about the same protocol share it, each with its own token above its own
  definition, so the reviewer reads one model of one design. A second, unrelated protocol gets
  its own module. Say which you chose and why in the PR.
- Every action gets a TLA+ comment citing the code it models (`\* models lockd/server.py:14-22,
  handle ACQUIRE`). A step of the code you abstracted away, merged, or left out is named in a
  comment too.
- The bound definition is the statement's design invariant (`MutualExclusion == …`) or temporal
  property, written at the start of a line with no parameters, with `\* proob:<ULID>` directly
  above it. It says what the statement says and nothing weaker: an invariant implied by the
  type alone, or one no action can reach, checks nothing.
- Constants small enough for TLC to finish in minutes, large enough to reach the case the
  statement is about (two processes for mutual exclusion, at least one retry for idempotency, a
  crash for recovery). Next to each constant in the `.cfg`, a `\*` comment says why that value.
  The `.cfg` names `SPECIFICATION` (or `INIT` and `NEXT`), the constants, and the bound name
  under `INVARIANT` or, for a temporal property, `PROPERTY`.

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
0 passed, verified, or no violation; 1 counterexample, proof failed, or violation; 2 errored or
usage. A Kani run compiles the crate under Kani's own nightly and can take minutes. A TLC run
needs `java` on `PATH` and `PROOB_TLA2TOOLS` naming a `tla2tools.jar`; proob copies the spec to a
temporary directory and writes nothing into the repository.

The result that counts is the one `run check` gives from `proob/config.toml` as it will be
committed: the gate and the evidence run replay that and nothing else. A Rust test that compiles
only with a cargo feature (a `#![cfg(feature = "…")]` file, a `[[test]]` with
`required-features`, an optional `proptest` dependency; `proob init` leaves a comment in the
adapter block when it sees one) errors as "bound test not found" until the adapter turns that
feature on. Then set it in the adapter block, `features = ["<name>"]` and no wider (not
`all_features`), rerun `validate` and `run check`, and commit the change with the test (step 6).
Never run the check in a scratch copy, or with flags of your own, and report that as its result.

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

For a Kani harness:

- `verified` (with `checks`): "verified locally within the harness's bounds (`outcome.checks`
  checks): unverified until the default-branch run". Never "proved", "proven", or "holds": the
  property reaches `Holds` only after review approves the harness and its bounds and the
  default-branch run records `verified` and a score at the floor.
- `proof_failed`: Kani found an input that breaks the statement. It is a bug, reported exactly
  as a counterexample above: quote `outcome.diagnostics` verbatim (the failed check and its
  location), give the two exits, and stop; until an exit is taken the gate shows the property as
  `Broken (proof failed on this PR)`. Never fall back to a test from it, even though a test
  might pass and turn the PR green: a sampled test that misses the input hides a known failure.
- `errored`: quote `outcome.reason`, then read its first words:

| `outcome.reason` begins | What you do |
|---|---|
| "Kani cannot verify a construct this code reaches: …" | fall back (5K) |
| "the harness's unwind bound is too small for a loop: …" | retry once with a larger `#[kani::unwind(N)]`; fall back (5K) if the retry's reason begins the same |
| "Kani timed out …" | fall back (5K) |
| "Kani ran out of memory" | fall back (5K) |
| "vacuous: the harness cannot reach a case it covers: …" | fix the harness: an assume or a bound excludes a case the statement names. Never delete the cover |
| "nothing was checked: …" | fix the harness: assert what the statement says |
| "the crate did not compile under Kani: …" | fix the harness if your code is wrong; otherwise stop and quote it |
| "harness not found: …" | fix the token's placement or the harness path |
| "the Kani toolchain was not found …" | stop and quote it: the install is the human's, and `/proob:setup` does it with their approval |
| anything else ("Kani could not decide …", "unrecognised Kani output: …") | stop and quote it |

Never shrink a size bound or add an assume to get past an unwind failure, a timeout, or out of
memory: that changes what is proved. One more fallback needs no run: a harness that cannot be
written without a feature Kani does not support (name it, citing
https://model-checking.github.io/kani/rust-feature-support.html).

For a TLA+ model:

- `no_violation`: "model-checked locally over the constants in the cfg: unverified until the
  default-branch run". Name the constants. Never "holds", "verified", "proved", and never say
  the code is correct or conforms to the spec: TLC checked the spec, not the code.
- `violation`: quote `outcome.trace` verbatim (the violated property and the trace's first and
  last states), then say in one plain sentence what the trace does in the scoped code's terms.
  Give the human exactly two exits: the design is wrong (fix the code, and the spec with it, so
  the action models what the code now does), or the statement is wrong (amend it as a new PR to
  the property file, which re-enters review). Then stop. Until an exit is taken the gate shows the
  property as `Broken (violation on this PR)` on any PR that carries the binding. Never weaken
  the invariant, drop an action, or shrink a constant so the trace disappears.
- `errored`: quote `outcome.reason`, then read its first words:

| `outcome.reason` begins | What you do |
|---|---|
| "TLC could not parse the spec: …", "TLC rejected the spec (semantic error): …" | fix your spec's syntax or names; never by removing what the invariant reads |
| "deadlock reached: …" | read the trace: if the code really stops there (every request served, the protocol over), add `CHECK_DEADLOCK FALSE` to the cfg and say so in the PR; if the code would go on, your spec is missing an action: add it |
| "TLC timed out …", "TLC used more than … MB …", "TLC ran out of Java heap …" | the state space is too large for these constants. You may propose smaller ones, but a smaller constant is a narrower claim: say in "Model-checked for" which one shrank, from what, and what it leaves out. Never shrink one below the case the statement is about |
| "TLC was not found …" | stop and quote it: Java and the jar are the human's install, and `/proob:setup` does it with their approval |
| anything else ("TLC reported invariant … violated, but only … was checked", "unrecognised TLC output: …", "TLC was killed …", "TLC did not run: …") | stop and quote it |

A model has no fallback: if TLC cannot take the spec, say so and leave the property unbound.

## 5K. Fall back to a test

Only for the reasons above. In this order:

1. Keep the harness's text and the `outcome.reason` line (and the retry's, for unwinding) for
   the PR.
2. Remove the harness and its token.
3. In the property file change `kind = "proof"` to `kind = "test"`, and nothing else. This
   re-opens the statement's approval: the reviewer approves the change of kind with the PR.
4. Write a property-based test for the statement (step 3), run `validate` and `run check` (step
   4), and report it (step 5).
5. Fill the PR's "Fell back to a test" block (`pr-template.md`).

After merge the property reads `Checked (tested)` at best.

## 6. Open the pull request

In shadow (the file `.proob-shadow` exists at the root): skip branch, commit, push, and PR; print
nothing about reviewers; the files stay in shadow.

Branch `proob/falsify-<YYYY-MM-DD>`; commit only the test, harness, and spec files (a model's
`.tla` modules and `.cfg` under `proob/specs/`), plus a fallback's
property file and a `features` line you added to `proob/config.toml` (step 4; nothing else in
that file, never any other file under `proob/properties/`, never `proob/.cache/`). Use
`gh pr create` with the body in `pr-template.md` in this directory: one row per binding with the
statement, the check reference from the run's text output, the local result labelled unverified,
and the binding set, plus the reviewer note that merging approves the check and its binding set;
a "Proved for" block per Kani harness, a "Model-checked for" block per TLA+ module, a "Fell back
to a test" block per fallback, and a "Config change" block when you set `features`. If the
human asked for no PR, print the body instead.

## The binding set, explained to the reviewer

Approval attaches to content hashes. For a Rust test the binding set is the test file, every file
its `mod` declarations reach (the shared `tests/common/mod.rs` included), and
`.config/nextest.toml` and `.cargo/mutants.toml` when they exist; `Cargo.toml` and `Cargo.lock` are
not members, so a dependency bump does not reopen the approval. `hash <id> --json` lists the exact
members under `binding_files`. After the PR merges, any edit to a member changes the binding hash
and the property renders `Re-approve (check amended)` until a reviewer approves the amendment;
the statement's own approval is untouched. Say this in the PR so nobody edits a strategy helper
expecting nothing to move.

For a Kani harness the binding set is the harness's file and the files its `#[cfg(kani)]` `mod`
declarations reach, the package `Cargo.toml`, the workspace root `Cargo.toml`, and `Cargo.lock`;
the code under proof is the property's scope, hashed with the statement.

For a TLA+ model the binding set is the module, its `.cfg`, and every module beside it that it
reaches through `EXTENDS` or `INSTANCE`; an edit to any of them reads `Re-approve (check
amended)`. The code the spec models is the property's scope, and it is watched differently: any
commit to a scope file after the later of the statement's and the spec's approving merges reads
`Re-approve (spec drifted: n commits to the modelled scope)`, because nothing checks that
changed code still follows the spec. A new approval of either clears it: a PR that amends the
spec when the code changed the design, or a re-affirmation of the statement (`/proob:guard`
proposes one) when it did not, each after the reviewer re-reads the spec against the code.
Merging this PR approves the spec, so drift counts from its merge. Drift is file-granular: a refactor flags it as surely as a change of
behaviour. Say this in the PR, so the team knows that touching the modelled code asks them to
re-read the spec.

## Words

Verdicts are only: Holds, Checked, Not checked, Re-approve, Broken, Proposed, Waived until d, with
modifiers in parentheses (`Checked (tested, strength pending)`, `Not checked (no check bound)`). A
property you bound today is `Not checked` until the PR merges with an eligible review and the
default-branch run produces evidence, then `Checked (tested, strength pending)` until a score run
shows the test can fail, then `Checked (tested)`. That is as far as a test goes: it samples inputs
and cannot prove, so `tested` is not a defect. `Holds` is the proof route's word, earned by a proof
or a model on the default branch, never yours for a local run. A Kani-bound property is
`Not checked` until the PR merges and the default-branch run records `verified`, then
`Checked (strength pending)` until a score run at the floor, then `Holds` if the team approved
it: `Holds` within the harness's bounds, the ones the "Proved for" block lists. A model-bound
property is `Not checked` until the PR merges and the default-branch run records `no_violation`,
then `Checked (strength pending)` until a score of spec mutants reaches the floor, then `Holds`
if the team approved it: `Holds` for the design over the constants the "Model-checked for" block
lists, never for the code. Any later commit to its scope reads `Re-approve (spec drifted)`.

## Never

- Edit a file under `proob/properties/`, except a fallback's `kind` (step 5K), or write a waiver.
- Fall back to a test from `proof_failed`, a missing toolchain, an ordinary compile error, or a
  vacuous cover.
- Weaken a test, narrow a generator, or skip a case to turn a counterexample into a pass.
- Hard-code a seed, a case count, or a settings profile in a bound test.
- Bind one property to two tests, or two properties to one test.
- Weaken an invariant, drop an action, or shrink a constant to turn a violation into a pass; or
  say the code conforms to, implements, or is verified by a spec.
- Report a check's result from a scratch copy or from cargo flags the committed config does not
  set, or change anything in `proob/config.toml` but an adapter's `features`.
- Say a property holds, is proved, is verified, or is approved because of anything that ran
  here; a Kani result is "verified locally within the harness's bounds", a TLC result
  "model-checked locally over the constants in the cfg", and nothing more.
