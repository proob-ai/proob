# The proof route

For a crate already under Verus, or a property the human marked `kind = "proof"`, the check is
the verifier and the strength measure is mutation-under-proof. You propose the binding; the team
approves it; proob runs it. You run nothing you then describe as a proof unless `run check`
printed `verified`.

Two verifiers take a proof: Verus, for a crate written in Verus (the sections below), and Kani,
for ordinary Rust when `proob/config.toml` has an adapter with `language = "kani"` (the last
section). Kani is Rust only; Python has no proof route in this release.

## What binds a proof

A comment-only line `// proob:<ULID>` directly above an item inside a `verus!` block, in a crate
whose `Cargo.toml` says `[package.metadata.verus] verify = true`, with `proob/config.toml`
carrying a `[[adapters]]` block `language = "verus"`, `runner = "cargo-verus"`,
`mutation_tool = "mutants-under-proof"`. The property file must say `kind = "proof"`; `validate`
reports a token on a `verus!` item whose property does not.

Bind the exec `fn` whose `ensures` states the property. A `proof fn` or `spec fn` can carry a
token too, but it has no implementation to mutate, so its score is `no_mutants` and the property
never clears the floor. From `tests/fixtures/verus-crate/clamp/src/lib.rs`:

```rust
verus! {
/// `x` brought into `[lo, hi]`.
// proob:01K8V5NQ4A0F3R7Y2M9D6P1XVA
pub fn clamp(x: i32, lo: i32, hi: i32) -> (r: i32)
    requires lo <= hi,
    ensures
        lo <= r <= hi,
        lo <= x <= hi ==> r == x,
        x < lo ==> r == lo,
        x > hi ==> r == hi,
{ … }
}
```

Property `01K8V5NQ4A0F3R7Y2M9D6P1XVA` reads "A clamped value lies between its bounds and equals
the input whenever the input already did": the first two `ensures` lines, in English.

The binding set is the whole verified crate's sources, its `Cargo.toml`, the workspace root
`Cargo.toml` when it pins `vstd`, `rust-toolchain.toml`, and the same files of every verified
path dependency (a trusted base). Any spec in the crate can weaken any proof in it, so the crate
is the unit of approval; after merge, an edit anywhere in it renders every proof-backed property
`Re-approve (check amended)`.

## Proposing the binding

1. Read the property's statement and scope. Find, in the scoped crate, the exec function whose
   `ensures` already says the statement. If one exists, propose `// proob:<id>` above it and
   explain which `ensures` lines carry the statement and which say more. Do not choose a function
   whose postcondition is weaker than the statement; that binds the property to a claim the proof
   does not make.
2. If no item states the property, draft one for review: a `spec fn` that names the statement
   over the function's arguments and result, and an `ensures` line on the exec function that
   calls it. Mark the draft as unverified and unrun; say what the proof will need (loop
   invariants, lemmas, a `requires` the callers already satisfy) and that the team's Verus
   maintainer decides whether to adopt it. Never write `assume`, `admit`, `external_body`, or
   `assume_specification` to make a draft go through; under `--no-cheating` proob's run rejects
   them anyway.
3. If the crate is not under Verus and the property says `kind = "proof"`: with a `kani` adapter,
   the property is Kani's, and `/proob:falsify <id>` writes its harness. Otherwise say that the
   route needs `verify = true` on the crate and a `verus` adapter in `proob/config.toml`, or a
   `kani` adapter (`proob init --kani`), all human decisions, and that until then the property
   renders `Not checked (no check bound)`. Offer the draft from step 2 as text in your reply or
   in a PR comment, not as a token in the tree.
4. Open the PR on a branch `proob/verify-<YYYY-MM-DD>` with the token and, when drafted, the spec;
   the body uses `../falsify/pr-template.md` with "Check" naming the item
   (`<package> <module path>::<fn>`) and the local result from step 5, or "not run" when you could
   not run it.

## Running it

```
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" run check <id> --json --record
"${CLAUDE_PLUGIN_ROOT}/scripts/proob" run score <id> --json --record
```

`check` runs `cargo verus verify` on the crate under `--no-cheating` with the pinned `--rlimit`,
then attributes the bound item with `--verify-function`. The JSON `outcome.result` is `verified`
(with `rlimit`), `proof_failed` (with `diagnostics`), or `errored`; `tool_versions` records
`verus`, `verus-commit`, and `z3`. Quote it. A `verified` here is a local run, unverified, like
every other local result; the words for it are "verified locally (Verus <version>, rlimit <n>):
unverified until the default-branch run", never `Holds`. `proof_failed` is a counterexample's
cousin: quote the diagnostics and offer the two exits (fix the code or the proof; or amend the
statement in its own PR). Do not weaken the `ensures`.

`score` is mutation-under-proof, operator set `mutants-under-proof-v2`: the bound function's
executable body is mutated one operator at a time (relational and arithmetic swaps, `&&`/`||`,
dropped `!`, flipped literals, swapped `match` arms and `if` branches, dropped returns, negated
results), its ghost code and specification left alone, and each mutant is verified the same way.
Read the JSON as:

- `killed`: the verifier rejected the mutant. The specification can fail.
- `survived`: the verifier accepted the mutant. The specification admits an implementation that
  differs from the real one on that line, so it cannot tell them apart. This is the question
  "can this specification fail?" answered no for that mutant.
- `unviable`: rustc or Verus could not check the mutant at all; excluded from the denominator.
- `timeout`: the resource limit or the wall clock ran out.
- `unsampled`: mutants beyond `max_mutants` (config, default 25) that were listed and never run.
  Say the number; do not raise the cap to make it disappear, and do not call a score complete
  when it is present.

The fixture's `smaller` (`ensures true`) scores `killed 0`: every mutant verifies, because a
postcondition that says nothing cannot fail. `clamp` kills the bound swaps because its `ensures`
pins the result. A stronger specification is more `ensures`, each line of which a mutant should
break; propose those lines and leave the adoption to the team.

## What a proof-backed property can read

`Not checked` until the binding PR merges and the push run records `verified`; then
`Checked (strength pending)` until the scheduled run records a score at or above `kill_floor`;
then `Holds` if the team approved the statement, the binding set, the config, and the workflow,
or `Checked (self-signed)` when the only approval is the author's own. A local `verified` and a
local score change none of this. This is the route to `Holds`: a test-backed property, however
strong, stops at `Checked (tested)`.

## Kani: a harness on ordinary Rust

`/proob:falsify` writes and binds the harness (its `conventions.md` has the convention); verify
reads the run and the score. A harness is checked for every input it admits and no other, so a
Kani result always travels with its bounds: the `kani::assume`s, size bounds, and unwind bounds
the binding PR's "Proved for" block lists.

`check` runs `cargo kani` on the one harness with Kani's own nightly. `outcome.result` is
`verified` (with `checks`), `proof_failed` (with `diagnostics`: the failed check, its location),
or `errored` (with `reason`); `tool_versions` records `kani`, `cbmc`, and `nightly`. The words
for a local `verified` are "verified locally within the harness's bounds (<checks> checks):
unverified until the default-branch run". A `proof_failed` is a bug, read as a counterexample:
quote the diagnostics, offer the two exits. An `errored` reason says what to do next; falsify's
step 5 has the table.

`score`, operator set `cargo-mutants-under-kani-v1`, applies sampled cargo-mutants mutants of the
property's scope (never of harness code) and re-runs the one harness on each, `max_mutants`
(default 30; 10 in `/proob:try`) per score. Read the JSON as:

- `killed`: an assertion written in the harness failed on the mutant. The harness tells the
  mutant from the real code.
- `survived`: the harness verified the mutant, or the only check that failed was one of Rust's or
  Kani's own (arithmetic overflow, an index out of bounds, a panic in another function). Those
  count as survived: the statement is not "the code does not panic", and a harness whose own
  assertions let the mutant through has not shown it can tell the mutant apart. A harness that
  only trips such checks on mutants (the fixture's `clamp_indexes_a_table`) reads `killed 0`;
  so does one that asserts only what it assumed (`clamp_runs_with_ordered_bounds`). The remedy is
  an assertion of the statement, not a weaker bound.
- `timeout`: the mutant hit the unwind bound, left checks undetermined, ran out of memory, or ran
  past the per-mutant limit (five times the baseline run); neither a kill nor a survivor.
- `unviable`: Kani could not check the mutant at all (it did not compile, reached an unsupported
  construct, made a cover unreachable, or the harness was not found); outside the denominator.
- `unsampled`: as for Verus.

A Kani-bound property reads as above: `Not checked`, then `Checked (strength pending)`, then
`Holds`, and a Kani `Holds` means within the harness's bounds: inputs outside them were not
checked.
