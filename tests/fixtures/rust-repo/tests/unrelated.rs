//! A test with no binding token. It pins `count_evens` exactly and would kill
//! every mutant of `src/parity.rs` if it ran during a score; suite restriction
//! to the bound test keeps it out of the vacuous property's kill count.
use rust_repo::parity::count_evens;

#[test]
fn count_evens_is_exact() {
    assert_eq!(count_evens(&[]), 0);
    assert_eq!(count_evens(&[1, 2, 3, 4]), 2);
    assert_eq!(count_evens(&[7]), 0);
}
