//! Content hash of a governed file (`proob-content-v1`).
//!
//! The facts crate normalises the file (LF line endings, trailing whitespace
//! trimmed per line, UTF-8) and hands the bytes here; the hash is SHA-256 over
//! the algorithm tag followed by those bytes. The tag is hashed first so a
//! content hash can never coincide with a scope or binding hash of the same
//! bytes. Governed files are approved by content, so this is the identity an
//! approval is about.
use proob_trusted::sha256;
// Spec functions are erased by a plain `cargo build`; the guard keeps this
// import to verification only.
#[cfg(verus_only)]
use proob_trusted::sha256_spec;
use vstd::prelude::*;

verus! {

/// The algorithm tag `proob-content-v1`.
pub const CONTENT_V1_TAG: [u8; 16] = [
    0x70, 0x72, 0x6f, 0x6f, 0x62, 0x2d, 0x63, 0x6f, 0x6e, 0x74, 0x65, 0x6e, 0x74, 0x2d, 0x76, 0x31,
];

/// The content hash as a mathematical function of the normalised bytes.
pub open spec fn spec_content_hash(bytes: Seq<u8>) -> Seq<u8> {
    sha256_spec(CONTENT_V1_TAG@ + bytes)
}

/// Computes `spec_content_hash`.
pub fn content_hash(bytes: &[u8]) -> (out: [u8; 32])
    ensures
        out@ == spec_content_hash(bytes@),
{
    let tag = CONTENT_V1_TAG;
    let mut frame: Vec<u8> = Vec::new();
    frame.extend_from_slice(tag.as_slice());
    frame.extend_from_slice(bytes);
    proof {
        assert(frame@ =~= CONTENT_V1_TAG@ + bytes@);
    }
    sha256(frame.as_slice())
}

} // verus!
