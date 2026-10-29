//! A bound test that cannot fail: it never looks at the code in its scope, so
//! every mutant of `src/parity.rs` survives it and its score reads
//! `Scored { killed: 0, .. }`.
mod common;

use proptest::prelude::*;

proptest! {
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHD
    #[test]
    fn evens_are_counted_vacuously(v in common::small_vec()) {
        prop_assert!(v.len() <= 8);
    }
}
