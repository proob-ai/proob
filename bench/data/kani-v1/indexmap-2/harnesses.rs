//! Kani harnesses for proob properties. Each map's keys are the labels
//! `0..len` inserted in order; every key's hash is a symbolic `u64`, so the
//! harnesses cover every hash layout, collisions included.

use crate::IndexMap;
use core::hash::{BuildHasher, Hasher};

/// Maps of up to 4 entries: each extra entry is one more loop iteration and
/// one more symbolic hash for Kani; 4 already spans hashbrown's 4- and 8-bucket tables.
const MAX_LEN: usize = 4;

/// A hasher whose output for key `k` is `table[k]`: consistent per key, symbolic across keys.
#[derive(Clone, Copy)]
struct SymbolicState {
    table: [u64; MAX_LEN + 1],
}

struct SymbolicHasher {
    table: [u64; MAX_LEN + 1],
    out: u64,
}

impl BuildHasher for SymbolicState {
    type Hasher = SymbolicHasher;
    fn build_hasher(&self) -> SymbolicHasher {
        SymbolicHasher { table: self.table, out: 0 }
    }
}

impl Hasher for SymbolicHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u8(b);
        }
    }
    fn write_u8(&mut self, b: u8) {
        self.out = self.table[b as usize % (MAX_LEN + 1)];
    }
    fn finish(&self) -> u64 {
        self.out
    }
}

type Map = IndexMap<u8, u8, SymbolicState>;

/// A map holding keys `0..len` in that order, with symbolic values and hashes.
fn any_map(len: usize) -> Map {
    let mut map = Map::with_hasher(SymbolicState { table: kani::any() });
    for k in 0..len {
        map.insert(k as u8, kani::any());
    }
    map
}

/// Every key sits at `expected[i]` for position `i`, and lookup finds it there.
fn assert_layout(map: &Map, expected: &[u8]) {
    assert_eq!(map.len(), expected.len());
    for (i, &k) in expected.iter().enumerate() {
        assert_eq!(map.get_index(i).map(|(k, _)| *k), Some(k), "key at position");
        assert_eq!(map.get_index_of(&k), Some(i), "lookup position");
    }
}

// proob:01M68C22H24EG7QDWFA8RRFMBS
#[kani::proof]
#[kani::unwind(10)]
fn shift_remove_keeps_order_and_lookups() {
    let len: usize = kani::any();
    kani::assume(len <= MAX_LEN); // the size bound, not the statement's
    let r: usize = kani::any();
    kani::assume(r < len); // "removing a key" that is in the map
    kani::cover!(len == 1, "removing the only key");
    kani::cover!(r == 0 && len > 1, "removing the first key");
    kani::cover!(r + 1 == len && len > 1, "removing the last key");
    let mut map = any_map(len);
    let before: [u8; MAX_LEN] = core::array::from_fn(|i| i as u8);
    let value = map[r];
    assert_eq!(map.shift_remove(&(r as u8)), Some(value));
    assert_eq!(map.get_index_of(&(r as u8)), None);
    let mut expected = [0u8; MAX_LEN];
    let mut n = 0;
    for &k in &before[..len] {
        if k as usize != r {
            expected[n] = k;
            n += 1;
        }
    }
    assert_layout(&map, &expected[..n]);
}

// proob:01M68C22HVJ2KHAK8WM9645T06
#[kani::proof]
#[kani::unwind(10)]
fn swap_remove_moves_only_the_last_entry() {
    let len: usize = kani::any();
    kani::assume(len <= MAX_LEN); // the size bound, not the statement's
    let r: usize = kani::any();
    kani::assume(r < len); // "removing a key" that is in the map
    kani::cover!(len == 1, "removing the only key");
    kani::cover!(r + 1 == len && len > 1, "removing the last entry itself");
    kani::cover!(r + 1 < len, "the last entry moves into the gap");
    let mut map = any_map(len);
    let value = map[r];
    assert_eq!(map.swap_remove(&(r as u8)), Some(value));
    assert_eq!(map.get_index_of(&(r as u8)), None);
    let mut expected: [u8; MAX_LEN] = core::array::from_fn(|i| i as u8);
    if r + 1 < len {
        expected[r] = (len - 1) as u8;
    }
    assert_layout(&map, &expected[..len - 1]);
}

// proob:01M68C22JJY533ADTJ85CSVV9H
#[kani::proof]
#[kani::unwind(10)]
fn move_index_keeps_the_rest_in_order() {
    let len: usize = kani::any();
    kani::assume(len <= MAX_LEN); // the size bound, not the statement's
    let (from, to): (usize, usize) = (kani::any(), kani::any());
    kani::assume(from < len && to < len); // "from one position to another" in the map
    kani::cover!(from < to, "moving up");
    kani::cover!(to < from, "moving down");
    kani::cover!(from == to, "moving to the same position");
    let mut map = any_map(len);
    map.move_index(from, to);
    // The keys in order without the moved one, then the moved one put back at `to`.
    let mut rest = [0u8; MAX_LEN];
    let mut n = 0;
    for k in 0..len {
        if k != from {
            rest[n] = k as u8;
            n += 1;
        }
    }
    let expected: [u8; MAX_LEN] = core::array::from_fn(|i| match i.cmp(&to) {
        core::cmp::Ordering::Less => rest[i],
        core::cmp::Ordering::Equal => from as u8,
        core::cmp::Ordering::Greater => rest[i - 1],
    });
    assert_layout(&map, &expected[..len]);
}

// proob:01M68C22K9E6CQFTDMRC2GE8AM
#[kani::proof]
#[kani::unwind(10)]
fn reverse_keeps_lookups_consistent() {
    let len: usize = kani::any();
    kani::assume(len <= MAX_LEN); // the size bound, not the statement's
    kani::cover!(len == 0, "an empty map");
    kani::cover!(len == MAX_LEN, "a full-size map");
    let mut map = any_map(len);
    map.reverse();
    let expected: [u8; MAX_LEN] = core::array::from_fn(|i| (len.wrapping_sub(i + 1)) as u8);
    assert_layout(&map, &expected[..len]);
}

// proob:01M68C22M1C56Z1QBTSZHNYM84
#[kani::proof]
#[kani::unwind(10)]
fn equality_ignores_insertion_order() {
    // For each of the MAX_LEN possible keys, its value in each map, or absent.
    let a: [Option<u8>; MAX_LEN] = kani::any();
    let b: [Option<u8>; MAX_LEN] = kani::any();
    // The order in which the second map's keys are inserted: any permutation.
    let order: [u8; MAX_LEN] = kani::any();
    for i in 0..MAX_LEN {
        kani::assume((order[i] as usize) < MAX_LEN);
        for j in 0..i {
            kani::assume(order[i] != order[j]);
        }
    }
    let mut ma = Map::with_hasher(SymbolicState { table: kani::any() });
    for (k, v) in a.iter().enumerate() {
        if let Some(v) = v {
            ma.insert(k as u8, *v);
        }
    }
    let mut mb = Map::with_hasher(SymbolicState { table: kani::any() });
    for &k in &order {
        if let Some(v) = b[k as usize] {
            mb.insert(k, v);
        }
    }
    let same = a == b;
    kani::cover!(same && order[0] != 0, "equal contents, different insertion order");
    kani::cover!(!same, "different contents");
    assert_eq!(ma == mb, same, "equality agrees with keys and values");
    assert_eq!(mb == ma, same, "equality is symmetric");
}
