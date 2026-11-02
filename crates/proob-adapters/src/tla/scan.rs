//! A TLA+ module read as text, never parsed: comments and strings blanked,
//! top-level definitions by their column-0 `Name ==` lines, binding tokens,
//! and the modules it `EXTENDS` or `INSTANCE`s. SANY is the parser; this
//! reads only what discovery and the mutator need, and a module it misreads
//! makes TLC fail, which is errored, never a pass.
//!
//! shortcut: a definition is recognised only when its name starts a line
//! (the convention of hand-written and PlusCal-translated specs); infix
//! operator definitions are not recognised, so they are never bound or mutated.
use crate::TokenProblem;
use proob_facts::ulid;
use std::ops::Range;

const TOKEN_PREFIX: &str = "proob:";

/// `text` with comments (`\*` to the end of the line, nested `(* … *)`) and,
/// when `strings`, string literals blanked to spaces byte for byte, newlines
/// kept: every offset in the result is the same offset in `text`.
pub fn blank(text: &str, strings: bool) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let (mut i, mut depth) = (0usize, 0usize);
    let at = |i: usize, s: &[u8]| b[i..].starts_with(s);
    while i < b.len() {
        let (from, to) = if depth > 0 || at(i, b"(*") {
            let step = if at(i, b"(*") || at(i, b"*)") { 2 } else { 1 };
            depth = if at(i, b"(*") {
                depth + 1
            } else if at(i, b"*)") {
                depth - 1
            } else {
                depth
            };
            (i, i + step)
        } else if at(i, b"\\*") {
            (i, b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |n| i + n))
        } else if b[i] == b'"' {
            let end = string_end(b, i);
            if strings { (i, end) } else { (end, end) }
        } else {
            (i + 1, i + 1)
        };
        out[from..to].iter_mut().filter(|c| **c != b'\n').for_each(|c| *c = b' ');
        i = to.max(i + 1);
    }
    String::from_utf8(out).unwrap_or_default()
}

/// One past the closing quote of the string opening at `start`, or the end of its line.
fn string_end(b: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
        i += if b[i] == b'\\' { 2 } else { 1 };
    }
    (i + 1).min(b.len())
}

/// The module's code: `blank` with strings, and everything before the
/// `---- MODULE` line and from the closing `====` line on blanked too.
pub fn masked(text: &str) -> String {
    let code = blank(text, true);
    let mut offset = 0;
    let (mut start, mut end) = (0, code.len());
    for line in code.split_inclusive('\n') {
        let t = line.trim_start();
        if start == 0 && t.starts_with("----") && t.contains("MODULE") {
            start = offset + line.len();
        } else if start > 0 && line.starts_with("====") {
            end = offset;
            break;
        }
        offset += line.len();
    }
    let blanked = |s: &str| s.chars().map(|c| if c == '\n' { '\n' } else { ' ' }).collect::<String>();
    format!("{}{}{}", blanked(&code[..start]), &code[start..end], blanked(&code[end..]))
}

/// A top-level definition: `Name == body`, `Name(p, q) == body`, `Name[x \in S] == body`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub params: bool,
    /// 0-based line of the `==`.
    pub line: usize,
    /// Bytes from after `==` to the next line that starts at column 0.
    pub body: Range<usize>,
}

/// Every top-level definition of a masked module, in order.
pub fn definitions(masked: &str) -> Vec<Definition> {
    let starts: Vec<(usize, usize, &str)> = lines(masked).collect();
    let mut defs: Vec<Definition> = Vec::new();
    for (n, offset, line) in &starts {
        if line.starts_with(|c: char| !c.is_whitespace()) {
            if let Some(d) = defs.last_mut().filter(|d| d.body.end == masked.len()) {
                d.body.end = *offset;
            }
            if let Some((name, params, eq)) = header(line) {
                defs.push(Definition { name, params, line: *n, body: offset + eq..masked.len() });
            }
        }
    }
    defs
}

/// (0-based number, byte offset, text without its newline) of each line.
pub fn lines(text: &str) -> impl Iterator<Item = (usize, usize, &str)> {
    text.split_inclusive('\n')
        .scan(0usize, |offset, line| {
            let at = *offset;
            *offset += line.len();
            Some((at, line.trim_end_matches('\n')))
        })
        .enumerate()
        .map(|(n, (at, line))| (n, at, line))
}

/// The name, whether it takes parameters, and the offset just past `==`.
fn header(line: &str) -> Option<(String, bool, usize)> {
    let rest = line.strip_prefix("LOCAL ").map_or(line, str::trim_start);
    let name_len = rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
    let name = &rest[..name_len];
    let after = rest[name_len..].trim_start();
    let params = after.starts_with('(') || after.starts_with('[');
    let eq = if params { after.find("==")? } else { after.starts_with("==").then_some(0)? };
    let eq_at = line.len() - after.len() + eq;
    let ok = !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) && !line[eq_at..].starts_with("===");
    ok.then(|| (name.to_string(), params, eq_at + 2))
}

/// What a `\* proob:<text>` line binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    Definition { id: u128, name: String },
    Problem(TokenProblem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// 1-based.
    pub line: usize,
    pub bound: Bound,
}

/// Each comment-only `\* proob:<ULID>` line, bound to the definition that
/// follows it with only blank and `\*` comment lines between.
pub fn tokens(text: &str) -> Vec<Token> {
    let defs = definitions(&masked(text));
    let all: Vec<&str> = text.lines().collect();
    fn comment(l: &str) -> Option<&str> {
        l.trim_start().strip_prefix("\\*").map(str::trim)
    }
    all.iter()
        .enumerate()
        .filter_map(|(n, l)| comment(l)?.strip_prefix(TOKEN_PREFIX).map(|t| (n, t.trim_end())))
        .map(|(n, text)| {
            let bound = match ulid::decode(text) {
                Err(error) => Bound::Problem(TokenProblem::NotUlid { text: text.to_string(), error }),
                Ok(id) => {
                    let next = (n + 1..all.len()).find(|&j| !all[j].trim().is_empty() && comment(all[j]).is_none());
                    match next.and_then(|j| defs.iter().find(|d| d.line == j)) {
                        Some(d) if !d.params => Bound::Definition { id, name: d.name.clone() },
                        _ => Bound::Problem(TokenProblem::NoTlaDefinition),
                    }
                }
            };
            Token { line: n + 1, bound }
        })
        .collect()
}

/// Identifier runs and single punctuation characters, with their offsets.
pub fn words(s: &str) -> Vec<(usize, &str)> {
    let b = s.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let len = if ident(b[i]) { b[i..].iter().take_while(|c| ident(**c)).count() } else { 1 };
        if !b[i].is_ascii_whitespace() && s.is_char_boundary(i) && s.is_char_boundary(i + len) {
            out.push((i, &s[i..i + len]));
        }
        i += len;
    }
    out
}

/// The modules a masked module names in `EXTENDS` and `INSTANCE`.
pub fn imports(masked: &str) -> Vec<String> {
    let w: Vec<&str> = words(masked).into_iter().map(|(_, t)| t).collect();
    let mut out = Vec::new();
    for (i, t) in w.iter().enumerate() {
        match *t {
            "INSTANCE" => out.extend(w.get(i + 1).map(|s| s.to_string())),
            "EXTENDS" => {
                let mut j = i + 1;
                while let Some(name) = w.get(j) {
                    out.push(name.to_string());
                    if w.get(j + 1) != Some(&",") {
                        break;
                    }
                    j += 2;
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "\\* before the module\n---- MODULE M ----\nEXTENDS Naturals, Local\n(* a (* nested *) comment\n   Fake == 1 *)\nVARIABLE x\n\nInit == x = 0 \\* \"not a string\"\n\n\\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XTA\n\\* the invariant\nInv == x # \"a \\\" b\"\n\nOp(a) ==\n  a + 1\nI == INSTANCE Other WITH y <- x\n====\nAfter == 1\n";

    #[test]
    fn blanking_keeps_every_offset() {
        let m = masked(SPEC);
        assert_eq!(m.len(), SPEC.len());
        assert_eq!(m.lines().count(), SPEC.lines().count());
        assert!(!m.contains("Fake") && !m.contains("nested") && !m.contains("not a string"));
        assert!(!m.contains("After") && !m.contains("MODULE") && !m.contains("before"));
        assert!(m.contains("Inv == x #") && !m.contains("a \\\" b"), "{m}");
        assert!(blank("x = \"(*\" y", false).contains("\"(*\" y"), "a string opens no comment");
    }

    #[test]
    fn definitions_start_at_column_zero_and_run_to_the_next() {
        let m = masked(SPEC);
        let defs = definitions(&m);
        let names: Vec<(&str, bool)> = defs.iter().map(|d| (d.name.as_str(), d.params)).collect();
        assert_eq!(names, vec![("Init", false), ("Inv", false), ("Op", true), ("I", false)]);
        assert_eq!(m[defs[2].body.clone()].trim(), "a + 1");
        assert_eq!(m[defs[0].body.clone()].trim(), "x = 0");
    }

    #[test]
    fn a_token_binds_the_definition_below_it() {
        let t = tokens(SPEC);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].line, 10);
        assert!(matches!(&t[0].bound, Bound::Definition { name, .. } if name == "Inv"), "{t:?}");
        let above_op = tokens("---- MODULE M ----\n\\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XTA\nOp(a) == a\n====\n");
        assert_eq!(above_op[0].bound, Bound::Problem(TokenProblem::NoTlaDefinition));
        let above_nothing = tokens("---- MODULE M ----\n\\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XTA\nVARIABLE x\n====\n");
        assert_eq!(above_nothing[0].bound, Bound::Problem(TokenProblem::NoTlaDefinition));
        let versioned = tokens("\\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XTA:v2\nInv == TRUE\n");
        assert!(matches!(versioned[0].bound, Bound::Problem(TokenProblem::NotUlid { .. })));
    }

    #[test]
    fn imports_are_extends_and_instances() {
        assert_eq!(imports(&masked(SPEC)), vec!["Naturals", "Local", "Other"]);
    }
}
