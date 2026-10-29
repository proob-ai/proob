/// Clamps `x` into `[lo, hi]`. Deliberately wrong: the upper bound is never
/// applied, so the property bound to it fails and shrinks to a small witness.
pub fn clamp_to(x: i64, lo: i64, hi: i64) -> i64 {
    if x < lo { lo } else { x }
}
