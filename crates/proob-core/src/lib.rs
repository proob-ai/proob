//! proob's verified core.
//!
//! Everything in this crate is checked by Verus under `--no-cheating`: no
//! `assume`, no `admit`, no `external_body`, no `assume_specification`. The only
//! trusted facts come from `proob-trusted`, whose README lists them.
//!
//! Data is plain: ids as `u128`, digests as `[u8; 32]`, paths as byte vectors,
//! times as `u64`. No `serde`, no `String`, no closures that capture mutably, no
//! trait objects, no async; these stay in the facts and effects crates.
//!
//! The six assertions that define `Holds` are written as `ensures` clauses on
//! exec functions, never as tests; ASSERTIONS.md is their authoritative prose
//! and `specs` their definitions.
//!
//! Modules: `types` (the closed data types), `grammar` (scope globs),
//! `bytes` (content equality of paths and digests), `content_hash`,
//! `scope_hash`, and `binding_hash` (the three hashes approvals and evidence
//! are about, with the verified sort that makes the scope hash a function of
//! the set of files), `specs` (the assertions and well-formedness predicates),
//! `eligibility` (review facts to `Approval`), `preview` (the same, before a
//! merge, with a remedy), `governance` (the one constructor of `Governance`),
//! `evidence_accept` (which workflow runs may supply evidence), `derive`
//! (facts to `Status`), `verdict` (`Status` to the closed word), `policy` (the
//! word to the gate's outcome), and `affected` (which properties a change
//! touches).
// Two lints are wrong for Verus code, so they are allowed here and nowhere else.
#![allow(
    clippy::ptr_arg,
    reason = "the specs view these arguments as `v@` of a `Vec`; a slice parameter would change every verified signature"
)]
#![allow(
    clippy::manual_range_contains,
    reason = "vstd gives `RangeInclusive::contains` no specification, so Verus cannot reason about it"
)]
pub mod affected;
pub mod binding_hash;
pub mod bytes;
pub mod content_hash;
pub mod derive;
pub mod eligibility;
pub mod evidence_accept;
pub mod governance;
pub mod grammar;
pub mod policy;
pub mod preview;
pub mod scope_hash;
pub mod specs;
pub mod types;
pub mod verdict;

pub use content_hash::{CONTENT_V1_TAG, content_hash};
