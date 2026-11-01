#!/usr/bin/env bash
# The Kani fixture with a bug planted in `clamp` (a value above the bounds
# comes back one past the upper bound) and a kind = "proof" property already
# bound to a correct Kani harness. Kani finds the input: the run is
# proof_failed, and falsify must report a bug and never fall back to a test.
set -eu
. "$(dirname -- "$0")/../lib/kani-fixture.sh"
sed -i.bak 's/hi\.max(lo)$/hi.max(lo) + 1/' src/lib.rs && rm -f src/lib.rs.bak
grep -q 'hi.max(lo) + 1' src/lib.rs || { echo "scaffold: the planted bug did not apply to src/lib.rs" >&2; exit 1; }
cat >> src/lib.rs <<'RS'
#[cfg(kani)]
mod verification {
    use super::clamp;

    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XKB
    #[kani::proof]
    fn clamp_stays_within_bounds() {
        let (x, lo, hi): (i32, i32, i32) = (kani::any(), kani::any(), kani::any());
        kani::assume(lo <= hi);
        kani::cover!(lo == hi, "equal bounds");
        kani::cover!(x < lo, "below the bounds");
        kani::cover!(x > hi, "above the bounds");
        let y = clamp(x, lo, hi);
        assert!(lo <= y && y <= hi, "clamped value outside its bounds");
    }
}
RS
cat > proob/properties/01K8V5NQ4A0F3R7Y2M9D6P1XKB.toml <<'TOML'
id = "01K8V5NQ4A0F3R7Y2M9D6P1XKB"
title = "Clamping lands inside its bounds"
statement = "For ordered bounds, a clamped value lies between them."
scope = ["src/lib.rs"]
severity = "high"
rationale = "Callers rely on the clamped value to index a bounded table."
kind = "proof"
TOML
git -c user.name="proob eval" -c user.email=eval@proob.invalid add -A
git -c user.name="proob eval" -c user.email=eval@proob.invalid commit -q --amend -m "Kani fixture, set up with proob init --kani, and a bound proof"
