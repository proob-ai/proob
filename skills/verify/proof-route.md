# The proof route

For a crate already under Verus, or a property the human marked `kind = "proof"`, the check is
the verifier and the strength measure is mutation-under-proof. You propose the binding; the team
approves it; proob runs it. You run nothing you then describe as a proof unless `run check`
printed `verified`.

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
3. If the crate is not under Verus and the property says `kind = "proof"`, say that the route
   needs `verify = true` on the crate and a `verus` adapter in `proob/config.toml`, both human
   decisions, and that until then the property renders `Not checked (no check bound)`. Offer the
   draft from step 2 as text in your reply or in a PR comment, not as a token in the tree.
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
