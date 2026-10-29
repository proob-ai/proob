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

Not falsify's route. A property whose statement is an `ensures` clause of an item in a crate with
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
