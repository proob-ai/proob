//! The bound proptest; the adapter sets its seed and case count.
use kani_crate::clamp;
use proptest::prelude::*;

proptest! {
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKA
    #[test]
    fn clamp_stays_within_bounds(x in any::<i32>(), lo in -100i32..0, hi in 0i32..100) {
        let y = clamp(x, lo, hi);
        prop_assert!(lo <= y && y <= hi, "clamp({}, {}, {}) = {}", x, lo, hi, y);
    }
}
