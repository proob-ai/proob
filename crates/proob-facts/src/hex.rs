//! Lower- or upper-case hex to and from the core's 32-byte digests and object
//! ids. A git SHA-1 id (40 characters) is left-aligned in the 32 bytes with
//! the last 12 bytes zero; `encode_oid` reverses that, so an id round-trips
//! through the core and back to git or GitHub unchanged.
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexError {
    Length(usize),
    Char { index: usize, byte: u8 },
}

impl fmt::Display for HexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HexError::Length(n) => write!(f, "expected 64 hex characters (or 40 for a git SHA-1 id), found {n}"),
            HexError::Char { index, byte } => write!(f, "byte {byte:#04x} at index {index} is not hex"),
        }
    }
}

impl std::error::Error for HexError {}

fn nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn byte_at(text: &[u8], index: usize) -> Result<u8, HexError> {
    match (text.get(index), text.get(index + 1)) {
        (Some(&hi), Some(&lo)) => Ok((nibble(hi).ok_or(HexError::Char { index, byte: hi })? << 4)
            | nibble(lo).ok_or(HexError::Char { index: index + 1, byte: lo })?),
        _ => Err(HexError::Length(text.len())),
    }
}

/// Decodes `text` (an even number of hex characters, at most 64) into the
/// first bytes of a zero-filled 32-byte array.
fn decode_left_aligned(text: &str) -> Result<[u8; 32], HexError> {
    let bytes = text.as_bytes();
    let decoded = (0..bytes.len() / 2).map(|i| byte_at(bytes, 2 * i)).collect::<Result<Vec<u8>, _>>()?;
    Ok(std::array::from_fn(|i| decoded.get(i).copied().unwrap_or(0)))
}

/// A SHA-256 digest: exactly 64 hex characters.
pub fn decode_digest(text: &str) -> Result<[u8; 32], HexError> {
    match text.len() {
        64 => decode_left_aligned(text),
        n => Err(HexError::Length(n)),
    }
}

/// A git object id: 40 (SHA-1) or 64 (SHA-256) hex characters.
pub fn decode_oid(text: &str) -> Result<[u8; 32], HexError> {
    match text.len() {
        40 | 64 => decode_left_aligned(text),
        n => Err(HexError::Length(n)),
    }
}

/// Lower-case hex of any bytes.
pub fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A git object id as git writes it: 40 characters when the last 12 bytes are
/// zero (a SHA-1 id as `decode_oid` stores it), 64 otherwise. A SHA-256 id
/// whose last 12 bytes happen to be zero would print as 40 characters; the
/// chance is 2^-96 and git would then report the ambiguity rather than act on
/// a wrong object.
pub fn encode_oid(oid: &[u8; 32]) -> String {
    if oid[20..].iter().all(|b| *b == 0) { encode(&oid[..20]) } else { encode(oid) }
}
