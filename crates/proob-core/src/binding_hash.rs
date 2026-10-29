//! The binding hash (`proob-binding-v1`): the identity of a bound check's
//! binding set, which is what the check's approval and its score evidence are
//! about.
//!
//! A binding set is a set of (path, content digest) pairs like a scope, so
//! its hash is the scope hash of those pairs under a second tag: SHA-256 over
//! `proob-binding-v1` followed by the pairs' scope hash. The tag keeps a
//! binding hash from ever coinciding with the scope hash of the same files,
//! and the inner scope hash carries the order independence proven in
//! `scope_hash` (`spec_binding_hash_of` is defined on the multiset). A
//! duplicate path has no canonical form and answers `None`; the effects layer
//! rules it out with `paths_distinct_exec` first.
use crate::scope_hash::{PathDigest, ScopeHash, scope_hash};
use crate::types::Digest;
use proob_trusted::sha256;
// Spec functions are erased by a plain `cargo build`; the guards keep these
// imports to verification only.
#[cfg(verus_only)]
use crate::scope_hash::{paths_distinct, spec_scope_hash_of};
#[cfg(verus_only)]
use proob_trusted::sha256_spec;
#[cfg(verus_only)]
use vstd::multiset::Multiset;
use vstd::prelude::*;

verus! {

/// The algorithm tag `proob-binding-v1`.
pub const BINDING_V1_TAG: [u8; 16] = [
    0x70, 0x72, 0x6f, 0x6f, 0x62, 0x2d, 0x62, 0x69, 0x6e, 0x64, 0x69, 0x6e, 0x67, 0x2d, 0x76, 0x31,
];

/// The binding hash of a multiset of pairs.
pub open spec fn spec_binding_hash_of(m: Multiset<PathDigest>) -> Seq<u8> {
    sha256_spec(BINDING_V1_TAG@ + spec_scope_hash_of(m))
}

/// The binding hash of any pair list: `Some` exactly when the paths are
/// distinct, and then a function of the multiset of pairs.
pub fn binding_hash(pairs: Vec<PathDigest>) -> (r: Option<Digest>)
    ensures
        r is Some <==> paths_distinct(pairs@),
        r matches Some(h) ==> h@ == spec_binding_hash_of(pairs@.to_multiset()),
{
    match scope_hash(pairs) {
        ScopeHash::Ok(inner) => {
            let tag = BINDING_V1_TAG;
            let mut frame: Vec<u8> = Vec::new();
            frame.extend_from_slice(tag.as_slice());
            frame.extend_from_slice(inner.as_slice());
            proof {
                assert(frame@ =~= BINDING_V1_TAG@ + inner@);
            }
            Some(sha256(frame.as_slice()))
        },
        ScopeHash::DuplicatePath => None,
    }
}

} // verus!
