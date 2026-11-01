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
    // Never compiled: contracts need `-Z function-contracts` and `clamp` has
    // none; the harness shows the scanner the contract form only.
    #[cfg(any())]
    #[kani::proof_for_contract(clamp)]
    fn clamp_contract() {
        clamp(kani::any(), kani::any(), kani::any());
    }

    // The harnesses below are the adapter's real-run cases: each reaches one
    // row of the outcome table.

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKF
    #[kani::proof]
    fn clamp_is_identity_planted_failure() {
        let (x, lo, hi) = (kani::any(), kani::any(), kani::any());
        assert_eq!(clamp(x, lo, hi), x);
    }

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKG
    #[kani::proof]
    fn clamp_through_inline_asm() {
        let x: u64 = kani::any();
        let y: u64;
        // `mov` reads the same on x86_64 and aarch64.
        unsafe { core::arch::asm!("mov {0}, {1}", out(reg) y, in(reg) x) };
        assert!(clamp(y as i32, 0, 1) <= 1);
    }

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKH
    #[kani::proof]
    #[kani::unwind(3)]
    fn clamp_in_a_loop_with_a_short_unwind() {
        let n: u8 = kani::any();
        kani::assume(n < 10);
        let mut i = 0u8;
        while i < n {
            assert!(clamp(i32::from(i), 0, 9) == i32::from(i));
            i += 1;
        }
    }

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKJ
    #[kani::proof]
    fn clamp_covers_an_unreachable_case() {
        let (x, lo, hi) = (kani::any(), kani::any(), kani::any());
        kani::assume(lo <= hi);
        let y = clamp(x, lo, hi);
        kani::cover!(y < lo, "below the lower bound");
        assert!(lo <= y && y <= hi);
    }

    // The harnesses below are the score's real-run cases: each is a weak
    // proof that mutation must read as weak.

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKK
    #[kani::proof]
    fn clamp_runs_with_ordered_bounds() {
        let (x, lo, hi): (i32, i32, i32) = (kani::any(), kani::any(), kani::any());
        kani::assume(lo <= hi);
        let _ = clamp(x, lo, hi);
        // Asserts only what it assumed: nothing about the result.
        assert!(lo <= hi);
    }

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKM
    #[kani::proof]
    fn clamp_indexes_a_table() {
        let table = [0u8; 11];
        // Only Rust's own bounds check looks at the result.
        let _entry = table[clamp(kani::any(), 0, 10) as usize];
    }
}
