# Binding conventions, by language

The adapter owns the convention; this file restates it with one correct and one wrong example
each. `proob validate` is the authority on the token's grammar and placement; the adapter's
`discover` is the authority on what the token binds.

The token is `proob:<ULID>`: the 26-character id from the property file name, nothing after it.
`proob:<ULID>:v2`, a truncated id, and an id that names no property are all `validate` errors.
Each property binds at most one check and each check carries at most one token.

## Rust: proptest through cargo-nextest

A line comment holding only the token, directly above the test's attributes. Blank lines, other
comments, and attributes may stand between the token and the `fn`; the attributes must include
`#[test]` (or `#[…::test]`, `#[proptest]`). Inside a `proptest!` block, or on a `#[proptest]`
function from the `test-strategy` crate: either way.

Correct (from `tests/fixtures/rust-repo/tests/props.rs`):

```rust
mod common;                       // shared strategies: a member of the binding set

use proptest::prelude::*;
use rust_repo::ordering::sorted;

proptest! {
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHA
    #[test]
    fn sorted_preserves_length(v in common::small_vec()) {
        prop_assert_eq!(sorted(&v).len(), v.len());
    }
}
```

Vacuous (from `tests/fixtures/rust-repo/tests/vacuous.rs`): bound correctly, runs green, and
never reads the code in its scope, so every mutant of `src/parity.rs` survives it and verify
scores it `killed 0`:

```rust
proptest! {
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHD
    #[test]
    fn evens_are_counted_vacuously(v in common::small_vec()) {
        prop_assert!(v.len() <= 8);   // true of the generator, says nothing about count_evens
    }
}
```

Forbidden inside a bound test:

```rust
proptest! {
    #![proptest_config(ProptestConfig { cases: 1000, .. ProptestConfig::default() })]  // no
    // proob:…
    #[test]
    #[ignore]                                                                           // no
    fn …
}
```

Why: `run check` sets `PROPTEST_RNG_SEED` and `PROPTEST_CASES` in the runner's environment and
disables failure persistence, so a run is reproducible from the seed in the JSON alone. A
`ProptestConfig` in the source overrides the environment and breaks replay; `#[ignore]` makes
nextest skip the test, which the adapter reports as `errored`.

Check reference: nextest's own name, `<binary id> <test path>`, e.g.
`rust-repo::props sorted_preserves_length`; `run check` prints it in its text output.

Binding set: for a test in an integration-test crate (`tests/x.rs` or `tests/x/main.rs`), every
file of that crate (the root plus everything its `mod` declarations reach, transitively, `#[path]`
included); for a test in a `src/` file, that file plus the files its `#[cfg(test)]` `mod`
declarations reach; plus `.config/nextest.toml` and `.cargo/mutants.toml` at the adapter root when
present. Not `Cargo.toml`, not `Cargo.lock`. Hashed as `proob-binding-v1`.

A crate whose `Cargo.toml` says `[package.metadata.verus] verify = true` belongs to the Verus
adapter: its `src/` holds proofs and is not scanned for tests; its `tests/` crates still are.

## Kani: a harness, for a `kind = "proof"` Rust property

A line comment holding only the token, directly above a `#[kani::proof]` fn (or
`#[cfg_attr(kani, kani::proof)]`), in a `#[cfg(kani)]` module of the library or binary crate.
A harness under `tests/` binds nothing, and a `#[kani::proof_for_contract]` harness is reported
unbound ("contracts are not supported yet"). The property must say `kind = "proof"`, and
`proob/config.toml` must carry an adapter with `language = "kani"`.

Correct (after `tests/fixtures/kani-crate/src/lib.rs`, statement "For ordered bounds, a clamped
value lies between them"):

```rust
#[cfg(kani)]
mod verification {
    use super::clamp;

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKB
    #[kani::proof]
    fn clamp_stays_within_bounds() {
        let (x, lo, hi): (i32, i32, i32) = (kani::any(), kani::any(), kani::any());
        kani::assume(lo <= hi);                     // "for ordered bounds"
        kani::cover!(lo == hi, "equal bounds");     // ordered includes equal
        kani::cover!(x < lo, "below the bounds");
        kani::cover!(x > hi, "above the bounds");
        let y = clamp(x, lo, hi);
        assert!(lo <= y && y <= hi, "clamp({x}, {lo}, {hi}) = {y}");
    }
}
```

Every `i32` triple with `lo <= hi` is checked, which is what the statement says and no less.

With a size bound (statement "The number of even values in a list is at most the number of
values in it"):

```rust
    /// Lists of up to 8 values: each extra value is one more loop iteration for Kani to unroll.
    const MAX_LEN: usize = 8;

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHC
    #[kani::proof]
    #[kani::unwind(9)]                              // MAX_LEN + 1: one per value, one to exit
    fn evens_never_exceed_the_length() {
        let values: [u32; MAX_LEN] = kani::any();
        let len: usize = kani::any();
        kani::assume(len <= MAX_LEN);               // the size bound, not the statement's
        let v = &values[..len];
        kani::cover!(len == 0, "an empty list");
        assert!(count_evens(v) <= v.len());
    }
```

The statement says "a list", any length; this proves it for lists of at most 8. That is a
narrowing the reviewer must see: the PR's "Proved for" block lists `MAX_LEN = 8` and says what it
leaves out.

Narrowed: verifies, and proves far less than the statement promises. Nothing in "for ordered
bounds" says the bounds are small or the input is between them:

```rust
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKB
    #[kani::proof]
    fn clamp_stays_within_bounds() {
        let (x, lo, hi): (i32, i32, i32) = (kani::any(), kani::any(), kani::any());
        kani::assume(-100 <= lo && lo <= hi && hi <= 100);  // no: the statement admits every i32
        kani::assume(lo <= x && x <= hi);                   // no: excludes the clamping cases
        assert!(lo <= clamp(x, lo, hi) && clamp(x, lo, hi) <= hi);
    }
```

Vacuous (from `tests/fixtures/kani-crate/src/lib.rs`): verifies, and asserts only what it
assumed, so every mutant of `clamp` survives and verify scores it `killed 0`:

```rust
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKK
    #[kani::proof]
    fn clamp_runs_with_ordered_bounds() {
        let (x, lo, hi): (i32, i32, i32) = (kani::any(), kani::any(), kani::any());
        kani::assume(lo <= hi);
        let _ = clamp(x, lo, hi);
        assert!(lo <= hi);                          // true of the assume, says nothing about clamp
    }
```

A cover the harness cannot reach (`kani::cover!(y < lo, …)` after the assume above, in the
fixture's `clamp_covers_an_unreachable_case`) makes the run `errored` with "vacuous: the harness
cannot reach a case it covers: …"; fix the assume or the bound, never the cover.

Forbidden in a bound harness: `kani::assume` of anything the statement does not state, unless
the PR flags it; `#[kani::stub]` and `#[kani::proof_for_contract]` (stubs and contracts are not
used in this release); `#[kani::should_panic]`; `#[kani::unwind]` on a harness with no loop;
`kani::assume(false)` or any assume that leaves no input.

Check reference: `<package> <module::path::harness>`, e.g. `kani-crate
verification::clamp_stays_within_bounds`; `run check` prints it in its text output.

Binding set: the harness's file and the files its `#[cfg(kani)]` `mod` declarations reach, the
package `Cargo.toml`, the workspace root `Cargo.toml` when there is a workspace, and `Cargo.lock`.

## TLA+: a model, for a `kind = "model"` property

A line `\* proob:<ULID>` holding only the token, directly above the definition it binds: an
invariant or temporal property with no parameters, written at the start of a line
(`MutualExclusion == …`), in a `.tla` module under `proob/specs/` with a `.cfg` of the same
base name beside it. A token above an operator with parameters, a `VARIABLE` line, or nothing
is a token problem `validate` names. The property must say `kind = "model"`, and
`proob/config.toml` must carry an adapter with `language = "tla"` (`proob init --tla`).

Correct (after `tests/fixtures/tla-spec/Mutex.tla`, statement "Two clients are never inside
their critical sections at the same time", scope the lock server and its client):

```tla
---- MODULE LockService ----
\* The lock protocol of lockd/: one server grants a single lock; clients ask, retry, and release.
\* Left out: the network (a request is delivered once, whole, in order) and client crashes.
EXTENDS Naturals

CONSTANT Clients

VARIABLES pc, holder

vars == <<pc, holder>>

Init == /\ pc = [c \in Clients |-> "idle"]
        /\ holder = "none"

\* models lockd/server.py:12-17, handle ACQUIRE: grant only when no one holds the lock;
\* a denied client stays waiting and asks again (lockd/client.py:9-11).
Acquire(c) == /\ pc[c] = "idle"
              /\ holder = "none"
              /\ holder' = c
              /\ pc' = [pc EXCEPT ![c] = "crit"]

\* models lockd/server.py:19-22, handle RELEASE from the holder (lockd/client.py:14).
Release(c) == /\ pc[c] = "crit"
              /\ holder = c
              /\ holder' = "none"
              /\ pc' = [pc EXCEPT ![c] = "idle"]

Next == \E c \in Clients : Acquire(c) \/ Release(c)

Spec == Init /\ [][Next]_vars

\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XTA
MutualExclusion == \A c, d \in Clients : (pc[c] = "crit" /\ pc[d] = "crit") => c = d
====
```

```
\* LockService.cfg
SPECIFICATION Spec
\* Three clients: two are the least that can collide; a third waits while the lock changes hands.
CONSTANT Clients = {c1, c2, c3}
INVARIANT MutualExclusion
```

Every action cites the code it models, the module says what it leaves out, and each constant
says why its value. `run check` keeps the cfg's specification, constants, symmetry, constraints,
and `CHECK_DEADLOCK`, and checks only the bound name: as a `PROPERTY` when the cfg lists it under
`PROPERTY`, else as an `INVARIANT`.

Narrowed: passes, and checks nothing the statement is about. With one client no two can collide,
so every spec mutant survives and the score reads `killed 0`:

```
CONSTANT Clients = {c1}    \* no: mutual exclusion needs at least two clients
```

Vacuous: passes over any constants, because it is true of the types and says nothing about who
is in the critical section:

```tla
\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XTA
TypeOK == holder \in Clients \cup {"none"}
```

Forbidden in a bound spec: an invariant the statement does not say, or a weaker one; a constant
below the case the statement is about; a `CONSTRAINT` or `ACTION_CONSTRAINT` that prunes the
states the statement is about (one that only bounds an unbounded counter is fine, with a comment
and a line in "Model-checked for"); `CHECK_DEADLOCK FALSE` without a comment saying why the code
may stop there; an action that cites no code.

Check reference: `<module path> <Name>`, e.g. `proob/specs/LockService.tla MutualExclusion`;
`run check` prints it in its text output.

Binding set: the module, its `.cfg`, and every module beside it that it reaches through `EXTENDS`
or `INSTANCE` (standard modules such as `Naturals` live in the jar). The scoped code is not in
it; drift watches that (see "The binding set, explained to the reviewer" in `SKILL.md`).

## Python: Hypothesis through pytest (adapter arriving in U6)

A comment line holding only the token, directly above the test function, above its decorators:

```python
from hypothesis import given, strategies as st
from svc.ledger import apply

# proob:01K8V5NQ4A0F3R7Y2M9D6P1XHA
@given(st.lists(st.integers(min_value=-10**9, max_value=10**9)))
def test_balance_never_negative(entries):
    assert apply(entries).balance >= 0, entries
```

Forbidden in a bound test: `@settings(max_examples=…)`, `@settings(derandomize=True)`,
`@seed(…)`, `@pytest.mark.skip`, `pytest.skip()`, `@settings(database=…)`. proob loads a settings
profile that fixes `max_examples` to the budget and the seed to the run's seed; a decorator on
the test overrides the profile and breaks replay.

Until the adapter lands, a Python repo renders every property `Not checked (no adapter)`; you may
still write the test in this convention so it binds the day the adapter arrives, and the PR must
say that the check has not run.

## Verus: a proof, not a test

Not falsify's route (Kani, above, is). A property whose statement is an `ensures` clause of an item in a crate with
`verify = true` is bound by `// proob:<ULID>` directly above that item inside `verus!`, and must
declare `kind = "proof"`. `/proob:verify` proposes those bindings; see its `proof-route.md`.

## Choosing generators

- Cover the stated domain, not the convenient part of it: if the function takes `i64`, a
  strategy over `0..100` tests a different property than the one declared.
- Reuse the team's strategies when they exist (`tests/common/mod.rs` in the fixture); a new one
  belongs beside them, because it joins the binding set either way.
- Encode preconditions in the generator (`(lo, hi)` with `lo <= hi` built by construction), not
  with `prop_assume!` on the rare path; assumptions that reject most cases waste the budget and
  hide the domain.
- Prefer the statement's own shape: "never" is `prop_assert!(!bad)`; "equals" is
  `prop_assert_eq!`; "is a permutation of" is a sorted-equality or a multiset check, not a length
  check.
