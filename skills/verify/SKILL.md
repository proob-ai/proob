---
name: verify
description: Measure whether a bound check can fail (mutation score via proob run score), explain the surviving mutants and what a stronger test needs, and for Verus-verified Rust or kind = "proof" properties propose the proof route (annotate the verified item, or draft a spec fn and ensures for review) and explain mutation-under-proof. Use when a property reads "Checked (tested, strength pending)" or "Checked (strength pending)", when the user asks how strong a test is, or when a core is under Verus.
argument-hint: "[id ...|all]"
allowed-tools: Read Grep Glob
---

# /proob:verify

Arguments: `$ARGUMENTS` (one or more property ids, or `all`; nothing means every bound property).

Verify answers one question per property: can this check fail? For a test, proob mutates the code
in the property's scope and runs only the bound test against each mutant. For a proof, proob
mutates the implementation under the proof and re-runs the verifier: a mutant the proof still
accepts is a specification that cannot tell right from wrong. You read the numbers the binary
prints, explain them, and propose stronger checks for the team to approve. You never claim a
result you did not see in the JSON, never claim `Holds`, and never lower `kill_floor`.

The binary is `"${CLAUDE_PLUGIN_ROOT}/scripts/proob"` (a shim that honours `PROOB_BIN`); if it
prints a line starting `proob:` and exits 1, stop and show the human that line.

## 1. Select the properties and the route

```
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" validate
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" status --json
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" hash <id> --json
```

For each property read its file (`statement`, `scope`, `kind`) and `hash <id> --json`
(`binding_hash`, `binding_files`, `scope_files`). Offline `status --json` rows carry
`status: null`, so bindings come from `hash`, not from `status`. Then route:

- `kind = "proof"`, or a scope file inside a crate whose `Cargo.toml` says
  `[package.metadata.verus] verify = true`: the proof route. Read `proof-route.md` in this
  directory and follow it; `run score` on such a binding is mutation-under-proof.
- `kind` absent or `"test"` and `binding_hash` present: the test route, steps 2 to 4.
- `kind` absent or `"test"` and `binding_hash: null`: nothing to measure. Say
  `Not checked (no check bound)` and point to `/proob:falsify <id>`.
- `kind = "model"` and `binding_hash` present (a TLA+ spec, with a `language = "tla"` adapter):
  the model route, in `proof-route.md`'s last section; `run score` mutates the spec, not the
  code. With `binding_hash: null`, point to `/proob:falsify <id>`, or with no tla adapter, to
  `proob init --tla`.

Never propose the proof route for a test-kind property in a crate that is not under Verus; the
human marks a core `kind = "proof"`, not you.

## 2. Run the score

```
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" run score <id> --json --record
```

Say before running that a score builds once per mutant and takes minutes on a small scope and
hours on a large one, and that the scheduled evidence run on the default branch is where the
score that counts comes from; a local score is a preview. One JSON line results; keep it
verbatim. `--record` appends it to `proob/.cache/results/results.jsonl` (untracked).

## 3. Explain the outcome, in proob's words

Quote `outcome` from the JSON. Per `outcome.result`:

- `scored` with `killed`, `survived`, `timeout`, `unviable`: say "the test killed K of M mutants
  in the scope", where M = `killed + survived + timeout`. `unviable` mutants did not compile and
  are outside M; they are not a weakness of the test, so say so when the number is not zero. A
  `survived` mutant compiled, changed the scoped code, and the bound test passed anyway: that is
  the weakness. A `timeout` is a mutant that made the test hang (an infinite loop in the mutant,
  usually), counted neither as a kill nor as a survivor. Compare `killed` with `kill_floor` in
  `proob/config.toml` (default 1): at or above it, the check can fail; below it, it cannot.
  When `unsampled` is present, the scope had more mutants than `max_mutants` (default 30 for
  rust and python, 25 for verus) and only a sample fixed by the property id ran: say "the test
  killed K of M sampled mutants (S in scope)", with S = M + `unviable` + `unsampled`, and that
  the kill floor was applied to the sample. Never call a sampled score exhaustive.
- `no_mutants`: nothing in the scope could be mutated (no `.rs` file, or only items cargo-mutants
  skips). The property stays `Checked (tested, strength pending)`, or `Checked (strength pending)`
  for a proof; the remedy is a scope that names the
  code the statement is about, which is an amendment to the property file for the human to make.
- `errored`: quote `outcome.reason`. "baseline failed" means the bound test does not pass before
  any mutation; run `/proob:falsify <id>` first. A missing `cargo-mutants` is an install hint.

For every score, say what it changes and what it does not: after the PR that bound the check
merges with an eligible review and the scheduled evidence run records a score at or above the
floor, a test-backed property moves from `Checked (tested, strength pending)` to
`Checked (tested)`, and no further: a test samples inputs and cannot prove, so strength never makes
it `Holds`. A proof-backed property moves from `Checked (strength pending)` to `Holds` only if the
team approved the statement, the check, the config, and the workflow (`proof-route.md`). With a
self-signed approval either reads `self-signed` as well. A local score moves nothing.

## 4. Name the survivors and what a stronger test needs

The score JSON carries counts, not the mutants. To talk about individual mutants, list them: read
`survivors.md` in this directory. For each mutant you believe survived, give the file and line,
the replacement cargo-mutants made, and the input on which the bound test would have to observe
a different value to kill it. Label that attribution as your reading of the list against the
test, not as the tool's verdict, since proob does not keep the per-mutant file.

A stronger test is a change to a bound file, so it reopens the binding's approval: offer it, and
on yes make the edit on a branch `proob/verify-<YYYY-MM-DD>`, rerun `run check` and `run score`,
and open a PR whose body carries both JSON lines and the same reviewer note as falsify's PR
(`../falsify/pr-template.md`): merging approves the amended check and its binding set, and the
property reads `Re-approve (check amended)` until then. Never lower `kill_floor` to make a weak
test clear the bar; that is a config amendment for the team, and the skill does not propose it.

## Words

Verdicts are only: Holds, Checked, Not checked, Re-approve, Broken, Proposed, Waived until d, with
modifiers in parentheses. Scores are "killed K of M"; proofs are `verified` or `proof failed`,
models `no violation` or `violation`, exactly as the JSON says; a local run is "local, unverified". Say "can fail" and "cannot fail",
not "strong" and "weak" as verdicts. A sample that killed nothing (`none_killed_in_sample`) is "not shown
able to fail", never "cannot fail": the mutants not run may be the ones the test reads.

## Never

- Claim a proof ran, verified, or failed unless `run check` on that binding printed it.
- Say a model's score, or its run, says anything about the code: it measures the spec.
- Say a property holds, or will hold, because of a local score.
- Lower `kill_floor`, raise `max_mutants` to hide unsampled mutants, or narrow a scope to shed
  mutants.
- Edit a file under `proob/properties/`; a `kind` change or a scope change is the human's PR.
- Present the mutant list as proob's survivor list.
