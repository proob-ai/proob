//! The bound property-based tests. A `// proob:<ULID>` line directly above a
//! `#[test]` binds that test to the property with that id; the test's seed and
//! case count are never set here, so the adapter's `PROPTEST_RNG_SEED` and
//! `PROPTEST_CASES` apply.
mod common;

use proptest::prelude::*;
use rust_repo::bounds::clamp_to;
use rust_repo::ordering::sorted;

proptest! {
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHA
    #[test]
    fn sorted_preserves_length(v in common::small_vec()) {
        prop_assert_eq!(sorted(&v).len(), v.len());
    }

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHB
    #[test]
    fn clamp_stays_within_bounds(x in -100i64..100, (lo, hi) in common::bounds()) {
        let y = clamp_to(x, lo, hi);
        prop_assert!(lo <= y && y <= hi, "clamp_to({}, {}, {}) = {}", x, lo, hi, y);
    }
}
