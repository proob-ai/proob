//! Kani harnesses for proob properties. Each harness checks its statement for every
//! input its bounds admit, and only those.

use crate::IndexMap;
use crate::util::{simplify_range, try_simplify_range};
use core::hash::{BuildHasher, Hasher};
use core::ops::Bound;

/// Hashes a key to its own value, so Kani reasons about concrete table slots
/// instead of a hash function. Every key still goes through the hash table.
#[derive(Clone, Default)]
struct IdentityState;

#[derive(Default)]
struct IdentityHasher(u64);

impl Hasher for IdentityHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 << 8) | u64::from(b);
        }
    }
}

impl BuildHasher for IdentityState {
    type Hasher = IdentityHasher;
    fn build_hasher(&self) -> IdentityHasher {
        IdentityHasher::default()
    }
}

type Map<V> = IndexMap<u8, V, IdentityState>;

/// Maps built from up to 3 inserts: each extra entry is more hash-table states
/// for Kani to explore, and 3 entries already reach the first table resize.
const MAX_ENTRIES: usize = 3;

fn any_bound() -> Bound<usize> {
    match kani::any::<u8>() % 3 {
        0 => Bound::Included(kani::any()),
        1 => Bound::Excluded(kani::any()),
        _ => Bound::Unbounded,
    }
}

fn any_map<V: kani::Arbitrary>() -> Map<V> {
    let keys: [u8; MAX_ENTRIES] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= MAX_ENTRIES); // the size bound, not the statement's
    let mut map = Map::with_hasher(IdentityState);
    for &k in &keys[..len] {
        map.insert(k, kani::any());
    }
    map
}

/// Every key in the map is found by lookup at the position where it is listed.
fn lookup_agrees_with_position<V>(map: &Map<V>) {
    for i in 0..map.len() {
        let (k, _) = map.get_index(i).unwrap();
        assert_eq!(map.get_index_of(k), Some(i), "key at {i} is not found at {i}");
    }
}

// proob:01M6870VY1VAS0CSMP9ETBDF9B
#[kani::proof]
fn range_resolution_stays_in_bounds() {
    let range = (any_bound(), any_bound());
    let len: usize = kani::any();
    let checked = try_simplify_range(range, len);
    kani::cover!(checked.is_none(), "a rejected range");
    kani::cover!(matches!(&checked, Some(r) if r.start == r.end), "an empty range");
    kani::cover!(matches!(&checked, Some(r) if r.end == len), "a range ending at the length");
    if let Some(r) = checked {
        assert!(r.start <= r.end && r.end <= len, "{r:?} is not inside 0..{len}");
        assert_eq!(simplify_range(range, len), r, "the two resolutions disagree");
    }
}

// proob:01M6870VZ8QD2NFMM4CDM5JQT6
#[kani::proof]
#[kani::unwind(9)]
fn swap_removal_keeps_lookup_and_position_in_step() {
    let mut map = any_map::<()>();
    let before = map.len();
    let key: u8 = kani::any();
    let was_present = map.contains_key(&key);
    kani::cover!(was_present && map.get_index_of(&key) != Some(before - 1), "removing a non-last key");
    kani::cover!(!was_present, "removing an absent key");
    map.swap_remove(&key);
    assert!(!map.contains_key(&key), "the removed key is still found");
    assert_eq!(map.len(), before - usize::from(was_present));
    lookup_agrees_with_position(&map);
}

// proob:01M6870W0HE37YYE625NQVF537
#[kani::proof]
#[kani::unwind(9)]
fn moving_an_entry_shifts_only_the_entries_between() {
    let mut map = any_map::<()>();
    let len = map.len();
    let (from, to): (usize, usize) = (kani::any(), kani::any());
    kani::assume(from < len && to < len); // "from one position to another": both positions exist
    kani::cover!(from < to, "moving later");
    kani::cover!(from > to, "moving earlier");
    kani::cover!(from == to, "moving in place");
    let mut before = [0u8; MAX_ENTRIES];
    for i in 0..len {
        before[i] = *map.get_index(i).unwrap().0;
    }
    map.move_index(from, to);
    assert_eq!(map.len(), len);
    for i in 0..len {
        // Where the entry now at i was before the move.
        let source = if i == to {
            from
        } else if from < to && from <= i && i < to {
            i + 1
        } else if to < from && to < i && i <= from {
            i - 1
        } else {
            i
        };
        assert_eq!(*map.get_index(i).unwrap().0, before[source], "wrong key at {i}");
    }
    lookup_agrees_with_position(&map);
}

// proob:01M6870W1SJRYP1F3BQQ22CRJP
#[kani::proof]
#[kani::unwind(9)]
fn reversal_keeps_lookup_and_position_in_step() {
    let mut map = any_map::<()>();
    let len = map.len();
    kani::cover!(len == 0, "an empty map");
    kani::cover!(len == MAX_ENTRIES, "a full map");
    let mut before = [0u8; MAX_ENTRIES];
    for i in 0..len {
        before[i] = *map.get_index(i).unwrap().0;
    }
    map.reverse();
    assert_eq!(map.len(), len);
    for i in 0..len {
        assert_eq!(*map.get_index(i).unwrap().0, before[len - 1 - i], "wrong key at {i}");
    }
    lookup_agrees_with_position(&map);
}

/// Every pair of `a` appears among the pairs of `b`, read by position only.
fn pairs_within(a: &Map<bool>, b: &Map<bool>) -> bool {
    (0..a.len()).all(|i| {
        let pa = a.get_index(i).unwrap();
        (0..b.len()).any(|j| b.get_index(j).unwrap() == pa)
    })
}

// proob:01M6870W31YZCHTXYBHVVEA1TG
#[kani::proof]
#[kani::unwind(9)]
fn map_equality_ignores_order() {
    let a = any_map::<bool>();
    let b = any_map::<bool>();
    let same_pairs = a.len() == b.len() && pairs_within(&a, &b) && pairs_within(&b, &a);
    kani::cover!(same_pairs && a.len() > 1 && a.get_index(0) != b.get_index(0), "equal maps in different orders");
    kani::cover!(!same_pairs && a.len() == b.len(), "unequal maps of the same length");
    assert_eq!(a == b, same_pairs, "equality disagrees with the pairs");
}
