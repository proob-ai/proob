//! Strategies shared by the bound tests. Every test crate that declares
//! `mod common;` carries this file in its binding set, so a change here reads
//! `Re-approve (check amended)` for each property bound in that crate.
use proptest::prelude::*;

/// Short vectors of small numbers.
pub fn small_vec() -> impl Strategy<Value = Vec<u32>> {
    prop::collection::vec(0u32..100, 0..8)
}

/// A pair `(lo, hi)` with `lo <= hi`.
pub fn bounds() -> impl Strategy<Value = (i64, i64)> {
    (-50i64..50, 0i64..50).prop_map(|(lo, span)| (lo, lo + span))
}
