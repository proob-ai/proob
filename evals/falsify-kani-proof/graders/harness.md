---
type: llm
weight: 3
---
Score 1 only if all of the following hold; otherwise 0.

1. The new property is bound to a Kani harness, not a proptest: a `#[kani::proof]` fn inside a `#[cfg(kani)]` module of the library crate (in `src/`), with the comment line `// proob:<the property's id>` directly above it (only blank lines, comments, or attributes between), and that token appears nowhere else.
2. The harness takes `x`, `lo`, and `hi` from `kani::any()` over the full `i32` range, and its only `kani::assume` is the ordering the statement states (`lo <= hi`); it does not narrow the bounds or the input further (no assume like `-100 <= lo`, `x == 3`, or `lo <= x`).
3. It has at least one `kani::cover!` for a boundary case the statement spans (for example equal bounds, or `x` below `lo` or above `hi`), and it asserts that the result lies between `lo` and `hi`.
4. It uses no `#[kani::unwind]` (`clamp` has no loop), no `#[kani::stub]`, no `proof_for_contract`, and no `#[kani::should_panic]`.
5. The property file's `kind` is still `"proof"` and `src/lib.rs`'s `clamp` is unchanged.
