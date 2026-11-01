//! The two ways a file can fail: it is not its schema, or it is the schema but
//! a value has no core meaning. Both name the key or field path.
use crate::convert::GlobRule;
use crate::hex::HexError;
use crate::ulid::UlidError;
use std::fmt;

/// The text is not an instance of the schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The file carries a `status` key. proob never stores status: it is
    /// derived at read time from review records and evidence, so the key is
    /// refused rather than ignored.
    StatusKey,
    /// A syntax error, an unknown key, a missing key, or a wrong type; the
    /// message names the key and the position.
    Schema(String),
    /// A value outside the range the schema allows.
    Invalid { field: &'static str, message: &'static str },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::StatusKey => write!(
                f,
                "`status` is not a property field: proob never stores status; it is derived at read \
                 time from review records and evidence, so remove the key"
            ),
            ParseError::Schema(message) => write!(f, "{message}"),
            ParseError::Invalid { field, message } => write!(f, "`{field}`: {message}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A well-formed file whose value has no core meaning, at the named field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertError {
    /// Path of the offending field, such as `scope[1]` or `waiver.until`.
    pub field: String,
    pub kind: ConvertErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertErrorKind {
    Ulid(UlidError),
    Hex(HexError),
    Glob(GlobRule),
    /// A scope must list at least one glob.
    EmptyScope,
    /// The waiver window is longer than `waiver_max_days`.
    WaiverTooLong {
        days: u64,
        max_days: u32,
    },
    /// The kill floor's schema minimum is one.
    FloorZero,
    /// An evidence record's outcome is not one its kind produces.
    Incoherent {
        kind: &'static str,
        outcome: &'static str,
    },
    /// A `verified` record carries neither or both of its measures.
    ProofMeasure,
}

impl ConvertError {
    pub fn at(field: impl Into<String>, kind: impl Into<ConvertErrorKind>) -> Self {
        ConvertError { field: field.into(), kind: kind.into() }
    }
}

impl From<UlidError> for ConvertErrorKind {
    fn from(e: UlidError) -> Self {
        ConvertErrorKind::Ulid(e)
    }
}

impl From<HexError> for ConvertErrorKind {
    fn from(e: HexError) -> Self {
        ConvertErrorKind::Hex(e)
    }
}

impl From<GlobRule> for ConvertErrorKind {
    fn from(r: GlobRule) -> Self {
        ConvertErrorKind::Glob(r)
    }
}

impl fmt::Display for ConvertErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvertErrorKind::Ulid(e) => write!(f, "not a ULID: {e}"),
            ConvertErrorKind::Hex(e) => write!(f, "not a hex digest: {e}"),
            ConvertErrorKind::Glob(rule) => write!(f, "not a scope glob: {rule}"),
            ConvertErrorKind::EmptyScope => {
                write!(f, "scope is empty: a property's scope lists at least one glob")
            }
            ConvertErrorKind::WaiverTooLong { days, max_days } => {
                write!(f, "waiver window of {days} days exceeds the maximum of {max_days} days (waiver_max_days)")
            }
            ConvertErrorKind::FloorZero => {
                write!(f, "kill floor is zero: the schema minimum is one and config cannot lower it")
            }
            ConvertErrorKind::Incoherent { kind, outcome } => {
                write!(f, "outcome `{outcome}` is not one that `{kind}` evidence produces")
            }
            ConvertErrorKind::ProofMeasure => {
                write!(f, "a `verified` outcome carries exactly one of `rlimit` (Verus) or `checks` (Kani)")
            }
        }
    }
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.kind)
    }
}

impl std::error::Error for ConvertError {}
