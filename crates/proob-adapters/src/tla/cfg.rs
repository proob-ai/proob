//! The model a check runs: the module's own `.cfg` with only the bound
//! property checked. Every section is kept as written (the specification,
//! the constants, symmetry, state constraints, `CHECK_DEADLOCK`: the bounds
//! of the claim the reviewer approved) but `INVARIANT` and `PROPERTY`, which
//! are replaced by the one bound name, under the keyword the cfg lists it
//! under, else `INVARIANT`. Pure.
use crate::tla::scan::{blank, words};

const KEYWORDS: [&str; 18] = [
    "CONSTANT",
    "CONSTANTS",
    "INIT",
    "NEXT",
    "SPECIFICATION",
    "INVARIANT",
    "INVARIANTS",
    "PROPERTY",
    "PROPERTIES",
    "SYMMETRY",
    "VIEW",
    "CONSTRAINT",
    "CONSTRAINTS",
    "ACTION_CONSTRAINT",
    "ACTION_CONSTRAINTS",
    "CHECK_DEADLOCK",
    "POSTCONDITION",
    "ALIAS",
];
const INVARIANTS: [&str; 2] = ["INVARIANT", "INVARIANTS"];
const PROPERTIES: [&str; 2] = ["PROPERTY", "PROPERTIES"];

/// The keyword TLC checks a name under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checked {
    Invariant,
    Property,
}

/// Each section of a cfg, comments blanked: its keyword and its body.
pub fn sections(cfg: &str) -> Vec<(&'static str, String)> {
    let text = blank(cfg, false);
    let starts: Vec<(usize, &'static str)> =
        words(&text).into_iter().filter_map(|(at, w)| KEYWORDS.iter().find(|k| **k == w).map(|k| (at, *k))).collect();
    starts
        .iter()
        .enumerate()
        .map(|(i, (at, k))| {
            let end = starts.get(i + 1).map_or(text.len(), |(next, _)| *next);
            (*k, text[at + k.len()..end].trim().to_string())
        })
        .collect()
}

/// The names listed under any of `keywords`.
pub fn names(cfg: &str, keywords: &[&str]) -> Vec<String> {
    sections(cfg)
        .iter()
        .filter(|(k, _)| keywords.contains(k))
        .flat_map(|(_, body)| words(body).into_iter().map(|(_, w)| w.to_string()).collect::<Vec<_>>())
        .filter(|w| w.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'))
        .collect()
}

/// How the cfg checks `name`: under `PROPERTY` when it lists it there.
pub fn checked_as(cfg: &str, name: &str) -> Checked {
    if names(cfg, &PROPERTIES).iter().any(|n| n == name) { Checked::Property } else { Checked::Invariant }
}

/// The cfg with only `name` checked.
pub fn only(cfg: &str, name: &str) -> String {
    let keyword = match checked_as(cfg, name) {
        Checked::Invariant => "INVARIANT",
        Checked::Property => "PROPERTY",
    };
    sections(cfg)
        .into_iter()
        .filter(|(k, _)| !INVARIANTS.contains(k) && !PROPERTIES.contains(k))
        .map(|(k, body)| format!("{k}\n{body}\n"))
        .chain([format!("{keyword}\n{name}\n")])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = "\\* the model\nSPECIFICATION Spec\nCONSTANTS\n  Procs = {p1, p2}\n  Max = 3 (* small *)\nSYMMETRY Perms\nCONSTRAINT Bounded\nINVARIANT TypeOK Safety\nINVARIANTS Other\nPROPERTY Live\nCHECK_DEADLOCK FALSE\n";

    #[test]
    fn only_the_bound_name_is_checked_and_the_rest_is_kept() {
        let out = only(CFG, "Safety");
        assert_eq!(
            out,
            "SPECIFICATION\nSpec\nCONSTANTS\nProcs = {p1, p2}\n  Max = 3\nSYMMETRY\nPerms\nCONSTRAINT\nBounded\nCHECK_DEADLOCK\nFALSE\nINVARIANT\nSafety\n"
        );
        assert!(only(CFG, "Live").ends_with("PROPERTY\nLive\n"), "a property stays a property");
        assert!(only(CFG, "Unlisted").ends_with("INVARIANT\nUnlisted\n"), "an unlisted name is an invariant");
        assert!(!only(CFG, "Live").contains("TypeOK"));
    }

    #[test]
    fn names_by_section_skip_comments_and_values() {
        assert_eq!(names(CFG, &["SPECIFICATION"]), vec!["Spec"]);
        assert_eq!(names(CFG, &INVARIANTS), vec!["TypeOK", "Safety", "Other"]);
        assert_eq!(names("INIT Init NEXT Next \\* SPECIFICATION Spec\n", &["SPECIFICATION"]), Vec::<String>::new());
        assert_eq!(checked_as(CFG, "Live"), Checked::Property);
        assert_eq!(checked_as(CFG, "TypeOK"), Checked::Invariant);
    }
}
