//! The closed grammar of a property's scope glob.
//!
//! A scope glob is root-relative and reaches a child process only as an
//! argument after the end-of-options separator, so the grammar is what keeps a
//! property file from steering a runner: no leading `-` (never an option), no
//! `..` segment (never above the root), no leading `/` (never absolute), bytes
//! from a short whitelist, non-empty. The whitelist is ASCII letters, digits,
//! and `/ . _ - * ? [ ] { } , @ + ~`.
//!
//! `glob_ok` is the definition; `glob_ok_exec` is proven to decide it.
//! `scope_ok` lifts it to a property's scope list (non-empty, every glob ok).
//!
//! ULIDs need no grammar here: every `u128` is a valid ULID value, and the
//! 26-character Crockford text form is parsed in the facts crate.
use vstd::prelude::*;

verus! {

/// Bytes a glob may contain.
pub open spec fn glob_byte_ok(b: u8) -> bool {
    (b'a' <= b && b <= b'z') || (b'A' <= b && b <= b'Z') || (b'0' <= b && b <= b'9')
        || b == b'/' || b == b'.' || b == b'_' || b == b'-' || b == b'*' || b == b'?'
        || b == b'[' || b == b']' || b == b'{' || b == b'}' || b == b',' || b == b'@'
        || b == b'+' || b == b'~'
}

/// `..` occupies a whole segment starting at `i`: preceded by the start or a
/// `/`, followed by the end or a `/`.
pub open spec fn parent_segment_at(s: Seq<u8>, i: int) -> bool {
    0 <= i && i + 1 < s.len() && s[i] == b'.' && s[i + 1] == b'.' && (i == 0 || s[i - 1] == b'/')
        && (i + 2 == s.len() || s[i + 2] == b'/')
}

/// The grammar.
pub open spec fn glob_ok(s: Seq<u8>) -> bool {
    s.len() > 0 && s[0] != b'/' && s[0] != b'-'
        && (forall|i: int| 0 <= i < s.len() ==> glob_byte_ok(#[trigger] s[i]))
        && (forall|i: int| 0 <= i < s.len() ==> !#[trigger] parent_segment_at(s, i))
}

/// A property's scope: at least one glob, every glob in the grammar.
pub open spec fn scope_ok(scope: Seq<Vec<u8>>) -> bool {
    scope.len() > 0 && (forall|i: int| 0 <= i < scope.len() ==> glob_ok((#[trigger] scope[i])@))
}

/// Decides `glob_byte_ok`.
pub fn glob_byte_ok_exec(b: u8) -> (r: bool)
    ensures
        r == glob_byte_ok(b),
{
    (b'a' <= b && b <= b'z') || (b'A' <= b && b <= b'Z') || (b'0' <= b && b <= b'9')
        || b == b'/' || b == b'.' || b == b'_' || b == b'-' || b == b'*' || b == b'?'
        || b == b'[' || b == b']' || b == b'{' || b == b'}' || b == b',' || b == b'@'
        || b == b'+' || b == b'~'
}

/// Decides `parent_segment_at` for an index inside the slice.
fn parent_segment_at_exec(s: &[u8], i: usize) -> (r: bool)
    requires
        i < s.len(),
    ensures
        r == parent_segment_at(s@, i as int),
{
    let n = s.len();
    if i + 1 >= n {
        return false;
    }
    if s[i] != b'.' || s[i + 1] != b'.' {
        return false;
    }
    let starts_segment = i == 0 || s[i - 1] == b'/';
    let ends_segment = i + 2 == n || s[i + 2] == b'/';
    starts_segment && ends_segment
}

/// Decides `glob_ok`.
pub fn glob_ok_exec(s: &[u8]) -> (r: bool)
    ensures
        r == glob_ok(s@),
{
    let n = s.len();
    if n == 0 {
        return false;
    }
    if s[0] == b'/' || s[0] == b'-' {
        return false;
    }
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == s@.len(),
            n > 0,
            s@[0] != b'/',
            s@[0] != b'-',
            forall|j: int| 0 <= j < i ==> glob_byte_ok(#[trigger] s@[j]),
            forall|j: int| 0 <= j < i ==> !#[trigger] parent_segment_at(s@, j),
        decreases n - i,
    {
        if !glob_byte_ok_exec(s[i]) {
            assert(!glob_byte_ok(s@[i as int]));
            return false;
        }
        if parent_segment_at_exec(s, i) {
            assert(parent_segment_at(s@, i as int));
            return false;
        }
        i += 1;
    }
    true
}

/// Decides `scope_ok`.
pub fn scope_ok_exec(scope: &Vec<Vec<u8>>) -> (r: bool)
    ensures
        r == scope_ok(scope@),
{
    let n = scope.len();
    if n == 0 {
        return false;
    }
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            n == scope@.len(),
            n > 0,
            forall|j: int| 0 <= j < i ==> glob_ok((#[trigger] scope@[j])@),
        decreases n - i,
    {
        if !glob_ok_exec(scope[i].as_slice()) {
            assert(!glob_ok(scope@[i as int]@));
            return false;
        }
        i += 1;
    }
    true
}

} // verus!
