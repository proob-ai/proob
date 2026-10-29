//! One source file of a verified crate, read for what the Verus adapter
//! needs: binding tokens with the `verus!` item each one sits above, every
//! `fn` item inside a `verus!` block with its mode, and the inline modules
//! around them. File-level `mod` declarations come from the Rust scanner,
//! which shares the masking pass (`rust::scan::mask`).
//!
//! A token is a comment-only line `// proob:<text>` at item position: at the
//! brace depth where the enclosing file, `verus!` block, inline `mod`, or
//! `impl`/`trait` block holds its items. It binds the first item that follows
//! with only blank lines, comments, and attributes between. The item must be
//! a `fn` (exec by default, or `proof fn`, or `spec fn`) and must sit inside
//! a `verus! { … }` block: outside one, Verus does not verify it, so there is
//! nothing a proof could be evidence of. A `// proob:` comment inside a
//! function body is not above an item and is left alone (proob's own
//! `// verify-negative-anchor` in `proob-core` is one, and so would a `proob:` one be).
use crate::TokenProblem;
use crate::rust::scan::{ModShape, mask, mod_decl};
use proob_facts::ulid;

/// What Verus makes of a `fn`: executable code (proof obligations from its
/// `ensures`), a proof (a lemma), or a specification (a definition).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Exec,
    Proof,
    Spec,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Exec => "exec fn",
            Mode::Proof => "proof fn",
            Mode::Spec => "spec fn",
        }
    }
}

/// A `fn` inside a `verus!` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub mode: Mode,
    /// Inline modules enclosing the item, outermost first.
    pub module_path: Vec<String>,
    /// 1-based line of the item's header (the line holding `fn`).
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    Item { id: u128, item: Item },
    Problem(TokenProblem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// 1-based line of the token comment.
    pub line: usize,
    pub bound: Bound,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scan {
    pub tokens: Vec<Token>,
    /// Every `fn` inside a `verus!` block, bound or not, in source order.
    pub items: Vec<Item>,
}

pub fn scan(text: &str) -> Scan {
    let masked = mask(text);
    let mut state = State::default();
    for (i, (orig, m)) in text.lines().zip(masked.lines()).enumerate() {
        state.line(i + 1, orig, m);
    }
    state.finish()
}

const TOKEN_PREFIX: &str = "proob:";

/// A block whose braces hold items.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Block {
    Verus,
    Mod(String),
    /// `impl` or `trait`: methods are items too.
    Items,
}

#[derive(Debug)]
struct Frame {
    depth: usize,
    block: Block,
}

#[derive(Debug, Default)]
struct State {
    depth: usize,
    /// The open item blocks, outermost first.
    frames: Vec<Frame>,
    /// A block header was seen on the current line; its `{` opens it.
    pending: Option<Block>,
    token: Option<(usize, String)>,
    tokens: Vec<Token>,
    items: Vec<Item>,
}

struct Header {
    name: String,
    mode: Mode,
}

impl State {
    fn module_path(&self) -> Vec<String> {
        self.frames
            .iter()
            .filter_map(|f| match &f.block {
                Block::Mod(name) => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn in_verus(&self) -> bool {
        self.frames.iter().any(|f| f.block == Block::Verus)
    }

    /// The depth at which the innermost open block holds its items.
    fn item_depth(&self) -> usize {
        self.frames.last().map_or(0, |f| f.depth)
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
        if m.starts_with("#![") || m.starts_with("#[") {
            // An attribute is not an item, but verusfmt wraps a long
            // `assert forall … by {` so that a continuation line begins with
            // `#[trigger] …) … by {`: its braces still count, or the enclosing
            // `verus!` frame closes early and every later item reads as
            // outside it.
            self.braces(m);
            return;
        }
        self.item(n, m);
    }

    /// A comment-only or blank line: a token when it is `// proob:…` at
    /// item position.
    fn comment_or_blank(&mut self, n: usize, orig: &str) {
        let Some(rest) = orig.strip_prefix("//") else { return };
        if rest.starts_with('/') || rest.starts_with('!') || self.depth != self.item_depth() {
            return;
        }
        let Some(text) = rest.trim_start().strip_prefix(TOKEN_PREFIX) else { return };
        self.flush(n, None);
        self.token = Some((n, text.trim_end().to_string()));
    }

    fn item(&mut self, n: usize, masked: &str) {
        let header = fn_header(masked);
        self.flush(n, header.as_ref());
        if let Some(h) = header
            && self.in_verus()
        {
            self.items.push(Item { name: h.name, mode: h.mode, module_path: self.module_path(), line: n });
        }
        self.pending = block_header(masked);
        self.braces(masked);
    }

    /// Resolves the pending token against the item line that follows it.
    fn flush(&mut self, n: usize, header: Option<&Header>) {
        let Some((line, text)) = self.token.take() else { return };
        let bound = match (ulid::decode(&text), header, self.in_verus()) {
            (Err(error), _, _) => Bound::Problem(TokenProblem::NotUlid { text, error }),
            (Ok(_), None, _) => Bound::Problem(TokenProblem::NoVerusItem),
            (Ok(_), Some(h), false) => Bound::Problem(TokenProblem::OutsideVerus { function: h.name.clone() }),
            (Ok(id), Some(h), true) => Bound::Item {
                id,
                item: Item { name: h.name.clone(), mode: h.mode, module_path: self.module_path(), line: n },
            },
        };
        self.tokens.push(Token { line, bound });
    }

    fn braces(&mut self, masked: &str) {
        for c in masked.chars() {
            match c {
                '{' => {
                    self.depth += 1;
                    if let Some(block) = self.pending.take() {
                        self.frames.push(Frame { depth: self.depth, block });
                    }
                }
                '}' => {
                    if self.frames.last().is_some_and(|f| f.depth == self.depth) {
                        self.frames.pop();
                    }
                    self.depth = self.depth.saturating_sub(1);
                }
                _ => {}
            }
        }
    }

    fn finish(mut self) -> Scan {
        self.flush(0, None);
        Scan { tokens: self.tokens, items: self.items }
    }
}

/// The block an item line opens, when it opens one that holds items:
/// `verus! {`, an inline `mod name {`, or an `impl`/`trait` block (whose
/// `{` may be on a later line).
fn block_header(masked: &str) -> Option<Block> {
    if masked.starts_with("verus!") {
        return Some(Block::Verus);
    }
    if let Some((name, ModShape::Inline)) = mod_decl(masked) {
        return Some(Block::Mod(name));
    }
    let mut words = masked.split_whitespace().skip_while(|w| w.starts_with("pub") || *w == "unsafe" || *w == "default");
    matches!(words.next(), Some(w) if w == "impl" || w.starts_with("impl<") || w == "trait").then_some(Block::Items)
}

/// `[pub[(…)]] [open|closed|uninterp|broadcast|const|unsafe] [spec|proof|exec] fn name`.
fn fn_header(masked: &str) -> Option<Header> {
    let at = fn_keyword(masked)?;
    let before: Vec<&str> = masked[..at].split_whitespace().collect();
    if before.iter().any(|w| matches!(*w, "impl" | "trait" | "type" | "let" | "struct" | "enum" | "use" | "=")) {
        return None;
    }
    let name: String = masked[at + 2..].trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let mode = if before.contains(&"spec") {
        Mode::Spec
    } else if before.contains(&"proof") || before.contains(&"axiom") {
        Mode::Proof
    } else {
        Mode::Exec
    };
    Some(Header { name, mode })
}

/// The byte offset of the first `fn` keyword on an item line.
pub(crate) fn fn_keyword(masked: &str) -> Option<usize> {
    let bytes = masked.as_bytes();
    masked.match_indices("fn").map(|(at, _)| at).find(|&at| {
        let before_ok = at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
        before_ok && masked[at + 2..].starts_with(char::is_whitespace)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r##"
use vstd::prelude::*;

// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHA
pub fn outside() -> u32 { 1 }

verus! {

/// `x` brought into `[lo, hi]`; the string "verus! {" and the char '}' are not code.
// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHB
pub fn clamp(x: i32, lo: i32, hi: i32) -> (r: i32)
    requires lo <= hi,
    ensures lo <= r <= hi,
{
    // proob:verify-negative-anchor
    let _s = "verus! {"; let _c = '}';
    if x < lo { lo } else if x > hi { hi } else { x }
}

impl Wrapper {
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHG
    pub fn get(&self) -> (r: u32) ensures r == self.0 { self.0 }
}

pub open spec fn small(n: nat) -> bool { n < 8 }

mod inner {
    // proob:01K8V5NQ4A0F3R7Y2M9D6P1XHC
    #[verifier::opaque]
    pub proof fn lemma_small() ensures small(1) {}
}

// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHD
pub struct NotAFunction { pub x: u32 }

// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHE:v2
pub fn versioned() {}

} // verus!

// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHF
"##;

    #[test]
    fn items_and_tokens_inside_and_outside_verus_blocks() {
        let s = scan(FILE);
        let names: Vec<(&str, Mode, Vec<String>)> =
            s.items.iter().map(|i| (i.name.as_str(), i.mode, i.module_path.clone())).collect();
        assert_eq!(
            names,
            vec![
                ("clamp", Mode::Exec, vec![]),
                ("get", Mode::Exec, vec![]),
                ("small", Mode::Spec, vec![]),
                ("lemma_small", Mode::Proof, vec!["inner".to_string()]),
                ("versioned", Mode::Exec, vec![]),
            ]
        );
        let bound: Vec<&Bound> = s.tokens.iter().map(|t| &t.bound).collect();
        assert!(matches!(bound[0], Bound::Problem(TokenProblem::OutsideVerus { function }) if function == "outside"));
        assert!(matches!(bound[1], Bound::Item { item, .. } if item.name == "clamp" && item.line == 11));
        assert!(
            matches!(bound[2], Bound::Item { item, .. } if item.name == "get" && item.module_path.is_empty()),
            "a method is an item"
        );
        assert!(
            matches!(bound[3], Bound::Item { item, .. } if item.name == "lemma_small" && item.module_path == ["inner"])
        );
        assert!(matches!(bound[4], Bound::Problem(TokenProblem::NoVerusItem)));
        assert!(matches!(bound[5], Bound::Problem(TokenProblem::NotUlid { .. })));
        assert!(matches!(bound[6], Bound::Problem(TokenProblem::NoVerusItem)));
        assert_eq!(s.tokens.len(), 7, "the comment inside clamp's body is not a token");
        assert_eq!(s.tokens[1].line, 10);
    }

    /// verusfmt's layout of a long `assert forall … by {`: the continuation
    /// line opens with `#[trigger]` and ends with the block's `{`. The brace
    /// must count, or the item after the lemma reads as outside `verus!`.
    #[test]
    fn a_trigger_continuation_line_keeps_the_verus_frame_open() {
        let text = r##"
verus! {

proof fn lemma(s: Seq<u8>)
    ensures forall|i: int| 0 <= i < s.len() ==> s[i] == s[i],
{
    assert forall|i: int, j: int|
        0 <= i < s.len() && 0 <= j < s.len() && i != j implies (#[trigger] s[i]) == (
    #[trigger] s[j]) by {
    }
}

// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHA
pub fn after(x: u8) -> (r: u8) ensures r == x { x }

} // verus!
"##;
        let s = scan(text);
        assert_eq!(s.tokens.len(), 1);
        assert!(
            matches!(&s.tokens[0].bound, Bound::Item { item, .. } if item.name == "after"),
            "{:?}",
            s.tokens[0].bound
        );
    }

    #[test]
    fn headers_read_modes_and_skip_non_items() {
        assert!(matches!(fn_header("pub(crate) open spec fn f(x: int) -> int"), Some(Header { mode: Mode::Spec, .. })));
        assert!(matches!(fn_header("broadcast proof fn lemma_x()"), Some(Header { mode: Mode::Proof, .. })));
        assert!(
            matches!(fn_header("pub const fn k() -> u8 { 1 }"), Some(Header { name, mode: Mode::Exec }) if name == "k")
        );
        assert!(fn_header("let f: fn(u32) -> u32 = id;").is_none());
        assert!(fn_header("impl Trait for X { fn m() {} }").is_none());
        assert!(fn_header("type F = fn();").is_none());
    }
}
