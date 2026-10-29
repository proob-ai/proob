//! One Rust source file, read for what discovery needs: binding tokens with
//! the test fn each one sits above, and `mod` declarations with their
//! enclosing inline modules and `#[cfg(test)]` status.
//!
//! Two passes. `mask` blanks the inside of every string, character literal,
//! and comment so braces and keywords are read only where they are code,
//! keeping a line comment's two slashes so that a comment-only line masks to
//! `//` while a line inside a string literal masks to nothing; the line pass
//! then walks the masked lines with a brace depth and a stack of inline
//! `mod` frames, and reads token lines from the original text. A token is a
//! comment-only line `// proob:<text>`; it binds the first `fn` that follows
//! with only blank lines, comments, and attributes between.
use crate::TokenProblem;
use proob_facts::ulid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub tokens: Vec<Token>,
    pub mods: Vec<ModDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// 1-based line of the token comment.
    pub line: usize,
    pub bound: Bound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    Test { id: u128, function: String, module_path: Vec<String> },
    Problem(TokenProblem),
}

/// `mod name;` with the attributes and the inline modules around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModDecl {
    pub name: String,
    pub path_attr: Option<String>,
    pub cfg_test: bool,
    pub module_path: Vec<String>,
    pub line: usize,
}

pub fn scan(text: &str) -> Scan {
    let masked = mask(text);
    let mut state = State::default();
    for (i, (orig, m)) in text.lines().zip(masked.lines()).enumerate() {
        state.line(i + 1, orig, m);
    }
    state.finish()
}

// ---------------------------------------------------------------------------
// Pass 1: masking
// ---------------------------------------------------------------------------

/// The text with string, char-literal, and comment contents replaced by
/// spaces; newlines are kept so lines align with the original, and a line
/// comment keeps its `//` so the line pass can tell it from a string's
/// interior.
pub fn mask(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let prev_ident = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        if c == '/' && next == Some('/') {
            out.push_str("//");
            i = blank_until(&chars, i + 2, &mut out, |cs, j| cs[j] == '\n', 0);
        } else if c == '/' && next == Some('*') {
            i = blank_block_comment(&chars, i, &mut out);
        } else if c == '"' {
            i = blank_string(&chars, i, &mut out);
        } else if (c == 'r' || c == 'b' || c == 'c') && !prev_ident && raw_or_byte_string_at(&chars, i) {
            i = blank_prefixed_string(&chars, i, &mut out);
        } else if c == '\'' && !prev_ident && char_literal_at(&chars, i) {
            i = blank_char_literal(&chars, i, &mut out);
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Writes spaces (keeping newlines) from `from` until `stop` says so, then
/// copies `keep` more characters blanked too; returns the next index.
fn blank_until(
    chars: &[char],
    from: usize,
    out: &mut String,
    stop: impl Fn(&[char], usize) -> bool,
    keep: usize,
) -> usize {
    let mut i = from;
    while i < chars.len() && !stop(chars, i) {
        out.push(if chars[i] == '\n' { '\n' } else { ' ' });
        i += 1;
    }
    for _ in 0..keep {
        if i < chars.len() {
            out.push(if chars[i] == '\n' { '\n' } else { ' ' });
            i += 1;
        }
    }
    i
}

fn blank_block_comment(chars: &[char], from: usize, out: &mut String) -> usize {
    let mut depth = 0usize;
    let mut i = from;
    while i < chars.len() {
        let pair = (chars[i], chars.get(i + 1).copied());
        match pair {
            ('/', Some('*')) => {
                depth += 1;
                out.push_str("  ");
                i += 2;
            }
            ('*', Some('/')) => {
                depth -= 1;
                out.push_str("  ");
                i += 2;
                if depth == 0 {
                    return i;
                }
            }
            (c, _) => {
                out.push(if c == '\n' { '\n' } else { ' ' });
                i += 1;
            }
        }
    }
    i
}

fn blank_string(chars: &[char], from: usize, out: &mut String) -> usize {
    out.push(' ');
    let mut i = from + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                out.push_str(if chars.get(i + 1) == Some(&'\n') { " \n" } else { "  " });
                i += 2;
            }
            '"' => {
                out.push(' ');
                return i + 1;
            }
            c => {
                out.push(if c == '\n' { '\n' } else { ' ' });
                i += 1;
            }
        }
    }
    i
}

/// `r"`, `r#"`, `b"`, `br"`, `c"`, `cr"` at `i`.
fn raw_or_byte_string_at(chars: &[char], i: usize) -> bool {
    let mut j = i;
    if matches!(chars.get(j), Some('b' | 'c')) {
        j += 1;
    }
    if chars.get(j) == Some(&'r') {
        j += 1;
        while chars.get(j) == Some(&'#') {
            j += 1;
        }
        return chars.get(j) == Some(&'"');
    }
    j == i + 1 && chars.get(j) == Some(&'"')
}

fn blank_prefixed_string(chars: &[char], from: usize, out: &mut String) -> usize {
    let mut i = from;
    if matches!(chars.get(i), Some('b' | 'c')) {
        out.push(' ');
        i += 1;
    }
    if chars.get(i) != Some(&'r') {
        return blank_string(chars, i, out);
    }
    out.push(' ');
    i += 1;
    let mut hashes = 0usize;
    while chars.get(i) == Some(&'#') {
        out.push(' ');
        i += 1;
        hashes += 1;
    }
    out.push(' ');
    i += 1; // the opening quote
    let closes = |cs: &[char], j: usize| cs[j] == '"' && (1..=hashes).all(|k| cs.get(j + k) == Some(&'#'));
    blank_until(chars, i, out, closes, hashes + 1)
}

/// `'x'` or `'\…'` (a char literal), as opposed to a lifetime `'a`.
fn char_literal_at(chars: &[char], i: usize) -> bool {
    match chars.get(i + 1) {
        Some('\\') => true,
        Some(_) => chars.get(i + 2) == Some(&'\''),
        None => false,
    }
}

fn blank_char_literal(chars: &[char], from: usize, out: &mut String) -> usize {
    out.push(' ');
    let mut i = from + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                out.push_str("  ");
                i += 2;
            }
            '\'' => {
                out.push(' ');
                return i + 1;
            }
            '\n' => return i,
            _ => {
                out.push(' ');
                i += 1;
            }
        }
    }
    i
}

// ---------------------------------------------------------------------------
// Pass 2: lines
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Frame {
    depth_at_open: usize,
    name: String,
    cfg_test: bool,
}

#[derive(Debug, Default)]
struct Pending {
    token: Option<(usize, String)>,
    has_test_attr: bool,
    cfg_test: bool,
    path_attr: Option<String>,
}

#[derive(Debug, Default)]
struct State {
    depth: usize,
    stack: Vec<Frame>,
    pending: Pending,
    tokens: Vec<Token>,
    mods: Vec<ModDecl>,
}

const TOKEN_PREFIX: &str = "proob:";

impl State {
    fn module_path(&self) -> Vec<String> {
        self.stack.iter().map(|f| f.name.clone()).collect()
    }

    fn in_cfg_test(&self) -> bool {
        self.stack.iter().any(|f| f.cfg_test)
    }

    fn line(&mut self, n: usize, orig: &str, masked: &str) {
        let m = masked.trim();
        if m.is_empty() {
            return; // blank, or inside a string literal or block comment
        }
        if m == "//" {
            self.comment_or_blank(n, orig.trim());
            return;
        }
        if m.starts_with("#![") {
            return;
        }
        if m.starts_with("#[") {
            self.attribute(m, orig);
            return;
        }
        self.item(n, m);
    }

    /// A comment-only or blank line: a token when it is `// proob:…`.
    fn comment_or_blank(&mut self, n: usize, orig: &str) {
        let Some(rest) = orig.strip_prefix("//") else { return };
        if rest.starts_with('/') || rest.starts_with('!') {
            return;
        }
        let Some(text) = rest.trim_start().strip_prefix(TOKEN_PREFIX) else { return };
        self.flush_token(None);
        self.pending.token = Some((n, text.trim_end().to_string()));
    }

    fn attribute(&mut self, masked: &str, orig: &str) {
        let masked = masked.split("//").next().unwrap_or("").trim();
        let inner = masked.trim_start_matches("#[").trim_end_matches(']').trim();
        let name = inner.split(|c: char| c == '(' || c == '=' || c.is_whitespace()).next().unwrap_or("").trim();
        let last = name.rsplit("::").next().unwrap_or(name);
        if name == "cfg" && cfg_names_test(inner) {
            self.pending.cfg_test = true;
        }
        if matches!(last, "test" | "proptest") {
            self.pending.has_test_attr = true;
        }
        if name == "path" {
            self.pending.path_attr = quoted(orig);
        }
    }

    fn item(&mut self, n: usize, masked: &str) {
        let decl = mod_decl(masked);
        if let Some((name, ModShape::File)) = &decl {
            self.mods.push(ModDecl {
                name: name.clone(),
                path_attr: self.pending.path_attr.clone(),
                cfg_test: self.pending.cfg_test || self.in_cfg_test(),
                module_path: self.module_path(),
                line: n,
            });
        }
        let function = fn_name(masked);
        self.flush_token(function.as_deref());
        let inline = match decl {
            Some((name, ModShape::Inline)) => {
                Some(Frame { depth_at_open: 0, name, cfg_test: self.pending.cfg_test || self.in_cfg_test() })
            }
            _ => None,
        };
        self.pending = Pending::default();
        self.braces(masked, inline);
    }

    /// Resolves the pending token against the item that follows it.
    fn flush_token(&mut self, function: Option<&str>) {
        let Some((line, text)) = self.pending.token.take() else { return };
        let bound = match (ulid::decode(&text), function, self.pending.has_test_attr) {
            (Err(error), _, _) => Bound::Problem(TokenProblem::NotUlid { text, error }),
            (Ok(_), None, _) => Bound::Problem(TokenProblem::NoFunction),
            (Ok(_), Some(f), false) => Bound::Problem(TokenProblem::NotATest { function: f.to_string() }),
            (Ok(id), Some(f), true) => Bound::Test { id, function: f.to_string(), module_path: self.module_path() },
        };
        self.tokens.push(Token { line, bound });
    }

    fn braces(&mut self, masked: &str, mut inline: Option<Frame>) {
        for c in masked.chars() {
            match c {
                '{' => {
                    self.depth += 1;
                    if let Some(frame) = inline.take() {
                        self.stack.push(Frame { depth_at_open: self.depth, ..frame });
                    }
                }
                '}' => {
                    if self.stack.last().is_some_and(|f| f.depth_at_open == self.depth) {
                        self.stack.pop();
                    }
                    self.depth = self.depth.saturating_sub(1);
                }
                _ => {}
            }
        }
    }

    fn finish(mut self) -> Scan {
        self.flush_token(None);
        Scan { tokens: self.tokens, mods: self.mods }
    }
}

/// `test` as a whole word inside `cfg(...)`: `cfg(test)`, `cfg(all(test, unix))`.
fn cfg_names_test(inner: &str) -> bool {
    inner.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| w == "test")
}

fn quoted(orig: &str) -> Option<String> {
    let start = orig.find('"')? + 1;
    let len = orig[start..].find('"')?;
    Some(orig[start..start + len].to_string())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModShape {
    File,
    Inline,
}

/// `[pub[(…)]] mod name ;|{`.
pub(crate) fn mod_decl(masked: &str) -> Option<(String, ModShape)> {
    let mut rest = masked;
    if let Some(after) = rest.strip_prefix("pub") {
        rest = after.trim_start();
        if let Some(after) = rest.strip_prefix('(') {
            rest = after.split_once(')')?.1.trim_start();
        }
    }
    let after = rest.strip_prefix("mod")?;
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let after = after.trim_start();
    let name: String = after.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() {
        return None;
    }
    match after[name.len()..].trim_start().chars().next() {
        Some(';') => Some((name, ModShape::File)),
        Some('{') => Some((name, ModShape::Inline)),
        _ => None,
    }
}

/// The name after the `fn` keyword on an item line, if any.
fn fn_name(masked: &str) -> Option<String> {
    let bytes = masked.as_bytes();
    let mut from = 0;
    while let Some(i) = masked[from..].find("fn") {
        let at = from + i;
        let before_ok = at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
        let after = &masked[at + 2..];
        if before_ok && after.starts_with(char::is_whitespace) {
            let name: String = after.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if !name.is_empty() && !name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some(name);
            }
        }
        from = at + 2;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r##"
mod common;
#[cfg(test)]
mod helpers;
#[path = "x/y.rs"]
pub(crate) mod renamed;
use proptest::prelude::*;

proptest! {
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHA
    #[test]
    fn holds(v in common::small_vec()) { prop_assert!(v.len() <= 8, "{}", "// proob:not-a-token}"); }
}

#[cfg(test)]
mod inner {
    mod deeper;
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHB
    #[tokio::test]
    async fn nested() { let _c = '{'; let _s = r#"}"#; }
}

// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHC
fn not_a_test() {}

// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHD:v2
#[test]
fn versioned() {}

// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHE
"##;

    #[test]
    fn finds_tokens_tests_and_module_paths() {
        let s = scan(FILE);
        let bound: Vec<&Bound> = s.tokens.iter().map(|t| &t.bound).collect();
        assert!(
            matches!(bound[0], Bound::Test { function, module_path, .. } if function == "holds" && module_path.is_empty())
        );
        assert!(
            matches!(bound[1], Bound::Test { function, module_path, .. } if function == "nested" && module_path == &["inner"])
        );
        assert!(matches!(bound[2], Bound::Problem(TokenProblem::NotATest { function }) if function == "not_a_test"));
        assert!(matches!(bound[3], Bound::Problem(TokenProblem::NotUlid { .. })));
        assert!(matches!(bound[4], Bound::Problem(TokenProblem::NoFunction)));
        assert_eq!(s.tokens.len(), 5);
    }

    #[test]
    fn finds_mod_declarations_with_cfg_and_path() {
        let s = scan(FILE);
        let names: Vec<(&str, bool, Option<&str>, Vec<String>)> = s
            .mods
            .iter()
            .map(|m| (m.name.as_str(), m.cfg_test, m.path_attr.as_deref(), m.module_path.clone()))
            .collect();
        assert_eq!(names[0], ("common", false, None, vec![]));
        assert_eq!(names[1], ("helpers", true, None, vec![]));
        assert_eq!(names[2], ("renamed", false, Some("x/y.rs"), vec![]));
        assert_eq!(names[3], ("deeper", true, None, vec!["inner".to_string()]));
        assert_eq!(s.mods.len(), 4);
    }

    #[test]
    fn masking_keeps_lines_aligned() {
        let text = "a \"x\ny\" b // c\n'{' /* d\ne */ f";
        let m = mask(text);
        assert_eq!(m.lines().count(), text.lines().count());
        assert_eq!(m, "a   \n   b //  \n        \n     f");
        // A comment-looking line inside a string is not a comment.
        let s = mask("let t = \"\n// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHA\n\";");
        assert_eq!(s.lines().nth(1).map(str::trim), Some(""));
        assert_eq!(
            scan("let t = \"\n// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHA\n\";\n#[test] // note\nfn f() {}\n").tokens,
            vec![]
        );
    }
}
