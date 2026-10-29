//! Plain-cargo tests of the verified scope hash. The proofs establish
//! correctness against the spec; these pin the erased binary to the same
//! behaviour and give U3 a reference fold to extend.
use proob_core::scope_hash::{PathDigest, SCOPE_V1_TAG, combine_sorted, is_strictly_sorted};
use proob_trusted::sha256;

fn pair(path: &str, fill: u8) -> PathDigest {
    PathDigest { path: path.as_bytes().to_vec(), digest: [fill; 32] }
}

fn reference_fold(pairs: &[PathDigest]) -> [u8; 32] {
    pairs.iter().fold(sha256(&SCOPE_V1_TAG), |acc, p| {
        let frame: Vec<u8> = [&acc[..], &p.path[..], &p.digest[..]].concat();
        sha256(&frame)
    })
}

#[test]
fn strict_order_is_recognised() {
    assert!(is_strictly_sorted(&vec![]));
    assert!(is_strictly_sorted(&vec![pair("a", 1)]));
    assert!(is_strictly_sorted(&vec![pair("a", 1), pair("ab", 2), pair("b", 3)]));
}

#[test]
fn unsorted_and_duplicate_paths_are_rejected() {
    assert!(!is_strictly_sorted(&vec![pair("b", 1), pair("a", 2)]));
    assert!(!is_strictly_sorted(&vec![pair("a", 1), pair("a", 2)]));
    assert!(!is_strictly_sorted(&vec![pair("ab", 1), pair("a", 2)]));
}

#[test]
fn combine_matches_the_reference_fold() {
    let pairs = vec![pair("src/a.rs", 1), pair("src/b.rs", 2), pair("src/c.rs", 3)];
    assert!(is_strictly_sorted(&pairs));
    assert_eq!(combine_sorted(&pairs), reference_fold(&pairs));
}

#[test]
fn empty_scope_hashes_to_the_tag_digest() {
    assert_eq!(combine_sorted(&vec![]), sha256(&SCOPE_V1_TAG));
}

#[test]
fn a_changed_digest_changes_the_hash() {
    let before = vec![pair("a", 1), pair("b", 2)];
    let after = vec![pair("a", 1), pair("b", 3)];
    assert_ne!(combine_sorted(&before), combine_sorted(&after));
}

// ---------------------------------------------------------------------------
// U3: the sort, the full scope hash, and distinctness
// ---------------------------------------------------------------------------

use proob_core::scope_hash::{ScopeHash, paths_distinct_exec, scope_hash, sort_by_path};

fn key(p: &PathDigest) -> (Vec<u8>, [u8; 32]) {
    (p.path.clone(), p.digest)
}

#[test]
fn sort_by_path_orders_and_keeps_every_pair() {
    let input = vec![pair("b", 1), pair("a/b", 2), pair("a", 3), pair("ab", 4)];
    let mut expected: Vec<_> = input.iter().map(key).collect();
    expected.sort();
    let sorted = sort_by_path(input);
    assert_eq!(sorted.iter().map(key).collect::<Vec<_>>(), expected);
    assert!(is_strictly_sorted(&sorted));
}

#[test]
fn scope_hash_is_the_same_in_any_order_and_differs_by_digest() {
    let one = vec![pair("src/a.rs", 1), pair("src/b.rs", 2), pair("src/c.rs", 3)];
    let other = vec![pair("src/c.rs", 3), pair("src/a.rs", 1), pair("src/b.rs", 2)];
    let changed = vec![pair("src/c.rs", 3), pair("src/a.rs", 1), pair("src/b.rs", 9)];
    let sorted = vec![pair("src/a.rs", 1), pair("src/b.rs", 2), pair("src/c.rs", 3)];
    let expected = ScopeHash::Ok(combine_sorted(&sorted));
    assert_eq!(scope_hash(one), expected);
    assert_eq!(scope_hash(other), expected);
    assert_ne!(scope_hash(changed), expected);
}

#[test]
fn a_duplicate_path_has_no_scope_hash() {
    assert!(!paths_distinct_exec(&vec![pair("a", 1), pair("b", 2), pair("a", 3)]));
    assert_eq!(scope_hash(vec![pair("a", 1), pair("b", 2), pair("a", 3)]), ScopeHash::DuplicatePath);
    assert!(paths_distinct_exec(&vec![pair("a", 1), pair("b", 2), pair("ab", 3)]));
    assert!(paths_distinct_exec(&vec![]));
}
