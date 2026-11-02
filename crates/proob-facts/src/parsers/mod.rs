//! Parsers for runner reports: text an adapter's tool wrote, turned into
//! closed shapes an adapter then maps to outcomes. Nothing here judges a
//! result; a report that is not what the tool documents fails closed as a
//! `ParseError` naming what was wrong.
pub mod cargo_mutants;
pub mod junit;
pub mod kani;
pub mod mutmut;
pub mod tlc;
pub mod verus;
pub mod xml;
