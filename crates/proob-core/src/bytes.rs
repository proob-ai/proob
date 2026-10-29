//! Byte-exact equality of paths and digests.
//!
//! The core compares paths (byte vectors) and digests (`[u8; 32]`) by content.
//! `bytes_eq` decides equality of the byte sequences; `digest_eq` lifts it to
//! equality of two digests, which is what "at the current hashes" means in
//! `derive` and what membership means in `affected`.
use vstd::prelude::*;

verus! {

/// Decides `a@ == b@`.
pub fn bytes_eq(a: &[u8], b: &[u8]) -> (r: bool)
    ensures
        r == (a@ == b@),
{
    let n = a.len();
    if n != b.len() {
        return false;
    }
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == a@.len(),
            n == b@.len(),
            forall|k: int| 0 <= k < i ==> a@[k] == b@[k],
        decreases n - i,
    {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    proof {
        assert(a@ =~= b@);
    }
    true
}

/// Decides `*a == *b` for two digests (arrays are extensional: equal bytes,
/// equal values).
pub fn digest_eq(a: &[u8; 32], b: &[u8; 32]) -> (r: bool)
    ensures
        r == (*a == *b),
{
    let r = bytes_eq(a.as_slice(), b.as_slice());
    proof {
        if r {
            assert(*a =~= *b);
        }
    }
    r
}

} // verus!
