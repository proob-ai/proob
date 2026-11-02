//! Spec mutants (`tla-spec-mutants-v1`): small edits to the bodies of the
//! definitions the cfg's `NEXT` or `SPECIFICATION` reaches by name in the
//! bound module, never the bound definition, the initial predicate (the
//! cfg's `INIT`, else `Init`), or the cfg. A mutant the bound property
//! catches shows it can fail. Pure.
//!
//! Operators, on code only (comments and strings are blanked first):
//!
//! - `=` ↔ `#`, except where the left side is primed (`x' = e` assigns, it
//!   does not compare) or an `EXCEPT` path (`![p] = e`), and never inside
//!   `==`, `/=`, `<=`, `>=`, `=<`, `=>`, `<=>`, `|=`, `:=`;
//! - `<` ↔ `<=` and `>` ↔ `>=`, never inside `<<`, `>>`, `<>`, `<-`, `->`,
//!   `|->`, `=>`, `<=>`, `~>`, `:>`, `<:`;
//! - `+ 1` ↔ `- 1` after an operand;
//! - one conjunct of a bulleted `/\` list dropped, when the list has another
//!   at the same column;
//! - `TRUE` ↔ `FALSE`.
//!
//! shortcut: reachability is by name, not by scope, so a local name that
//! shadows a definition reaches it too; the extra mutants are only surplus.
use crate::tla::scan::{Definition, definitions, lines, masked, words};
use std::collections::BTreeSet;
use std::ops::Range;

pub const OPERATOR_SET: &str = "tla-spec-mutants-v1";

/// One edit of the module text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutant {
    /// `<module>:<line>:<column>: <edit>`, unique within a module.
    pub name: String,
    pub range: Range<usize>,
    pub replacement: String,
}

impl Mutant {
    pub fn apply(&self, text: &str) -> String {
        format!("{}{}{}", &text[..self.range.start], self.replacement, &text[self.range.end..])
    }
}

/// Every mutant of `text`, in text order, over the bodies `roots` reach.
pub fn mutants(text: &str, module: &str, roots: &[String], excluded: &[String]) -> Vec<Mutant> {
    let m = masked(text);
    let defs = definitions(&m);
    let mut edits: Vec<(Range<usize>, String, String)> = reachable(&defs, &m, roots, excluded)
        .into_iter()
        .flat_map(|d| {
            let body = defs[d].body.clone();
            [swaps(&m, body.clone()), steps(&m, body.clone()), truths(&m, body.clone()), conjuncts(text, &m, body)]
                .concat()
        })
        .collect();
    edits.sort_by_key(|(r, _, _)| (r.start, r.end));
    edits
        .into_iter()
        .map(|(range, replacement, what)| {
            let (line, col) = position(text, range.start);
            Mutant { name: format!("{module}:{line}:{col}: {what}"), range, replacement }
        })
        .collect()
}

/// Indices of the definitions reachable from `roots` by name, `excluded` cut out.
pub fn reachable(defs: &[Definition], masked: &str, roots: &[String], excluded: &[String]) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    let mut todo: Vec<String> = roots.to_vec();
    while let Some(name) = todo.pop() {
        let Some(i) = defs.iter().position(|d| d.name == name) else { continue };
        if excluded.contains(&name) || !seen.insert(i) {
            continue;
        }
        todo.extend(words(&masked[defs[i].body.clone()]).into_iter().map(|(_, w)| w.to_string()));
    }
    seen.into_iter().collect()
}

/// 1-based line and column of byte `at`.
fn position(text: &str, at: usize) -> (usize, usize) {
    let before = &text[..at];
    (before.matches('\n').count() + 1, at - before.rfind('\n').map_or(0, |n| n + 1) + 1)
}

type Edit = (Range<usize>, String, String);

fn byte(m: &[u8], i: Option<usize>) -> u8 {
    i.and_then(|i| m.get(i).copied()).unwrap_or(b' ')
}

/// The last non-blank byte before `i`, and where.
fn before(m: &[u8], i: usize) -> Option<usize> {
    m[..i].iter().rposition(|c| !c.is_ascii_whitespace())
}

fn edit(range: Range<usize>, to: &str, what: String) -> Edit {
    (range, to.to_string(), what)
}

/// `=` ↔ `#`, `<` ↔ `<=`, `>` ↔ `>=`.
fn swaps(text: &str, body: Range<usize>) -> Vec<Edit> {
    let m = text.as_bytes();
    let mut out = Vec::new();
    for i in body.clone() {
        let (prev, next, next2) = (byte(m, i.checked_sub(1)), byte(m, Some(i + 1)), byte(m, Some(i + 2)));
        let one = |to: &str| edit(i..i + 1, to, format!("replace {} with {to}", m[i] as char));
        match m[i] {
            b'=' if !b"=</>!:|~\\".contains(&prev) && !b"=<>|".contains(&next) && !assigned(m, i) => out.push(one("#")),
            b'#' if prev != b'#' && next != b'#' && !assigned(m, i) => out.push(one("=")),
            b'<' | b'>' if next == b'=' && !b"<>=-:|~".contains(&prev) && !b"<>=".contains(&next2) => {
                let op = if m[i] == b'<' { "<" } else { ">" };
                out.push(edit(i..i + 2, op, format!("replace {op}= with {op}")));
            }
            b'<' if !b"<=-|>\\:".contains(&prev) && !b"<=>-:|".contains(&next) => out.push(one("<=")),
            b'>' if !b"<=->:|~".contains(&prev) && !b">=".contains(&next) => out.push(one(">=")),
            _ => {}
        }
    }
    out.retain(|(r, _, _)| r.end <= body.end);
    out
}

/// Whether the `=` or `#` at `i` follows a primed variable or an `EXCEPT` path.
fn assigned(m: &[u8], i: usize) -> bool {
    let Some(p) = before(m, i) else { return false };
    match m[p] {
        b'\'' => true,
        b']' => opening(m, p).is_some_and(|o| o > 0 && m[o - 1] == b'!'),
        c if c.is_ascii_alphanumeric() || c == b'_' => {
            let start = m[..=p].iter().rposition(|c| !(c.is_ascii_alphanumeric() || *c == b'_')).map_or(0, |s| s + 1);
            start >= 2 && &m[start - 2..start] == b"!."
        }
        _ => false,
    }
}

/// The `[` that the `]` at `close` closes.
fn opening(m: &[u8], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for j in (0..=close).rev() {
        match m[j] {
            b']' => depth += 1,
            b'[' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

/// `+ 1` ↔ `- 1` after an operand.
fn steps(text: &str, body: Range<usize>) -> Vec<Edit> {
    let m = text.as_bytes();
    body.clone()
        .filter(|&i| matches!(m[i], b'+' | b'-'))
        .filter(|&i| {
            let one = if byte(m, Some(i + 1)) == b' ' { i + 2 } else { i + 1 };
            let operand = before(m, i).is_some_and(|p| operand_end(m, p));
            let lone = !b"+-|<~".contains(&byte(m, i.checked_sub(1))) && !b"+->|".contains(&byte(m, Some(i + 1)));
            let after = byte(m, Some(one + 1));
            operand
                && lone
                && byte(m, Some(one)) == b'1'
                && one < body.end
                && !(after.is_ascii_alphanumeric() || after == b'_')
        })
        .map(|i| {
            let to = if m[i] == b'+' { "-" } else { "+" };
            edit(i..i + 1, to, format!("replace {} 1 with {to} 1", m[i] as char))
        })
        .collect()
}

/// Whether the byte at `p` ends an operand: a name that is not a keyword or
/// a `\`-operator (`\o`), a number, a closing bracket, or a prime.
fn operand_end(m: &[u8], p: usize) -> bool {
    const KEYWORDS: [&[u8]; 10] =
        [b"IN", b"THEN", b"ELSE", b"IF", b"LET", b"CASE", b"OTHER", b"EXCEPT", b"CHOOSE", b"UNCHANGED"];
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    if !ident(m[p]) {
        return b")]}'".contains(&m[p]);
    }
    let start = m[..=p].iter().rposition(|c| !ident(*c)).map_or(0, |s| s + 1);
    !(start > 0 && m[start - 1] == b'\\') && !KEYWORDS.contains(&&m[start..=p])
}

/// `TRUE` ↔ `FALSE`.
fn truths(text: &str, body: Range<usize>) -> Vec<Edit> {
    words(&text[body.clone()])
        .into_iter()
        .filter_map(|(at, w)| {
            let to = match w {
                "TRUE" => "FALSE",
                "FALSE" => "TRUE",
                _ => return None,
            };
            let at = body.start + at;
            Some(edit(at..at + w.len(), to, format!("replace {w} with {to}")))
        })
        .collect()
}

/// A bullet `/\` that starts its line, or the body (`Name == /\ …`): its
/// offset and column.
fn bullets(m: &str, body: Range<usize>) -> Vec<(usize, usize)> {
    lines(m)
        .filter(|(_, at, line)| at + line.len() > body.start && *at < body.end)
        .filter_map(|(_, at, line)| {
            let from = body.start.saturating_sub(at).min(line.len());
            let lead = line[from..].len() - line[from..].trim_start().len();
            let col = from + lead;
            line[col..].starts_with("/\\").then_some((at + col, col))
        })
        .collect()
}

/// One conjunct dropped, where its list has another at the same column.
fn conjuncts(text: &str, m: &str, body: Range<usize>) -> Vec<Edit> {
    let all = bullets(m, body.clone());
    all.iter()
        .filter(|(at, col)| all.iter().any(|(o, c)| c == col && o != at))
        .map(|&(at, col)| {
            let end = item_end(m, at, col, body.end);
            let line_start = m[..at].rfind('\n').map_or(0, |n| n + 1);
            let range = if line_start >= body.start {
                line_start..end
            } else {
                at..if m[..end].ends_with('\n') { end - 1 } else { end }
            };
            let first = text[at + 2..].lines().next().unwrap_or("").trim();
            edit(range, "", format!("drop conjunct `{}`", crate::cap(first, 60)))
        })
        .collect()
}

/// Where the conjunct at `at` (column `col`) ends: the start of the next
/// line with code at or left of `col`, else `limit`.
fn item_end(m: &str, at: usize, col: usize, limit: usize) -> usize {
    let next_line = m[at..].find('\n').map_or(limit, |n| at + n + 1).min(limit);
    lines(&m[next_line..limit])
        .find(|(_, _, line)| !line.trim().is_empty() && line.len() - line.trim_start().len() <= col)
        .map_or(limit, |(_, off, _)| next_line + off)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edits_of(body: &str) -> Vec<String> {
        let text = format!("---- MODULE M ----\nNext == {body}\n====\n");
        mutants(&text, "M.tla", &["Next".to_string()], &[])
            .into_iter()
            .map(|m| m.apply(&text))
            .map(|t| t.lines().nth(1).unwrap_or("").trim_start_matches("Next == ").to_string())
            .collect()
    }

    #[test]
    fn equality_swaps_skip_assignments_definitions_and_other_operators() {
        assert_eq!(edits_of("x = 1"), vec!["x # 1"]);
        assert_eq!(edits_of("x # 1"), vec!["x = 1"]);
        assert!(edits_of("x' = 1").is_empty(), "a primed left side assigns");
        assert!(edits_of("f' = [f EXCEPT ![p] = 2]").is_empty());
        assert!(edits_of("r' = [r EXCEPT !.a = 2]").is_empty());
        assert!(edits_of("LET y == 2 IN y /= 3 /\\ (y => y) /\\ (y <=> y)").is_empty());
        assert!(edits_of("x \\in S /\\ y \\notin T").is_empty(), "set membership is untouched");
    }

    #[test]
    fn order_swaps_skip_tuples_arrows_and_temporal_operators() {
        assert_eq!(edits_of("x < y"), vec!["x <= y"]);
        assert_eq!(edits_of("x <= y"), vec!["x < y"]);
        assert_eq!(edits_of("x > y"), vec!["x >= y"]);
        assert_eq!(edits_of("x >= y"), vec!["x > y"]);
        assert!(edits_of("<<a, b>> \\o [i \\in S |-> i] \\o (a :> b) \\o (P ~> Q) \\o <>P \\o [][A]_v").is_empty());
    }

    #[test]
    fn steps_flip_only_after_an_operand() {
        assert_eq!(edits_of("x + 1"), vec!["x - 1"]);
        assert_eq!(edits_of("f[x]-1"), vec!["f[x]+1"]);
        assert!(edits_of("x + 10").is_empty());
        assert!(edits_of("y \\o -1").is_empty(), "a unary minus is no step");
    }

    #[test]
    fn truths_flip_and_comments_and_strings_are_never_touched() {
        assert_eq!(edits_of("x = TRUE"), vec!["x # TRUE", "x = FALSE"]);
        assert!(edits_of("UNCHANGED x \\* x = 1 < 2 TRUE").is_empty());
        assert!(edits_of("y' = \"a = b < c\" (* x = 1 *)").is_empty());
    }

    #[test]
    fn a_conjunct_is_dropped_only_from_a_list_of_two_or_more() {
        let text = "---- MODULE M ----\nNext == /\\ a' = 1\n        /\\ b' = 2\n           + 0\n        /\\ c' = 3\n\nOne == /\\ d' = 4\n====\n";
        let dropped: Vec<String> = mutants(text, "M.tla", &["Next".to_string(), "One".to_string()], &[])
            .into_iter()
            .filter(|m| m.name.contains("drop conjunct"))
            .map(|m| m.apply(text))
            .collect();
        assert_eq!(dropped.len(), 3, "{dropped:?}");
        assert!(dropped[0].starts_with("---- MODULE M ----\nNext == \n        /\\ b' = 2\n"), "{}", dropped[0]);
        assert!(
            dropped[1].contains("/\\ a' = 1\n        /\\ c' = 3\n"),
            "the continuation goes with it: {}",
            dropped[1]
        );
        assert!(dropped[2].contains("/\\ b' = 2\n           + 0\nOne"), "{}", dropped[2]);
    }

    #[test]
    fn only_what_the_roots_reach_is_mutated() {
        let text = "---- MODULE M ----\nInit == x = 0\nA == x' = x + 1\nB == x < 3\nNext == A /\\ B\nSpec == Init /\\ [][Next]_x\nInv == x < 5\n====\n";
        let roots = ["Spec".to_string()];
        let names: Vec<String> = mutants(text, "M.tla", &roots, &["Init".to_string(), "Inv".to_string()])
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(names, vec!["M.tla:3:13: replace + 1 with - 1", "M.tla:4:8: replace < with <="]);
    }
}
