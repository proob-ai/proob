//! Four verified functions. `clamp`'s postcondition says what clamping means;
//! `smaller`'s says nothing at all; mutation-under-proof tells them apart (a
//! wrong bound fails `clamp`'s proof, every mutant of `smaller` verifies).
//! `classify` and `ordered` are the shapes the first operator set missed.
use vstd::prelude::*;

verus! {

/// `x` brought into `[lo, hi]`.
// proob:01K8V5NQ4A0F3R7Y2M9D6P1XVA
pub fn clamp(x: i32, lo: i32, hi: i32) -> (r: i32)
    requires
        lo <= hi,
    ensures
        lo <= r <= hi,
        lo <= x <= hi ==> r == x,
        x < lo ==> r == lo,
        x > hi ==> r == hi,
{
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

/// The smaller of two values, under a specification any result satisfies.
// proob:01K8V5NQ4A0F3R7Y2M9D6P1XVB
pub fn smaller(a: u32, b: u32) -> (r: u32)
    ensures
        true,
{
    if a < b {
        a
    } else {
        b
    }
}

/// The sign of a value.
#[derive(PartialEq, Eq, Structural, Clone, Copy, Debug)]
pub enum Sign {
    Negative,
    Zero,
    Positive,
}

/// The sign of a value, by cases. The body is one `match` whose arms return
/// constructors: no operator for `mutants-under-proof-v1` to swap, so it
/// scored `NoMutants`; v2 exchanges adjacent arms' results and each
/// `ensures` line kills one exchange.
// proob:01K8V5NQ4A0F3R7Y2M9D6P1XVE
pub fn classify(n: i64) -> (s: Sign)
    ensures
        n < 0 ==> s == Sign::Negative,
        n == 0 ==> s == Sign::Zero,
        n > 0 ==> s == Sign::Positive,
{
    match n {
        i64::MIN..=-1 => Sign::Negative,
        0 => Sign::Zero,
        _ => Sign::Positive,
    }
}

/// Two values in order.
#[derive(PartialEq, Eq, Structural, Clone, Copy, Debug)]
pub struct Range {
    pub lo: i32,
    pub hi: i32,
}

/// Braces inside the specification: a struct pattern after `matches` and a
/// `matches!` holding one. v1 took the first of them for the body and found
/// nothing to mutate there; the body is the `if` below, whose comparison and
/// branches v2 mutates.
// proob:01K8V5NQ4A0F3R7Y2M9D6P1XVF
pub fn ordered(a: i32, b: i32) -> (r: Range)
    ensures
        r matches Range { lo, hi } ==> lo <= hi && lo + hi == a + b,
        matches!(r, Range { lo, .. } if lo == a) ==> a <= b,
        r.lo == a || r.lo == b,
{
    if a < b {
        Range { lo: a, hi: b }
    } else {
        Range { lo: b, hi: a }
    }
}

} // verus!
