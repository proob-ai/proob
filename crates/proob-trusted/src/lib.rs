//! The trusted base of proob.
//!
//! Every exec function here is `#[verifier::external_body]`: Verus trusts the
//! contract and never reads the body. README.md lists each one and why it is
//! trusted. Nothing else in the verified core may introduce an assumption;
//! `cargo verus verify -p proob-core --fwd-verus-args-to roots -- --no-cheating`
//! enforces that on `proob-core` while leaving this crate's contracts in place.
use sha2::Digest as _;
use vstd::prelude::*;

verus! {

/// SHA-256 as a mathematical function on byte sequences.
///
/// Uninterpreted: a proof may use only that it is a function (equal inputs give
/// equal digests). No collision, preimage, or length axiom is stated; the exec
/// wrapper's return type already fixes the width at 32 bytes.
pub uninterp spec fn sha256_spec(bytes: Seq<u8>) -> Seq<u8>;

/// Trusted: the `sha2` crate's SHA-256.
///
/// The verifier learns exactly one fact about the result: it is `sha256_spec`
/// of the input. Everything the verified core proves about hashes rests on
/// `sha2` computing SHA-256 correctly.
#[verifier::external_body]
pub fn sha256(bytes: &[u8]) -> (digest: [u8; 32])
    ensures
        digest@ == sha256_spec(bytes@),
{
    sha2::Sha256::digest(bytes).into()
}

/// Trusted: the platform clock, in whole seconds since the Unix epoch.
///
/// `None` when the platform clock is set before the epoch, rather than a fake
/// zero. Carries no specification: nothing verified may depend on what time it
/// is, only on the value it was handed.
#[verifier::external_body]
pub fn now_secs() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs())
}

} // verus!
