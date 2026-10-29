//! Scope-hash combination (`proob-scope-v1`).
//!
//! A scope is a set of (path, content digest) pairs. Its hash folds the pairs
//! through SHA-256 in canonical order. The canonical order is strict
//! lexicographic order on path bytes, which also means a path appears at most
//! once; `is_strictly_sorted` is the verified decision procedure for that form
//! and `combine_sorted` is defined only on it.
//!
//! Proven here: `combine_sorted` computes exactly `spec_scope_hash`;
//! `is_strictly_sorted` agrees with `strictly_sorted`; `sort_by_path` returns a
//! permutation (`to_multiset()` equality) sorted by path; `lex_lt` is a strict
//! total order, so a strictly sorted permutation of a multiset is unique
//! (`lemma_canonical_unique`); and therefore `scope_hash` is a function of the
//! multiset of pairs, not their order (`spec_scope_hash_of`,
//! `lemma_scope_hash_order_independent`). A duplicate path has no canonical
//! form: `scope_hash` answers `DuplicatePath` instead of panicking, and
//! `paths_distinct_exec` lets a caller rule it out first.
//!
//! The sort takes its input by value. In Verus two byte vectors with equal
//! contents are not provably the same value, so a sort over clones could not be
//! shown to be a permutation of its input; moving the elements can.
use crate::bytes::bytes_eq;
use proob_trusted::sha256;
// Spec functions are erased by a plain `cargo build`; the guards keep these
// imports to verification only (the one sanctioned use of `verus_only`).
#[cfg(verus_only)]
use proob_trusted::sha256_spec;
#[cfg(verus_only)]
use vstd::multiset::Multiset;
use vstd::prelude::*;

verus! {

/// One member of a scope: a root-relative path and the SHA-256 of its
/// normalised content.
#[derive(Debug, PartialEq, Eq)]
pub struct PathDigest {
    pub path: Vec<u8>,
    pub digest: [u8; 32],
}

/// The scope hash of a pair list, or the reason there is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Structural)]
pub enum ScopeHash {
    Ok([u8; 32]),
    /// Two pairs share a path, so no canonical order exists. Resolved files
    /// never do this; `derive` requires it ruled out.
    DuplicatePath,
}

/// The algorithm tag `proob-scope-v1`, hashed first so that a scope hash can
/// never coincide with a content or binding hash over the same bytes.
pub const SCOPE_V1_TAG: [u8; 14] = [
    0x70, 0x72, 0x6f, 0x6f, 0x62, 0x2d, 0x73, 0x63, 0x6f, 0x70, 0x65, 0x2d, 0x76, 0x31,
];

/// Strict lexicographic order on byte sequences; a proper prefix sorts first.
pub open spec fn lex_lt(a: Seq<u8>, b: Seq<u8>) -> bool
    decreases a.len(),
{
    if b.len() == 0 {
        false
    } else if a.len() == 0 {
        true
    } else if a[0] < b[0] {
        true
    } else if a[0] > b[0] {
        false
    } else {
        lex_lt(a.drop_first(), b.drop_first())
    }
}

/// Canonical form: each path is strictly below its successor, so no path
/// repeats.
pub open spec fn strictly_sorted(pairs: Seq<PathDigest>) -> bool {
    forall|i: int|
        0 <= i && i + 1 < pairs.len() ==> lex_lt((#[trigger] pairs[i]).path@, pairs[i + 1].path@)
}

/// One fold step: running digest, then path, then content digest. Both digests
/// are 32 bytes wide, so the path's extent inside the frame is unambiguous.
pub open spec fn step(acc: Seq<u8>, p: PathDigest) -> Seq<u8> {
    sha256_spec(acc + p.path@ + p.digest@)
}

/// Left fold of `step` over `pairs`, defined from the back so that a prefix of
/// the list has a prefix of the fold.
pub open spec fn fold(acc: Seq<u8>, pairs: Seq<PathDigest>) -> Seq<u8>
    decreases pairs.len(),
{
    if pairs.len() == 0 {
        acc
    } else {
        step(fold(acc, pairs.drop_last()), pairs.last())
    }
}

/// The scope hash of a canonical pair list.
pub open spec fn spec_scope_hash(pairs: Seq<PathDigest>) -> Seq<u8> {
    fold(sha256_spec(SCOPE_V1_TAG@), pairs)
}

/// Dropping one equal leading byte from both sides leaves `lex_lt` unchanged.
proof fn lemma_lex_lt_skip_one(a: Seq<u8>, b: Seq<u8>, i: int)
    requires
        0 <= i < a.len(),
        i < b.len(),
        a[i] == b[i],
    ensures
        lex_lt(a.skip(i), b.skip(i)) == lex_lt(a.skip(i + 1), b.skip(i + 1)),
{
    assert(a.skip(i).drop_first() =~= a.skip(i + 1));
    assert(b.skip(i).drop_first() =~= b.skip(i + 1));
}

/// Decides `lex_lt` on byte slices.
pub fn lex_lt_exec(a: &[u8], b: &[u8]) -> (r: bool)
    ensures
        r == lex_lt(a@, b@),
{
    proof {
        assert(a@.skip(0) =~= a@);
        assert(b@.skip(0) =~= b@);
    }
    let mut i: usize = 0;
    while i < a.len() && i < b.len()
        invariant
            i <= a@.len(),
            i <= b@.len(),
            lex_lt(a@, b@) == lex_lt(a@.skip(i as int), b@.skip(i as int)),
        decreases a@.len() - i,
    {
        if a[i] < b[i] {
            return true;
        }
        if a[i] > b[i] {
            return false;
        }
        proof {
            lemma_lex_lt_skip_one(a@, b@, i as int);
        }
        i += 1;
    }
    a.len() < b.len()
}

/// Decides `strictly_sorted`.
pub fn is_strictly_sorted(pairs: &Vec<PathDigest>) -> (r: bool)
    ensures
        r == strictly_sorted(pairs@),
{
    let n = pairs.len();
    if n < 2 {
        return true;
    }
    let mut i: usize = 1;
    while i < n
        invariant
            1 <= i <= n,
            n == pairs@.len(),
            forall|j: int|
                0 <= j && j + 1 < i ==> lex_lt((#[trigger] pairs@[j]).path@, pairs@[j + 1].path@),
        decreases n - i,
    {
        if !lex_lt_exec(pairs[i - 1].path.as_slice(), pairs[i].path.as_slice()) {
            assert(!lex_lt(pairs@[i - 1].path@, pairs@[i as int].path@));
            return false;
        }
        i += 1;
    }
    true
}

/// The bytes of one fold step, in frame order.
fn frame_bytes(acc: &[u8; 32], p: &PathDigest) -> (out: Vec<u8>)
    ensures
        out@ == acc@ + p.path@ + p.digest@,
{
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(acc.as_slice());
    out.extend_from_slice(p.path.as_slice());
    out.extend_from_slice(p.digest.as_slice());
    out
}

/// The scope hash of a canonical pair list. Callers establish the precondition
/// with `is_strictly_sorted`; U3 adds the verified sort that produces it.
pub fn combine_sorted(pairs: &Vec<PathDigest>) -> (out: [u8; 32])
    requires
        strictly_sorted(pairs@),
    ensures
        out@ == spec_scope_hash(pairs@),
{
    // verify-negative-anchor
    let tag = SCOPE_V1_TAG;
    let mut acc: [u8; 32] = sha256(tag.as_slice());
    let n = pairs.len();
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == pairs@.len(),
            acc@ == fold(sha256_spec(SCOPE_V1_TAG@), pairs@.subrange(0, i as int)),
        decreases n - i,
    {
        let frame = frame_bytes(&acc, &pairs[i]);
        acc = sha256(frame.as_slice());
        i += 1;
        proof {
            let done = pairs@.subrange(0, i as int);
            assert(done.drop_last() =~= pairs@.subrange(0, i as int - 1));
            assert(done.last() == pairs@[i as int - 1]);
        }
    }
    proof {
        assert(pairs@.subrange(0, n as int) =~= pairs@);
    }
    acc
}

// ---------------------------------------------------------------------------
// lex_lt is a strict total order on byte sequences
// ---------------------------------------------------------------------------

proof fn lemma_lex_lt_irreflexive(a: Seq<u8>)
    ensures
        !lex_lt(a, a),
    decreases a.len(),
{
    if a.len() > 0 {
        lemma_lex_lt_irreflexive(a.drop_first());
    }
}

proof fn lemma_lex_lt_asymmetric(a: Seq<u8>, b: Seq<u8>)
    requires
        lex_lt(a, b),
    ensures
        !lex_lt(b, a),
    decreases a.len(),
{
    if a.len() > 0 && b.len() > 0 && a[0] == b[0] {
        lemma_lex_lt_asymmetric(a.drop_first(), b.drop_first());
    }
}

proof fn lemma_lex_lt_transitive(a: Seq<u8>, b: Seq<u8>, c: Seq<u8>)
    requires
        lex_lt(a, b),
        lex_lt(b, c),
    ensures
        lex_lt(a, c),
    decreases a.len(),
{
    if a.len() > 0 && b.len() > 0 && c.len() > 0 && a[0] == b[0] && b[0] == c[0] {
        lemma_lex_lt_transitive(a.drop_first(), b.drop_first(), c.drop_first());
    }
}

/// Two sequences neither of which is below the other are equal.
proof fn lemma_lex_lt_connected(a: Seq<u8>, b: Seq<u8>)
    requires
        !lex_lt(a, b),
        !lex_lt(b, a),
    ensures
        a == b,
    decreases a.len(),
{
    if a.len() == 0 || b.len() == 0 {
        assert(a =~= b);
    } else {
        lemma_lex_lt_connected(a.drop_first(), b.drop_first());
        assert forall|i: int| 0 <= i < a.len() implies a[i] == b[i] by {
            if i > 0 {
                assert(a.drop_first()[i - 1] == b.drop_first()[i - 1]);
            }
        }
        assert(a =~= b);
    }
}

/// `lex_lt` followed by its reflexive closure is still `lex_lt`.
proof fn lemma_lex_lt_le_transitive(a: Seq<u8>, b: Seq<u8>, c: Seq<u8>)
    requires
        lex_lt(a, b),
        b == c || lex_lt(b, c),
    ensures
        lex_lt(a, c),
{
    if b != c {
        lemma_lex_lt_transitive(a, b, c);
    }
}

// ---------------------------------------------------------------------------
// Orders on pair lists
// ---------------------------------------------------------------------------

/// The reflexive closure of `lex_lt` on paths: the order the sort establishes.
pub open spec fn path_le(a: PathDigest, b: PathDigest) -> bool {
    a.path@ == b.path@ || lex_lt(a.path@, b.path@)
}

/// Sorted by `path_le`, pairwise.
pub open spec fn sorted_le(pairs: Seq<PathDigest>) -> bool {
    forall|i: int, j: int| 0 <= i < j < pairs.len() ==> path_le(#[trigger] pairs[i], #[trigger] pairs[j])
}

/// No two pairs share a path.
pub open spec fn paths_distinct(pairs: Seq<PathDigest>) -> bool {
    forall|i: int, j: int|
        0 <= i < pairs.len() && 0 <= j < pairs.len() && i != j ==> (#[trigger] pairs[i]).path@ != (
        #[trigger] pairs[j]).path@
}

/// In a strictly sorted list every earlier path is below every later one.
proof fn lemma_strict_chain(s: Seq<PathDigest>, i: int, j: int)
    requires
        strictly_sorted(s),
        0 <= i < j < s.len(),
    ensures
        lex_lt(s[i].path@, s[j].path@),
    decreases j - i,
{
    if j > i + 1 {
        lemma_strict_chain(s, i, j - 1);
        lemma_lex_lt_transitive(s[i].path@, s[j - 1].path@, s[j].path@);
    }
}

/// Strict order has no repeated path.
proof fn lemma_strict_implies_distinct(s: Seq<PathDigest>)
    requires
        strictly_sorted(s),
    ensures
        paths_distinct(s),
{
    assert forall|i: int, j: int|
        0 <= i < s.len() && 0 <= j < s.len() && i != j implies (#[trigger] s[i]).path@ != (
    #[trigger] s[j]).path@ by {
        if i < j {
            lemma_strict_chain(s, i, j);
        } else {
            lemma_strict_chain(s, j, i);
        }
        lemma_lex_lt_irreflexive(s[i].path@);
    }
}

/// Sorted with distinct paths is strictly sorted.
proof fn lemma_sorted_le_distinct_strict(s: Seq<PathDigest>)
    requires
        sorted_le(s),
        paths_distinct(s),
    ensures
        strictly_sorted(s),
{
    assert forall|i: int| 0 <= i && i + 1 < s.len() implies lex_lt(
        (#[trigger] s[i]).path@,
        s[i + 1].path@,
    ) by {
        assert(path_le(s[i], s[i + 1]));
    }
}

/// A permutation of a list with distinct paths has distinct paths. The
/// multiset counts values, not paths, so the argument goes through
/// `no_duplicates` for equal values and through membership for distinct
/// values that share a path.
proof fn lemma_permutation_keeps_paths_distinct(x: Seq<PathDigest>, y: Seq<PathDigest>)
    requires
        x.to_multiset() == y.to_multiset(),
        paths_distinct(x),
    ensures
        paths_distinct(y),
{
    broadcast use vstd::seq_lib::group_to_multiset_ensures;

    assert(x.no_duplicates());
    x.lemma_multiset_has_no_duplicates();
    y.lemma_multiset_has_no_duplicates_conv();
    assert forall|i: int, j: int|
        0 <= i < y.len() && 0 <= j < y.len() && i != j implies (#[trigger] y[i]).path@ != (
    #[trigger] y[j]).path@ by {
        if y[i].path@ == y[j].path@ {
            assert(y.contains(y[i]));
            assert(y.contains(y[j]));
            assert(y.to_multiset().count(y[i]) > 0);
            assert(y.to_multiset().count(y[j]) > 0);
            assert(x.to_multiset().count(y[i]) > 0);
            assert(x.to_multiset().count(y[j]) > 0);
            assert(x.contains(y[i]));
            assert(x.contains(y[j]));
            let k = choose|k: int| 0 <= k < x.len() && x[k] == y[i];
            let l = choose|l: int| 0 <= l < x.len() && x[l] == y[j];
            assert(k != l);
            assert(x[k].path@ != x[l].path@);
        }
    }
}

/// Uniqueness of the canonical form: two strictly sorted lists with the same
/// elements are the same list. The head of each is its least path, so the
/// heads agree; then the tails do by induction.
pub proof fn lemma_canonical_unique(x: Seq<PathDigest>, y: Seq<PathDigest>)
    requires
        strictly_sorted(x),
        strictly_sorted(y),
        x.to_multiset() == y.to_multiset(),
    ensures
        x =~= y,
    decreases x.len(),
{
    broadcast use vstd::seq_lib::group_to_multiset_ensures;

    assert(x.len() == x.to_multiset().len());
    assert(y.len() == y.to_multiset().len());
    if x.len() == 0 {
        assert(x =~= y);
    } else {
        assert(x.contains(x[0]));
        assert(x.to_multiset().count(x[0]) > 0);
        assert(y.to_multiset().count(x[0]) > 0);
        assert(y.contains(x[0]));
        assert(y.contains(y[0]));
        assert(y.to_multiset().count(y[0]) > 0);
        assert(x.to_multiset().count(y[0]) > 0);
        assert(x.contains(y[0]));
        let j = choose|j: int| 0 <= j < y.len() && y[j] == x[0];
        let i = choose|i: int| 0 <= i < x.len() && x[i] == y[0];
        if i > 0 && j > 0 {
            lemma_strict_chain(x, 0, i);
            lemma_strict_chain(y, 0, j);
            lemma_lex_lt_asymmetric(x[0].path@, y[0].path@);
        }
        assert(x[0] == y[0]);
        assert(x.remove(0) =~= x.drop_first());
        assert(y.remove(0) =~= y.drop_first());
        assert(x.drop_first().to_multiset() == y.drop_first().to_multiset());
        lemma_canonical_unique(x.drop_first(), y.drop_first());
        assert forall|k: int| 0 <= k < x.len() implies x[k] == y[k] by {
            if k > 0 {
                assert(x.drop_first()[k - 1] == y.drop_first()[k - 1]);
            }
        }
        assert(x =~= y);
    }
}

// ---------------------------------------------------------------------------
// The sort
// ---------------------------------------------------------------------------

/// Where `x` goes in a sorted list: the first position whose path is above
/// `x`'s, or the end.
fn insertion_point(out: &Vec<PathDigest>, x: &PathDigest) -> (pos: usize)
    requires
        sorted_le(out@),
    ensures
        pos <= out@.len(),
        forall|k: int| 0 <= k < pos ==> path_le(#[trigger] out@[k], *x),
        forall|k: int| pos <= k < out@.len() ==> path_le(*x, #[trigger] out@[k]),
{
    let n = out.len();
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == out@.len(),
            sorted_le(out@),
            forall|k: int| 0 <= k < i ==> path_le(#[trigger] out@[k], *x),
        decreases n - i,
    {
        if lex_lt_exec(x.path.as_slice(), out[i].path.as_slice()) {
            proof {
                lemma_below_sorted_tail(out@, i as int, *x);
            }
            return i;
        }
        proof {
            if !lex_lt(out@[i as int].path@, x.path@) {
                lemma_lex_lt_connected(out@[i as int].path@, x.path@);
            }
        }
        i += 1;
    }
    i
}

/// Below one element of a sorted list means below the whole tail from it.
proof fn lemma_below_sorted_tail(s: Seq<PathDigest>, i: int, x: PathDigest)
    requires
        sorted_le(s),
        0 <= i < s.len(),
        lex_lt(x.path@, s[i].path@),
    ensures
        forall|k: int| i <= k < s.len() ==> path_le(x, #[trigger] s[k]),
{
    assert forall|k: int| i <= k < s.len() implies path_le(x, #[trigger] s[k]) by {
        if k > i {
            lemma_lex_lt_le_transitive(x.path@, s[i].path@, s[k].path@);
        }
    }
}

/// The shape of `insert`: the prefix, the element, the shifted suffix.
proof fn lemma_insert_index(s: Seq<PathDigest>, i: int, a: PathDigest)
    requires
        0 <= i <= s.len(),
    ensures
        s.insert(i, a).len() == s.len() + 1,
        forall|k: int| 0 <= k < i ==> #[trigger] s.insert(i, a)[k] == s[k],
        s.insert(i, a)[i] == a,
        forall|k: int| i < k <= s.len() ==> #[trigger] s.insert(i, a)[k] == s[k - 1],
{
}

/// Inserting between its neighbours keeps a list sorted.
proof fn lemma_insert_sorted(s: Seq<PathDigest>, i: int, a: PathDigest)
    requires
        sorted_le(s),
        0 <= i <= s.len(),
        forall|k: int| 0 <= k < i ==> path_le(#[trigger] s[k], a),
        forall|k: int| i <= k < s.len() ==> path_le(a, #[trigger] s[k]),
    ensures
        sorted_le(s.insert(i, a)),
{
    lemma_insert_index(s, i, a);
    let t = s.insert(i, a);
    assert forall|p: int, q: int| 0 <= p < q < t.len() implies path_le(#[trigger] t[p], #[trigger] t[q]) by {
        if q < i {
            assert(path_le(s[p], s[q]));
        } else if q == i {
            assert(path_le(s[p], a));
        } else if p < i {
            assert(path_le(s[p], s[q - 1]));
        } else if p == i {
            assert(path_le(a, s[q - 1]));
        } else {
            assert(path_le(s[p - 1], s[q - 1]));
        }
    }
}

/// Moving one element across a union of multisets changes nothing.
proof fn lemma_multiset_insert_shift(a: Multiset<PathDigest>, b: Multiset<PathDigest>, x: PathDigest)
    ensures
        a.insert(x).add(b) == a.add(b.insert(x)),
{
    broadcast use vstd::multiset::group_multiset_axioms;

    assert(a.insert(x).add(b) =~= a.add(b.insert(x)));
}

/// Insertion sort by path. The output is a permutation of the input (same
/// multiset) sorted by `path_le`; with distinct paths that is the strict
/// canonical order `combine_sorted` needs.
pub fn sort_by_path(pairs: Vec<PathDigest>) -> (out: Vec<PathDigest>)
    ensures
        sorted_le(out@),
        out@.to_multiset() == pairs@.to_multiset(),
{
    let mut rest = pairs;
    let mut out: Vec<PathDigest> = Vec::new();
    proof {
        assert(out@ + rest@ =~= rest@);
        vstd::seq_lib::lemma_multiset_commutative(out@, rest@);
    }
    while !rest.is_empty()
        invariant
            sorted_le(out@),
            out@.to_multiset().add(rest@.to_multiset()) == pairs@.to_multiset(),
        decreases rest.len(),
    {
        let ghost rest0 = rest@;
        let ghost out0 = out@;
        let x = match rest.pop() {
            Some(x) => x,
            None => vstd::pervasive::unreached(),
        };
        let pos = insertion_point(&out, &x);
        proof {
            lemma_insert_sorted(out@, pos as int, x);
        }
        out.insert(pos, x);
        proof {
            broadcast use vstd::seq_lib::group_to_multiset_ensures;

            assert(rest0 =~= rest@.push(x));
            assert(rest0.to_multiset() == rest@.to_multiset().insert(x));
            assert(out@.to_multiset() == out0.to_multiset().insert(x));
            lemma_multiset_insert_shift(out0.to_multiset(), rest@.to_multiset(), x);
        }
    }
    proof {
        assert(out@ + rest@ =~= out@);
        vstd::seq_lib::lemma_multiset_commutative(out@, rest@);
    }
    out
}

// ---------------------------------------------------------------------------
// The scope hash as a function of the set of pairs
// ---------------------------------------------------------------------------

/// `s` is the canonical form of the multiset `m`.
pub open spec fn canonical_of(m: Multiset<PathDigest>, s: Seq<PathDigest>) -> bool {
    strictly_sorted(s) && s.to_multiset() == m
}

/// The scope hash of a multiset of pairs: the hash of its canonical form.
/// Defined on the multiset, so order independence is by construction; that
/// the definition is pinned to the one canonical form is
/// `lemma_canonical_pins`.
pub open spec fn spec_scope_hash_of(m: Multiset<PathDigest>) -> Seq<u8> {
    spec_scope_hash(choose|s: Seq<PathDigest>| canonical_of(m, s))
}

/// A canonical form determines the hash of its multiset.
pub proof fn lemma_canonical_pins(m: Multiset<PathDigest>, s: Seq<PathDigest>)
    requires
        canonical_of(m, s),
    ensures
        spec_scope_hash_of(m) == spec_scope_hash(s),
{
    let c = choose|t: Seq<PathDigest>| canonical_of(m, t);
    assert(canonical_of(m, c));
    lemma_canonical_unique(c, s);
}

/// The order-independence theorem: pair lists with the same multiset have
/// the same scope hash.
pub proof fn lemma_scope_hash_order_independent(a: Seq<PathDigest>, b: Seq<PathDigest>)
    requires
        a.to_multiset() == b.to_multiset(),
    ensures
        spec_scope_hash_of(a.to_multiset()) == spec_scope_hash_of(b.to_multiset()),
{
}

/// Decides `paths_distinct`.
pub fn paths_distinct_exec(pairs: &Vec<PathDigest>) -> (r: bool)
    ensures
        r == paths_distinct(pairs@),
{
    let n = pairs.len();
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == pairs@.len(),
            forall|a: int, b: int|
                0 <= a < i && a < b < n ==> (#[trigger] pairs@[a]).path@ != (#[trigger] pairs@[b]).path@,
        decreases n - i,
    {
        let mut j: usize = i + 1;
        while j < n
            invariant
                i < n,
                i + 1 <= j <= n,
                n == pairs@.len(),
                forall|a: int, b: int|
                    0 <= a < i && a < b < n ==> (#[trigger] pairs@[a]).path@ != (#[trigger] pairs@[b]).path@,
                forall|b: int| i < b < j ==> pairs@[i as int].path@ != (#[trigger] pairs@[b]).path@,
            decreases n - j,
        {
            if bytes_eq(pairs[i].path.as_slice(), pairs[j].path.as_slice()) {
                assert(pairs@[i as int].path@ == pairs@[j as int].path@);
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    proof {
        assert forall|a: int, b: int|
            0 <= a < n && 0 <= b < n && a != b implies (#[trigger] pairs@[a]).path@ != (
        #[trigger] pairs@[b]).path@ by {
            if a < b {
            } else {
                assert(pairs@[b].path@ != pairs@[a].path@);
            }
        }
    }
    true
}

/// The scope hash of any pair list: sort, then combine. `Ok` exactly when the
/// paths are distinct; the hash is then a function of the multiset of pairs.
// proob:01M622RQ778BXGXXT6ZQ54EJFR
pub fn scope_hash(pairs: Vec<PathDigest>) -> (r: ScopeHash)
    ensures
        r is Ok <==> paths_distinct(pairs@),
        r matches ScopeHash::Ok(h) ==> h@ == spec_scope_hash_of(pairs@.to_multiset()),
{
    let ghost original = pairs@;
    let sorted = sort_by_path(pairs);
    if is_strictly_sorted(&sorted) {
        proof {
            lemma_strict_implies_distinct(sorted@);
            lemma_permutation_keeps_paths_distinct(sorted@, original);
            lemma_canonical_pins(original.to_multiset(), sorted@);
        }
        ScopeHash::Ok(combine_sorted(&sorted))
    } else {
        proof {
            if paths_distinct(original) {
                lemma_permutation_keeps_paths_distinct(original, sorted@);
                lemma_sorted_le_distinct_strict(sorted@);
            }
        }
        ScopeHash::DuplicatePath
    }
}

} // verus!
