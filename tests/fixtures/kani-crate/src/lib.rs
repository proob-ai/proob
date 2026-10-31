//! One pure function, checked two ways: a proptest under `tests/` and Kani
//! harnesses in `verification`, compiled only under `cargo kani`.

/// `x` clamped into `lo..=hi`; `lo` wins when the bounds cross.
pub fn clamp(x: i32, lo: i32, hi: i32) -> i32 {
    if x < lo {
        lo
    } else if x > hi {
        hi.max(lo)
    } else {
        x
    }
}

#[cfg(kani)]
mod verification {
    use super::clamp;

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKB
    #[kani::proof]
    fn clamp_stays_within_bounds() {
        let (x, lo, hi) = (kani::any(), kani::any(), kani::any());
        kani::assume(lo <= hi);
        let y = clamp(x, lo, hi);
        assert!(lo <= y && y <= hi);
    }

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKC
    #[cfg_attr(kani, kani::proof)]
    fn clamp_keeps_values_inside() {
        let (x, lo, hi) = (kani::any(), kani::any(), kani::any());
        kani::assume(lo <= x && x <= hi);
        assert_eq!(clamp(x, lo, hi), x);
    }

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKD
    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn clamp_is_idempotent() {
        let (x, lo, hi) = (kani::any(), kani::any(), kani::any());
        let once = clamp(x, lo, hi);
        assert_eq!(clamp(once, lo, hi), once);
    }

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKE
    #[kani::proof_for_contract(clamp)]
    fn clamp_contract() {
        clamp(kani::any(), kani::any(), kani::any());
    }
}
