//! The affected set: a property is affected by a change when some changed
//! path is one of its scope files or one of its bound check's binding-set
//! files.
//!
//! Paths are resolved files, byte-exact, root-relative; glob resolution and
//! binding-set computation happen in the effects and adapter layers, which
//! hand the resolved lists in. Membership is content equality of the bytes.
use crate::bytes::bytes_eq;
use vstd::prelude::*;

verus! {

/// `p` is the content of some entry of `files`.
pub open spec fn has_path(files: Seq<Vec<u8>>, p: Seq<u8>) -> bool {
    exists|j: int| 0 <= j < files.len() && (#[trigger] files[j])@ == p
}

/// Some changed path is a scope file or a binding file.
pub open spec fn spec_affected(changed: Seq<Vec<u8>>, scope_files: Seq<Vec<u8>>, binding_files: Seq<Vec<u8>>) -> bool {
    exists|i: int|
        0 <= i < changed.len() && (has_path(scope_files, (#[trigger] changed[i])@) || has_path(
            binding_files,
            changed[i]@,
        ))
}

/// Decides `has_path`.
pub fn contains_path(files: &Vec<Vec<u8>>, p: &Vec<u8>) -> (r: bool)
    ensures
        r == has_path(files@, p@),
{
    let n = files.len();
    let mut j: usize = 0;
    while j < n
        invariant
            j <= n,
            n == files@.len(),
            forall|k: int| 0 <= k < j ==> (#[trigger] files@[k])@ != p@,
        decreases n - j,
    {
        if bytes_eq(files[j].as_slice(), p.as_slice()) {
            return true;
        }
        j += 1;
    }
    false
}

/// Decides `spec_affected`.
pub fn affected(changed: &Vec<Vec<u8>>, scope_files: &Vec<Vec<u8>>, binding_files: &Vec<Vec<u8>>) -> (r: bool)
    ensures
        r == spec_affected(changed@, scope_files@, binding_files@),
{
    let n = changed.len();
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == changed@.len(),
            forall|k: int|
                0 <= k < i ==> !has_path(scope_files@, (#[trigger] changed@[k])@) && !has_path(
                    binding_files@,
                    changed@[k]@,
                ),
        decreases n - i,
    {
        if contains_path(scope_files, &changed[i]) || contains_path(binding_files, &changed[i]) {
            return true;
        }
        i += 1;
    }
    false
}

} // verus!
