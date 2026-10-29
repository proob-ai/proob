//! One Python source file, read for what discovery needs: binding tokens
//! with the test function each one sits above.
//!
//! Two passes. `mask` blanks the inside of every string literal (single,
//! triple-quoted, with any prefix) and every comment, keeping a comment's
//! `#` so that a comment-only line masks to `#` while a line inside a
//! docstring masks to nothing; the line pass then classifies masked lines
//! and reads token lines from the original text. A token is a comment-only
//! line `# proob:<text>`; it binds the first `def` that follows with only
//! blank lines, comments, and decorators between (a decorator's open
//! brackets are tracked, so a multi-line `@given(...)` is one decorator).
use crate::TokenProblem;
use proob_facts::ulid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub tokens: Vec<Token>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// 1-based line of the token comment.
    pub line: usize,
    pub bound: Bound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    Test { id: u128, function: String },
    Problem(TokenProblem),
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

/// The text with string contents and comment text replaced by spaces;
/// newlines are kept so lines align, and a comment keeps its `#`.
pub fn mask(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '#' => {
                out.push('#');
                i = blank_until_newline(&chars, i + 1, &mut out);
            }
            q @ ('"' | '\'') => i = blank_string(&chars, i, q, &mut out),
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn blank_until_newline(chars: &[char], from: usize, out: &mut String) -> usize {
    let mut i = from;
    while i < chars.len() && chars[i] != '\n' {
        out.push(' ');
        i += 1;
    }
    i
}

/// A string literal opened at `from` by `q`: triple-quoted when three `q`
/// follow, else ending at the next unescaped `q` or at the end of the line.
fn blank_string(chars: &[char], from: usize, q: char, out: &mut String) -> usize {
    let triple = chars.get(from + 1) == Some(&q) && chars.get(from + 2) == Some(&q);
    let open = if triple { 3 } else { 1 };
    out.push_str(&" ".repeat(open));
    let mut i = from + open;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                out.push(' ');
                out.push(if chars.get(i + 1) == Some(&'\n') { '\n' } else { ' ' });
                i += 2;
            }
            '\n' if !triple => return i,
            c if c == q && (!triple || (chars.get(i + 1) == Some(&q) && chars.get(i + 2) == Some(&q))) => {
                out.push_str(&" ".repeat(open));
                return i + open;
            }
            '\n' => {
                out.push('\n');
                i += 1;
            }
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

#[derive(Debug)]
struct Pending {
    line: usize,
    text: String,
    /// Brackets a decorator has left open; its continuation lines are consumed until zero.
    open: usize,
}

#[derive(Debug, Default)]
struct State {
    pending: Option<Pending>,
    tokens: Vec<Token>,
}

const TOKEN_PREFIX: &str = "proob:";

impl State {
    fn line(&mut self, n: usize, orig: &str, masked: &str) {
        let m = masked.trim();
        if let Some(p) = self.pending.as_mut().filter(|p| p.open > 0) {
            p.open = depth(p.open, m);
            return;
        }
        match m {
            "" => {}
            "#" => self.comment(n, orig.trim()),
            _ if m.starts_with('@') => {
                if let Some(p) = self.pending.as_mut() {
                    p.open = depth(0, m);
                }
            }
            _ => self.statement(orig, m),
        }
    }

    /// A comment-only line: a token when it is `# proob:…`.
    fn comment(&mut self, n: usize, orig: &str) {
        let Some(rest) = orig.strip_prefix('#') else { return };
        let Some(text) = rest.trim_start().strip_prefix(TOKEN_PREFIX) else { return };
        self.flush(None);
        self.pending = Some(Pending { line: n, text: text.trim_end().to_string(), open: 0 });
    }

    fn statement(&mut self, orig: &str, masked: &str) {
        self.flush(Some((orig, masked)));
    }

    /// Resolves the pending token against the statement that follows it.
    fn flush(&mut self, statement: Option<(&str, &str)>) {
        let Some(p) = self.pending.take() else { return };
        let bound = match ulid::decode(&p.text) {
            Err(error) => Bound::Problem(TokenProblem::NotUlid { text: p.text, error }),
            Ok(id) => bind(id, statement),
        };
        self.tokens.push(Token { line: p.line, bound });
    }

    fn finish(mut self) -> Scan {
        self.flush(None);
        Scan { tokens: self.tokens }
    }
}

fn bind(id: u128, statement: Option<(&str, &str)>) -> Bound {
    let Some((orig, masked)) = statement else { return Bound::Problem(TokenProblem::NoDef) };
    match def_name(masked) {
        None => Bound::Problem(TokenProblem::NoDef),
        Some(function) if !function.is_ascii() => Bound::Problem(TokenProblem::NonAsciiName { function }),
        Some(function) if orig.starts_with([' ', '\t']) => Bound::Problem(TokenProblem::NestedDef { function }),
        Some(function) if !function.starts_with("test") => Bound::Problem(TokenProblem::NotATestDef { function }),
        Some(function) => Bound::Test { id, function },
    }
}

/// Net open brackets after `masked`, starting from `open`.
fn depth(open: usize, masked: &str) -> usize {
    masked.chars().fold(open, |d, c| match c {
        '(' | '[' | '{' => d + 1,
        ')' | ']' | '}' => d.saturating_sub(1),
        _ => d,
    })
}

/// The name after `def` (or `async def`) on a masked statement line.
fn def_name(masked: &str) -> Option<String> {
    let rest = masked.strip_prefix("async").map_or(masked, |r| r.trim_start());
    let after = rest.strip_prefix("def")?;
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    let name: String = after.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r##""""Module docstring with a decoy:
# proob:01K8V5NQ4A0F3R7Y2M9D6P1XHA
"""
from hypothesis import given, strategies as st

DECOY = "# proob:01K8V5NQ4A0F3R7Y2M9D6P1XHB"


# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPA
@given(
    st.lists(st.integers()),  # a comment inside the decorator
)
@pytest.mark.slow
def test_holds(xs):
    assert ")" != "("


# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPB
async def test_async(): ...


class TestGroup:
    # proob:01K8V5NQ4A0F3R7Y2M9D6P1XPC
    def test_method(self):
        pass


# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPD
def helper():
    pass


# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPE
x = 1

# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPF:v2
def test_versioned():
    pass

# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPG
def test_café():
    pass

# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPH
"##;

    #[test]
    fn finds_tokens_and_the_defs_they_bind() {
        let s = scan(FILE);
        let bound: Vec<(usize, &Bound)> = s.tokens.iter().map(|t| (t.line, &t.bound)).collect();
        assert_eq!(bound.len(), 8, "{bound:?}");
        assert!(matches!(bound[0], (9, Bound::Test { function, .. }) if function == "test_holds"), "{:?}", bound[0]);
        assert!(matches!(bound[1], (18, Bound::Test { function, .. }) if function == "test_async"), "{:?}", bound[1]);
        assert!(
            matches!(bound[2], (23, Bound::Problem(TokenProblem::NestedDef { function })) if function == "test_method"),
            "{:?}",
            bound[2]
        );
        assert!(
            matches!(bound[3], (28, Bound::Problem(TokenProblem::NotATestDef { function })) if function == "helper"),
            "{:?}",
            bound[3]
        );
        assert!(matches!(bound[4], (33, Bound::Problem(TokenProblem::NoDef))), "{:?}", bound[4]);
        assert!(matches!(bound[5], (36, Bound::Problem(TokenProblem::NotUlid { .. }))), "{:?}", bound[5]);
        assert!(
            matches!(bound[6], (40, Bound::Problem(TokenProblem::NonAsciiName { function })) if function == "test_café"),
            "{:?}",
            bound[6]
        );
        assert!(matches!(bound[7], (44, Bound::Problem(TokenProblem::NoDef))), "{:?}", bound[7]);
    }

    #[test]
    fn masking_keeps_lines_aligned_and_comments_marked() {
        let text = "a = 'x' # c\nb = \"\"\"\n# not a comment\n\"\"\"\nc = r'\\'' # d";
        let m = mask(text);
        assert_eq!(m.lines().count(), text.lines().count());
        // A string prefix (`r`, `b`, `f`) is an identifier character and stays; the literal behind it is blanked.
        assert_eq!(m, "a =     #  \nb =    \n               \n   \nc = r     #  ");
        assert_eq!(scan("# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPA\ndef test_x(): pass\n").tokens.len(), 1);
        assert!(matches!(
            &scan("# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPA\nclass TestX:\n    pass\n").tokens[0].bound,
            Bound::Problem(TokenProblem::NoDef)
        ));
    }
}
