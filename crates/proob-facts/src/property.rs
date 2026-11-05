//! The property file: `proob/properties/<id>.toml`.
//!
//! What a team agrees to in plain English, with the files it is about. There
//! is no `status` field, by design: status is derived at read time and never
//! stored, so a `status` key is refused with that explanation before any
//! other schema error. Every other unknown key is refused by the schema.
//!
//! Approval is by content, so the content hash is defined here too: the text
//! normalised to LF line endings with trailing whitespace trimmed per line,
//! through the verified `proob-content-v1` hash.
use crate::date::Date;
use crate::error::ParseError;
use serde::{Deserialize, Serialize};

/// The key proob refuses. Named once so the hook (U11) and the parser agree.
pub const STATUS_KEY: &str = "status";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyFile {
    /// A 26-character Crockford base32 ULID.
    pub id: String,
    pub title: String,
    /// One sentence, present tense, falsifiable.
    pub statement: String,
    /// Root-relative globs; at least one (checked at conversion).
    pub scope: Vec<String>,
    pub severity: SeverityName,
    pub rationale: String,
    /// Absent: the bound check's adapter decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<KindName>,
    /// The date a human last re-affirmed the statement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed: Option<Date>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiver: Option<WaiverFile>,
}

/// Declared in rising order, so `Ord` ranks `Critical` highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeverityName {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KindName {
    Test,
    Proof,
    Model,
}

/// A waiver is content: changing it changes the file, which needs a review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaiverFile {
    pub until: Date,
    /// The tracking issue, which must be open for the waiver to render.
    pub issue: String,
    pub reason: String,
}

/// Parses a property file, refusing a `status` key by name first.
pub fn parse_property(text: &str) -> Result<PropertyFile, ParseError> {
    reject_status_key(text)?;
    toml::from_str(text).map_err(|e| ParseError::Schema(e.to_string()))
}

fn reject_status_key(text: &str) -> Result<(), ParseError> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| ParseError::Schema(e.to_string()))?;
    if table.contains_key(STATUS_KEY) { Err(ParseError::StatusKey) } else { Ok(()) }
}

/// The normal form the content hash is over: each line without trailing
/// whitespace, LF-terminated, no trailing blank lines; empty text stays empty.
pub fn normalise(text: &str) -> String {
    let body: Vec<&str> = text.lines().map(str::trim_end).collect();
    let trimmed = body.join("\n");
    let trimmed = trimmed.trim_end_matches('\n');
    if trimmed.is_empty() { String::new() } else { format!("{trimmed}\n") }
}

/// The `proob-content-v1` hash of the normalised text.
pub fn content_hash_of(text: &str) -> [u8; 32] {
    proob_core::content_hash(normalise(text).as_bytes())
}

/// `normalise` for a file that may not be text: UTF-8 is normalised as text,
/// anything else is hashed as it is (A8: line endings and trailing whitespace
/// only; nothing formatter-aware).
pub fn normalise_bytes(bytes: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(bytes) {
        Ok(text) => normalise(text).into_bytes(),
        Err(_) => bytes.to_vec(),
    }
}

/// The `proob-content-v1` hash of any file's normalised bytes: the digest a
/// scope or binding set carries for each member.
pub fn file_digest(bytes: &[u8]) -> [u8; 32] {
    proob_core::content_hash(&normalise_bytes(bytes))
}
