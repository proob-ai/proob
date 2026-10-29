//! ULID text, Crockford base32, to and from its 128-bit value.
//!
//! Twenty-six characters over `0-9 A-Z` without `I L O U`; decoding accepts
//! lower case and rejects everything else. The first character carries three
//! significant bits, so it must be `0`–`7` for the value to fit.
use std::fmt;

pub const LEN: usize = 26;
pub const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UlidError {
    Length(usize),
    Char { index: usize, byte: u8 },
    Overflow,
}

impl fmt::Display for UlidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UlidError::Length(n) => write!(f, "expected {LEN} Crockford base32 characters, found {n}"),
            UlidError::Char { index, byte } => {
                write!(f, "byte {byte:#04x} at index {index} is not Crockford base32 (0-9, A-Z without I, L, O, U)")
            }
            UlidError::Overflow => write!(f, "first character must be 0-7: the value does not fit 128 bits"),
        }
    }
}

impl std::error::Error for UlidError {}

/// The value of one Crockford digit, case-insensitive.
fn digit(b: u8) -> Option<u8> {
    match b.to_ascii_uppercase() {
        c @ b'0'..=b'9' => Some(c - b'0'),
        c @ b'A'..=b'H' => Some(c - b'A' + 10),
        c @ b'J'..=b'K' => Some(c - b'J' + 18),
        c @ b'M'..=b'N' => Some(c - b'M' + 20),
        c @ b'P'..=b'T' => Some(c - b'P' + 22),
        c @ b'V'..=b'Z' => Some(c - b'V' + 27),
        _ => None,
    }
}

fn digits_of(text: &str) -> Result<Vec<u8>, UlidError> {
    let count = text.chars().count();
    if count != LEN {
        return Err(UlidError::Length(count));
    }
    text.bytes().enumerate().map(|(index, byte)| digit(byte).ok_or(UlidError::Char { index, byte })).collect()
}

/// Parses ULID text.
pub fn decode(text: &str) -> Result<u128, UlidError> {
    let digits = digits_of(text)?;
    match digits.first() {
        Some(first) if *first > 7 => Err(UlidError::Overflow),
        _ => Ok(digits.iter().fold(0u128, |acc, &d| (acc << 5) | u128::from(d))),
    }
}

/// The canonical (upper-case) text of a value. `decode(&encode(v)) == Ok(v)`.
pub fn encode(value: u128) -> String {
    (0..LEN).rev().map(|i| char::from(ALPHABET[usize::from(((value >> (5 * i)) & 0x1f) as u8)])).collect()
}
