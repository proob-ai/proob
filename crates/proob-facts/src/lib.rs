//! proob's data-transfer layer.
//!
//! The verified core speaks only in numbers and bytes; files and APIs speak in
//! text. This crate is where text becomes numbers, once, at the boundary: the
//! TOML property and config files (`property`, `config`), the evidence JSON
//! record (`evidence`), the GitHub payloads the effects layer reads
//! (`github`), the run-result record a PR or local run writes (`results`),
//! the conversions of each into core types (`convert`), and the paths proob's
//! files live at (`paths`), with ULID, hex, and date codecs beneath them.
//!
//! Nothing here carries a verdict. There is no `serde` type for `Approval`,
//! `Status`, or `Verdict`, so no file, cache entry, or payload can supply one;
//! that is the structural half of assertion 6 in `proob-core/ASSERTIONS.md`.
pub mod config;
pub mod convert;
pub mod date;
pub mod error;
pub mod evidence;
pub mod github;
pub mod hex;
pub mod parsers;
pub mod paths;
pub mod property;
pub mod results;
pub mod ulid;
