//! Mutation-under-proof's mutants: the operator set `mutants-under-proof-v2`
//! applied to the executable body of one `verus!` function, with its ghost
//! code left alone, so that every mutant is a change to the implementation
//! and never to the specification or to the proof of it. A mutant the
//! verifier rejects is killed; one it accepts shows a specification that
//! cannot tell the two implementations apart.
//!
//! cargo-mutants cannot do this: it reads items with `syn`, and everything
//! inside `verus! { … }` is one macro invocation to `syn`, so it lists no
//! mutants at all for a verified crate (checked against cargo-mutants 27.1.0
//! with `--list --json --Zmutate-file` on `proob-core`). The set here is
//! small, textual, and versioned; a change to it changes what every Verus
//! score means, so the version is recorded in every result's tool versions.
//!
//! # The body
//!
//! The body is the block after the complete signature: the parameter list
//! (balanced parentheses), the return annotation, a `where` clause, and every
//! specification clause (`requires`, `ensures`, `returns`, `recommends`,
//! `decreases`, `opens_invariants`, `no_unwind`, `default_ensures`). A clause
//! is a comma-separated list of spec expressions, and those hold braces of
//! their own: struct patterns after `matches` (`s matches Status::Scoped {
//! kind, .. } ==> …`), blocks after `if`, `else`, `match`, or a closure's
//! `|…|`, and anything inside parentheses or brackets (`Some(BindingFacts {
//! … })`, `matches!(r, Range { .. })`). The body's `{` is the first brace at
//! bracket depth zero that none of these claims: it follows a clause item's
//! `,`, the return type, or an expression that is not a path inside a
//! pattern. verusfmt puts that brace on its own line when there are clauses
//! and at the end of the signature line otherwise; the locator does not rely
//! on that, so hand-written `ensures r == self.0 { self.0 }` reads the same.
//!
//! # Operators
//!
//! Each applies to one occurrence at a time. Binary operators are matched
//! only with a space on both sides, which is how rustfmt (and verusfmt) write
//! them and how generics, dereferences, and negative literals are not.
//!
//! The operator swaps of v1, unchanged:
//!
//! - relational: `<` → `>`, `<=`; `<=` → `>=`, `<`; `>` → `<`, `>=`; `>=` → `<=`, `>`; `==` ↔ `!=`
//! - arithmetic: `+` ↔ `-`; `*` → `+`; `/` → `*`; `%` → `/`
//! - logical: `&&` ↔ `||`; a prefix `!` removed
//! - literal: `true` ↔ `false`
//!
//! The structural operators of v2, which reach the dispatch bodies the swaps
//! cannot (an exhaustive `match` whose arms return constructors has no
//! operator in it). Each keeps the mutant's type, so it compiles:
//!
//! - arm swap: the result expressions of two adjacent `match` arms change
//!   places: `A => x, B => y` becomes `A => y, B => x`; for a block arm
//!   `B => { …; y }` the result is the block's tail expression. When a result
//!   uses a name only its own pattern (or a `let` in its block) binds, the
//!   swap would not compile, and the mutant is the one-way copy that does
//!   (`A => x, B => x`, "arm 2 takes arm 1's result"); two arms with the
//!   same result give no mutant. One mutant per adjacent pair.
//! - branch swap: the tails of the two blocks of `if c { a } else { b }`
//!   change places (`if c { b } else { a }`), one-way under the same rule
//!   when an `if let` pattern or a `let` binds a name the other branch
//!   lacks; an `else if` chain is mutated at its innermost `if … else { }`.
//! - return drop: an early `return x;` that ends the then-block of an `if`
//!   without `else` is removed (`if c { return x; }` becomes `if c { }`),
//!   which typechecks because such a block already has unit type; a `return`
//!   that is the body's own last statement, or sits in a `match` arm, a
//!   loop body, or an `else`, is kept.
//! - result negation: when the return type is `bool`, the body's tail
//!   expression `e` becomes `!(e)`; a bare `true`/`false` tail is left to the
//!   literal flip.
//!
//! # Ghost code
//!
//! Masked before the operators run, inside the body: `proof { … }` blocks;
//! `assert` and `assume` statements, including `assert … by { … }`; a loop's
//! `invariant`, `invariant_except_break`, `ensures`, and `decreases` clauses
//! up to the loop's `{` (read past any brace a clause itself holds, as for
//! a signature); `let ghost` and `let tracked` statements; and
//! `Ghost(…)` and `Tracked(…)` arguments. The function's own `requires` and
//! `ensures` precede the body and are never inside it. A structural operator
//! never moves, removes, or wraps a region that holds ghost code: a match arm
//! with a `proof` block is mutated only in its tail outside the block, and a
//! result that passes a `Ghost(…)` argument is left alone.
//!
//! # Order
//!
//! Mutants are enumerated by operator kind in the order above, then by
//! source position, so a run under `max_mutants` verifies a deterministic
//! prefix and reports the rest as unsampled.
use crate::rust::scan::mask;

/// The operator set every score records in `tool_versions`.
pub const OPERATOR_SET: &str = "mutants-under-proof-v2";

/// The operator kinds, in enumeration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Operator {
    Relational,
    Arithmetic,
    Logical,
    Negation,
    Literal,
    ArmSwap,
    BranchSwap,
    ReturnDrop,
    ResultNegation,
}

/// One replacement: `len` chars at char index `index` become `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub index: usize,
    pub len: usize,
    pub to: String,
}

/// One change to the body: its edits, applied together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutant {
    pub operator: Operator,
    /// Disjoint, in ascending order of `index`.
    pub edits: Vec<Edit>,
    /// What changed, as `describe` says it.
    pub change: String,
    /// 1-based position of the first edit in the file.
    pub line: usize,
    pub column: usize,
}

impl Mutant {
    pub fn describe(&self) -> String {
        format!("{}:{}: {}", self.line, self.column, self.change)
    }
}

/// A block's braces, as char indices: `open` is the `{`, `close` its `}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Body {
    pub open: usize,
    pub close: usize,
}

/// What the locator reads off a signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature {
    pub body: Body,
    /// The return type is `bool`, or `(name: bool)` as Verus names a result.
    pub returns_bool: bool,
}

/// `[start, end)` char indices.
type Range = (usize, usize);

/// A mutation before its position is known.
struct Found {
    operator: Operator,
    edits: Vec<Edit>,
    change: String,
}

/// The text in its three readings: as written, with comments and strings
/// blanked (`masked`), and with the body's ghost code blanked too (`ghost`).
struct Source<'a> {
    chars: &'a [char],
    masked: &'a [char],
    ghost: &'a [char],
}

impl Source<'_> {
    fn text(&self, r: Range) -> String {
        self.chars[r.0..r.1].iter().collect()
    }

    /// The region holds no ghost code: it reads the same before and after
    /// the ghost mask.
    fn exec_only(&self, r: Range) -> bool {
        self.masked[r.0..r.1] == self.ghost[r.0..r.1]
    }
}

const BINARY: &[(&str, &str, Operator)] = &[
    ("<", ">", Operator::Relational),
    ("<", "<=", Operator::Relational),
    ("<=", ">=", Operator::Relational),
    ("<=", "<", Operator::Relational),
    (">", "<", Operator::Relational),
    (">", ">=", Operator::Relational),
    (">=", "<=", Operator::Relational),
    (">=", ">", Operator::Relational),
    ("==", "!=", Operator::Relational),
    ("!=", "==", Operator::Relational),
    ("+", "-", Operator::Arithmetic),
    ("-", "+", Operator::Arithmetic),
    ("*", "+", Operator::Arithmetic),
    ("/", "*", Operator::Arithmetic),
    ("%", "/", Operator::Arithmetic),
    ("&&", "||", Operator::Logical),
    ("||", "&&", Operator::Logical),
];

const OPERATOR_CHARS: &str = "<>=!+-*/%&|";
const LOOP_CLAUSES: [&str; 4] = ["invariant", "invariant_except_break", "ensures", "decreases"];
const SPEC_CLAUSES: [&str; 8] =
    ["requires", "ensures", "returns", "recommends", "decreases", "opens_invariants", "no_unwind", "default_ensures"];
/// Words that are never a local name a pattern binds or an expression uses.
const KEYWORDS: [&str; 16] = [
    "ref", "mut", "box", "true", "false", "self", "crate", "super", "as", "if", "else", "match", "return", "let", "in",
    "move",
];

/// Every mutant of the function whose header is on 1-based `header_line`,
/// by operator kind then source position; empty when the line holds no item
/// with a body.
pub fn mutants(text: &str, header_line: usize) -> Vec<Mutant> {
    let chars: Vec<char> = text.chars().collect();
    let masked: Vec<char> = mask(text).chars().collect();
    let Some(header) = line_start(&chars, header_line) else { return Vec::new() };
    let Some(sig) = signature(&masked, header) else { return Vec::new() };
    let ghost = ghost_mask(&masked, sig.body);
    let src = Source { chars: &chars, masked: &masked, ghost: &ghost };
    let mut found = all_mutations(&src, sig);
    found.sort_by_key(|f| (f.operator, first_edit(f).0, first_edit(f).1, f.change.clone()));
    found.into_iter().map(|f| place(&chars, f)).collect()
}

fn all_mutations(src: &Source, sig: Signature) -> Vec<Found> {
    let body = sig.body;
    let negated = if sig.returns_bool { result_negation(src, body) } else { Vec::new() };
    [
        binary_mutants(src.ghost, body),
        negation_mutants(src.ghost, body),
        literal_mutants(src.ghost, body),
        arm_swaps(src, body),
        branch_swaps(src, body),
        return_drops(src, body),
        negated,
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The first edit's index and replacement length: the sort key after the kind.
fn first_edit(f: &Found) -> (usize, usize) {
    f.edits.first().map_or((0, 0), |e| (e.index, e.to.chars().count()))
}

fn place(chars: &[char], f: Found) -> Mutant {
    let (line, column) = position(chars, first_edit(&f).0);
    Mutant { operator: f.operator, edits: f.edits, change: f.change, line, column }
}

/// The text with one mutant applied.
pub fn apply(text: &str, m: &Mutant) -> String {
    let chars: Vec<char> = text.chars().collect();
    let (mut out, at) = m.edits.iter().fold((String::new(), 0), |(mut out, at), e| {
        out.extend(chars.get(at..e.index).unwrap_or_default());
        out.push_str(&e.to);
        (out, e.index + e.len)
    });
    out.extend(chars.get(at..).unwrap_or_default());
    out
}

/// The char index at which 1-based `line` starts.
pub fn line_start(chars: &[char], line: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }
    if line == 1 {
        return Some(0);
    }
    chars
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == '\n')
        .nth(line - 2)
        .map(|(i, _)| i + 1)
        .filter(|i| *i <= chars.len())
}

fn position(chars: &[char], index: usize) -> (usize, usize) {
    let index = index.min(chars.len());
    let line = chars[..index].iter().filter(|c| **c == '\n').count() + 1;
    let column = index - chars[..index].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1) + 1;
    (line, column)
}

// ---------------------------------------------------------------------------
// The signature and the body
// ---------------------------------------------------------------------------

/// The body of the `fn` whose header starts at `header`, and whether it
/// returns `bool`; `None` when the item has no body.
pub fn signature(masked: &[char], header: usize) -> Option<Signature> {
    let name_end = fn_name_end(masked, header)?;
    let params_open = params_open(masked, name_end)?;
    let params_close = matching(masked, params_open, '(', ')')?;
    let returns_bool = return_type(masked, params_close + 1).is_some_and(|ty| is_bool(&ty));
    let open = body_open(masked, params_close + 1)?;
    let close = matching(masked, open, '{', '}')?;
    Some(Signature { body: Body { open, close }, returns_bool })
}

/// One past the name of the first `fn` at or after `from`.
fn fn_name_end(masked: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while i < masked.len() {
        match word_at(masked, i) {
            Some(w) if word_is(w, "fn") => {
                let name_at = next_code(masked, i + 2)?;
                return word_at(masked, name_at).map(|name| name_at + name.len());
            }
            Some(w) => i += w.len(),
            None => i += 1,
        }
    }
    None
}

/// The `(` of the parameter list: the first one past the generics.
fn params_open(masked: &[char], from: usize) -> Option<usize> {
    let i = next_code(masked, from)?;
    match masked[i] {
        '(' => Some(i),
        '<' => next_code(masked, matching_angle(masked, i)? + 1).filter(|&j| masked[j] == '('),
        _ => None,
    }
}

/// The `>` closing the generics at `open`; the `>` of a bound's `->` is not it.
fn matching_angle(masked: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < masked.len() {
        match masked[i] {
            '<' => depth += 1,
            '-' if masked.get(i + 1) == Some(&'>') => i += 1,
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The return type's text when `->` follows the parameters.
fn return_type(masked: &[char], from: usize) -> Option<String> {
    let arrow = next_code(masked, from)?;
    if masked[arrow] != '-' || masked.get(arrow + 1) != Some(&'>') {
        return None;
    }
    let start = arrow + 2;
    let text: String = masked[start..type_end(masked, start)].iter().collect();
    Some(text.trim().to_string())
}

/// Where a type at depth zero ends: a spec clause, `where`, `{`, or `;`.
fn type_end(masked: &[char], from: usize) -> usize {
    let mut depth = 0usize;
    let mut i = from;
    while i < masked.len() {
        let c = masked[i];
        if depth == 0 && (c == '{' || c == ';') {
            return i;
        }
        if let Some(w) = word_at(masked, i) {
            if depth == 0 && (word_is(w, "where") || SPEC_CLAUSES.iter().any(|k| word_is(w, k))) {
                return i;
            }
            i += w.len();
            continue;
        }
        match c {
            '-' if masked.get(i + 1) == Some(&'>') => i += 1,
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    masked.len()
}

/// `bool`, or `(name: bool)`.
fn is_bool(ty: &str) -> bool {
    let named = ty.strip_prefix('(').and_then(|t| t.strip_suffix(')')).and_then(|t| t.split_once(':')).map(|(_, t)| t);
    named.unwrap_or(ty).trim() == "bool"
}

/// A clause construct that claims the `{` that follows it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// `else`, `loop`, `unsafe`, `by`, or a closure's `|…|` claims the very
    /// next token, if it is a brace.
    NextToken,
    /// `if`, `match`, `while`, or `for` claims the next brace, after its
    /// condition or scrutinee.
    NextBrace,
}

/// The state of reading a signature's clauses, token by token.
#[derive(Default)]
struct Clauses {
    depth: usize,
    /// A specification clause keyword has been seen.
    in_spec: bool,
    /// The tokens since the last `matches` form a pattern, where `Path {`
    /// opens a struct pattern.
    pattern: bool,
    /// Inside a closure's `|…|` parameter list.
    closure: bool,
    pending: Option<Pending>,
    /// The previous significant token was an identifier or a number.
    after_ident: bool,
    /// The previous significant token was an operator, so a `{` next opens
    /// a block expression (`==> { … }`), never the body.
    after_operator: bool,
}

const EXPRESSION_OPERATORS: &str = "=<>+-*/%&|!^?";

impl Clauses {
    fn word(&mut self, w: &[char]) {
        let is = |keys: &[&str]| keys.iter().any(|k| word_is(w, k));
        (self.after_ident, self.after_operator) = (false, false);
        if is(&SPEC_CLAUSES) {
            (self.in_spec, self.pattern, self.pending) = (true, false, None);
        } else if word_is(w, "matches") {
            self.pattern = true;
        } else if is(&["if", "match", "while", "for"]) {
            (self.pattern, self.pending) = (false, Some(Pending::NextBrace));
        } else if is(&["else", "loop", "unsafe", "by"]) {
            (self.pattern, self.pending) = (false, Some(Pending::NextToken));
        } else {
            self.token();
            self.after_ident = true;
        }
    }

    /// A significant token that is not a brace: it uses up a `NextToken` claim.
    fn token(&mut self) {
        if self.pending == Some(Pending::NextToken) {
            self.pending = None;
        }
    }

    fn punct(&mut self, c: char, next: Option<char>) {
        let at_top = self.depth == 0;
        match c {
            '(' | '[' | '{' => self.depth += 1,
            ')' | ']' | '}' => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        if !at_top {
            return;
        }
        self.token();
        (self.after_ident, self.after_operator) = (false, EXPRESSION_OPERATORS.contains(c));
        match c {
            '|' if !self.pattern && next != Some('|') => {
                self.closure = !self.closure;
                self.pending = (!self.closure).then_some(Pending::NextToken);
            }
            ',' => (self.pattern, self.pending) = (false, None),
            '(' | ')' | '[' | ']' | ':' | '.' | '@' => {}
            '&' | '|' if next != Some(c) => {}
            _ => self.pattern = false,
        }
    }

    /// A number at depth zero: an identifier for the brace rule.
    fn number(&mut self) {
        self.token();
        (self.after_ident, self.after_operator) = (true, false);
    }

    /// Whether a `{` at depth zero is claimed by a clause or is the body.
    fn claimed(&self) -> bool {
        self.in_spec && (self.pending.is_some() || self.after_operator || (self.pattern && self.after_ident))
    }

    fn skipped(&mut self) {
        (self.pending, self.after_ident, self.after_operator) = (None, false, false);
    }
}

/// The body's `{`: the first brace at depth zero after the parameters that no
/// return type, `where` clause, or specification clause claims.
fn body_open(masked: &[char], from: usize) -> Option<usize> {
    unclaimed_brace(masked, from, Clauses::default())
}

/// The first `{` at depth zero from `from` that the clause reader, started
/// in state `st`, does not claim; `None` at a `;` first (no body).
fn unclaimed_brace(masked: &[char], from: usize, mut st: Clauses) -> Option<usize> {
    let mut i = from;
    while i < masked.len() {
        let c = masked[i];
        let top = st.depth == 0;
        if c.is_whitespace() {
            i += 1;
        } else if let Some(w) = word_at(masked, i) {
            if top {
                st.word(w);
            }
            i += w.len();
        } else if c.is_ascii_digit() {
            if top {
                st.number();
            }
            i += masked[i..].iter().take_while(|c| is_ident(**c)).count();
        } else if c == '{' && top && !st.claimed() {
            return Some(i);
        } else if c == '{' && top {
            i = matching(masked, i, '{', '}')? + 1;
            st.skipped();
        } else if c == ';' && top {
            return None;
        } else {
            st.punct(c, masked.get(i + 1).copied());
            i += 1;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Lexical helpers
// ---------------------------------------------------------------------------

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The identifier starting at `i`, when `i` starts one.
fn word_at(chars: &[char], i: usize) -> Option<&[char]> {
    let starts =
        chars.get(i).is_some_and(|c| is_ident(*c) && !c.is_ascii_digit()) && (i == 0 || !is_ident(chars[i - 1]));
    if !starts {
        return None;
    }
    let end = chars[i..].iter().position(|c| !is_ident(*c)).map_or(chars.len(), |n| i + n);
    Some(&chars[i..end])
}

fn word_is(word: &[char], text: &str) -> bool {
    word.iter().copied().eq(text.chars())
}

fn next_code(chars: &[char], from: usize) -> Option<usize> {
    chars.get(from..)?.iter().position(|c| !c.is_whitespace()).map(|n| from + n)
}

/// The last significant char before `at`.
fn prev_code(chars: &[char], at: usize) -> Option<usize> {
    chars[..at.min(chars.len())].iter().rposition(|c| !c.is_whitespace())
}

/// The next significant char at or after `from` is the word `w`.
fn word_follows(chars: &[char], from: usize, w: &str) -> bool {
    next_code(chars, from).and_then(|j| word_at(chars, j)).is_some_and(|found| word_is(found, w))
}

/// Every identifier in `r`, with its index, in source order.
fn words_in(chars: &[char], r: Range) -> impl Iterator<Item = (usize, &[char])> {
    let mut i = r.0;
    std::iter::from_fn(move || {
        while i < r.1 {
            match word_at(chars, i) {
                Some(w) => {
                    let at = i;
                    i += w.len();
                    return Some((at, w));
                }
                None => i += 1,
            }
        }
        None
    })
}

/// The index of the bracket closing the one at `open`.
fn matching(chars: &[char], open: usize, l: char, r: char) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in chars.get(open..)?.iter().enumerate() {
        if *c == l {
            depth += 1;
        } else if *c == r {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(open + i);
            }
        }
    }
    None
}

fn closer(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        _ => '}',
    }
}

/// One past the `;` that ends the statement starting before `from`, or the
/// index of the `}` that closes the enclosing block when there is no `;`.
fn statement_end(chars: &[char], from: usize, limit: usize) -> usize {
    let mut depth = 0usize;
    for (i, c) in chars[from..limit].iter().enumerate() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth > 0 => depth -= 1,
            '}' => return from + i,
            ';' if depth == 0 => return from + i + 1,
            _ => {}
        }
    }
    limit
}

/// `r` without trailing whitespace.
fn trim_end(chars: &[char], r: Range) -> Range {
    let end = chars[r.0..r.1].iter().rposition(|c| !c.is_whitespace()).map_or(r.0, |n| r.0 + n + 1);
    (r.0, end)
}

// ---------------------------------------------------------------------------
// Ghost code
// ---------------------------------------------------------------------------

/// `masked` with the ghost regions of `body` blanked (newlines kept).
pub fn ghost_mask(masked: &[char], body: Body) -> Vec<char> {
    let mut out = masked.to_vec();
    let mut i = body.open + 1;
    while i < body.close {
        let Some(word) = word_at(masked, i) else {
            i += 1;
            continue;
        };
        let after = i + word.len();
        match ghost_end(masked, body, word, after) {
            Some(end) => {
                let end = end.min(body.close);
                blank(&mut out, i, end);
                i = end.max(after);
            }
            None => i = after,
        }
    }
    out
}

/// Where the ghost region a word starts would end, if it starts one.
fn ghost_end(masked: &[char], body: Body, word: &[char], after: usize) -> Option<usize> {
    if word_is(word, "proof") && next_code(masked, after).is_some_and(|j| masked[j] == '{') {
        next_code(masked, after).and_then(|j| matching(masked, j, '{', '}')).map(|j| j + 1)
    } else if word_is(word, "assert") || word_is(word, "assume") {
        Some(statement_end(masked, after, body.close))
    } else if LOOP_CLAUSES.iter().any(|k| word_is(word, k)) {
        let clauses = Clauses { in_spec: true, ..Clauses::default() };
        Some(unclaimed_brace(masked, after, clauses).map_or(body.close, |j| j.min(body.close)))
    } else if word_is(word, "let") && (word_follows(masked, after, "ghost") || word_follows(masked, after, "tracked")) {
        Some(statement_end(masked, after, body.close))
    } else if (word_is(word, "Ghost") || word_is(word, "Tracked")) && masked.get(after) == Some(&'(') {
        matching(masked, after, '(', ')').map(|j| j + 1)
    } else {
        None
    }
}

fn blank(out: &mut [char], from: usize, to: usize) {
    for c in &mut out[from..to] {
        if *c != '\n' {
            *c = ' ';
        }
    }
}

// ---------------------------------------------------------------------------
// Operator swaps (v1)
// ---------------------------------------------------------------------------

fn replacement(operator: Operator, index: usize, len: usize, to: &str, change: String) -> Found {
    Found { operator, edits: vec![Edit { index, len, to: to.to_string() }], change }
}

fn binary_mutants(ghost: &[char], body: Body) -> Vec<Found> {
    let mut found = Vec::new();
    let mut i = body.open + 1;
    while i < body.close {
        if !OPERATOR_CHARS.contains(ghost[i]) {
            i += 1;
            continue;
        }
        let end = ghost[i..body.close].iter().position(|c| !OPERATOR_CHARS.contains(*c)).map_or(body.close, |n| i + n);
        let spaced = ghost[i - 1] == ' ' && ghost.get(end) == Some(&' ');
        if spaced {
            let op: String = ghost[i..end].iter().collect();
            found.extend(
                BINARY
                    .iter()
                    .filter(|(from, _, _)| *from == op)
                    .map(|(_, to, kind)| replacement(*kind, i, end - i, to, format!("`{op}` -> `{to}`"))),
            );
        }
        i = end;
    }
    found
}

/// A prefix `!` before an operand (not `!=`, not a macro's `!`).
fn negation_mutants(ghost: &[char], body: Body) -> Vec<Found> {
    (body.open + 1..body.close)
        .filter(|&i| ghost[i] == '!')
        .filter(|&i| ghost.get(i + 1).is_some_and(|c| is_ident(*c) || *c == '('))
        .filter(|&i| !is_ident(ghost[i - 1]) && ghost[i - 1] != '!' && ghost[i - 1] != '=')
        .map(|i| replacement(Operator::Negation, i, 1, "", "`!` removed".to_string()))
        .collect()
}

fn literal_mutants(ghost: &[char], body: Body) -> Vec<Found> {
    words_in(ghost, (body.open + 1, body.close))
        .filter_map(|(i, w)| match () {
            () if word_is(w, "true") => {
                Some(replacement(Operator::Literal, i, 4, "false", "`true` -> `false`".to_string()))
            }
            () if word_is(w, "false") => {
                Some(replacement(Operator::Literal, i, 5, "true", "`false` -> `true`".to_string()))
            }
            () => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Blocks, statements, and names
// ---------------------------------------------------------------------------

/// The first block at depth zero from `from`, before any `;`, `,`, `=>`, or
/// unmatched closer (so a match guard's `if` claims no later block).
fn block_after(ghost: &[char], from: usize, limit: usize) -> Option<Body> {
    let mut depth = 0usize;
    for i in from..limit {
        match ghost[i] {
            '(' | '[' => depth += 1,
            ')' | ']' if depth == 0 => return None,
            ')' | ']' => depth -= 1,
            '{' if depth == 0 => return matching(ghost, i, '{', '}').map(|close| Body { open: i, close }),
            ';' | '}' | ',' if depth == 0 => return None,
            '=' if depth == 0 && ghost.get(i + 1) == Some(&'>') => return None,
            _ => {}
        }
    }
    None
}

/// Where an `if`'s condition starts: past the pattern and `=` of an `if let`,
/// whose struct patterns hold braces of their own.
fn condition_start(ghost: &[char], from: usize, limit: usize) -> usize {
    match next_code(ghost, from).filter(|&j| word_follows(ghost, j, "let")) {
        Some(j) => let_pattern_end(ghost, j + 3, limit) + 1,
        None => from,
    }
}

/// The innermost block of the body holding `pos`; the body itself at depth one.
fn enclosing_block(ghost: &[char], body: Body, pos: usize) -> Option<Body> {
    let mut depth = 0usize;
    for i in (body.open..pos.min(ghost.len())).rev() {
        match ghost[i] {
            '}' => depth += 1,
            '{' if depth == 0 => return matching(ghost, i, '{', '}').map(|close| Body { open: i, close }),
            '{' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Something continues the expression a `}` just closed: `else`, `as`, a
/// method or field, `?`, or the `;` or `,` that ends it as a whole.
fn continues(ghost: &[char], from: usize, limit: usize) -> bool {
    match next_code(ghost, from).filter(|&j| j < limit) {
        None => false,
        Some(j) => {
            matches!(ghost[j], '.' | '?' | ';' | ',') || word_follows(ghost, j, "else") || word_follows(ghost, j, "as")
        }
    }
}

/// The depth-zero statements of a block as `[start, end)` ranges; the last
/// is the tail expression when it does not end in `;`.
fn statements(ghost: &[char], block: Body) -> Vec<Range> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut i = block.open + 1;
    while i < block.close {
        let c = ghost[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let s = *start.get_or_insert(i);
        match c {
            '(' | '[' => i = matching(ghost, i, c, closer(c)).map_or(block.close, |j| j + 1),
            '{' => {
                i = matching(ghost, i, '{', '}').map_or(block.close, |j| j + 1);
                if !continues(ghost, i, block.close) {
                    out.push((s, i));
                    start = None;
                }
            }
            ';' => {
                out.push((s, i + 1));
                start = None;
                i += 1;
            }
            _ => i += 1,
        }
    }
    out.extend(start.map(|s| (s, block.close)));
    out
}

/// A block's tail expression, trimmed, when it has one.
fn tail(ghost: &[char], block: Body) -> Option<Range> {
    let last = statements(ghost, block).last().copied()?;
    (ghost[last.1 - 1] != ';').then(|| trim_end(ghost, last)).filter(|r| r.1 > r.0)
}

/// A lowercase identifier that is neither a path segment, a field label or
/// access, nor a keyword: a local name a pattern may bind and an expression
/// may use.
fn is_name(ghost: &[char], at: usize, w: &[char]) -> bool {
    let lower = w[0].is_lowercase() || (w[0] == '_' && w.len() > 1);
    let path_before =
        prev_code(ghost, at).is_some_and(|b| ghost[b] == '.' || (ghost[b] == ':' && b > 0 && ghost[b - 1] == ':'));
    let colon_after = next_code(ghost, at + w.len()).is_some_and(|a| ghost[a] == ':');
    lower && !path_before && !colon_after && !KEYWORDS.iter().any(|k| word_is(w, k))
}

/// The local names in `r`: what a pattern binds, or what an expression uses.
fn names(ghost: &[char], r: Range) -> Vec<String> {
    words_in(ghost, r).filter(|(i, w)| is_name(ghost, *i, w)).map(|(_, w)| w.iter().collect()).collect()
}

/// Where a `let` pattern ends: at its `:` type, its `=`, or the `;`.
fn let_pattern_end(ghost: &[char], from: usize, limit: usize) -> usize {
    let mut depth = 0usize;
    for i in from..limit {
        match ghost[i] {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 && ghost.get(i + 1) != Some(&':') && ghost.get(i.wrapping_sub(1)) != Some(&':') => {
                return i;
            }
            '=' | ';' if depth == 0 => return i,
            _ => {}
        }
    }
    limit
}

/// The names the `let` statements of a block bind at its depth.
fn let_bound(ghost: &[char], block: Body) -> Vec<String> {
    statements(ghost, block)
        .into_iter()
        .filter(|s| word_at(ghost, s.0).is_some_and(|w| word_is(w, "let")))
        .flat_map(|s| names(ghost, (s.0 + 3, let_pattern_end(ghost, s.0 + 3, s.1))))
        .collect()
}

/// The names an `if let` pattern between `from` and the then-block binds.
fn if_let_bound(ghost: &[char], from: usize, then_open: usize) -> Vec<String> {
    match next_code(ghost, from).filter(|&j| word_follows(ghost, j, "let")) {
        Some(j) => names(ghost, (j + 3, let_pattern_end(ghost, j + 3, then_open))),
        None => Vec::new(),
    }
}

/// `result`, written where `src` binds its names, still compiles where
/// `dst` binds instead: every name it takes from `src` is bound by `dst`.
fn fits(ghost: &[char], result: Range, src: &[String], dst: &[String]) -> bool {
    names(ghost, result).into_iter().filter(|n| src.contains(n)).all(|n| dst.contains(&n))
}

/// One side of an exchange: a result expression and the names its context binds.
struct Side<'a> {
    result: Range,
    bound: &'a [String],
    name: &'static str,
}

/// The swap of two results, or the one-way copy when only one direction
/// compiles; nothing when the results are equal or hold ghost code.
fn exchange(src: &Source, operator: Operator, a: Side, b: Side) -> Option<Found> {
    if !src.exec_only(a.result) || !src.exec_only(b.result) {
        return None;
    }
    let (ta, tb) = (src.text(a.result), src.text(b.result));
    if ta == tb {
        return None;
    }
    let edit =
        |side: &Side, to: &String| Edit { index: side.result.0, len: side.result.1 - side.result.0, to: to.clone() };
    match (fits(src.ghost, a.result, a.bound, b.bound), fits(src.ghost, b.result, b.bound, a.bound)) {
        (true, true) => Some(Found {
            operator,
            edits: vec![edit(&a, &tb), edit(&b, &ta)],
            change: format!("{} and {} swapped", a.name, b.name),
        }),
        (true, false) => Some(Found {
            operator,
            edits: vec![edit(&b, &ta)],
            change: format!("{} takes {}'s result", b.name, a.name),
        }),
        (false, true) => Some(Found {
            operator,
            edits: vec![edit(&a, &tb)],
            change: format!("{} takes {}'s result", a.name, b.name),
        }),
        (false, false) => None,
    }
}

// ---------------------------------------------------------------------------
// Arm swap
// ---------------------------------------------------------------------------

/// One arm of a `match`: its result expression (a block arm's tail) and the
/// names its pattern and its block's `let`s bind.
struct Arm {
    result: Option<Range>,
    bound: Vec<String>,
}

const ARM_NAMES: [&str; 8] = ["arm 1", "arm 2", "arm 3", "arm 4", "arm 5", "arm 6", "arm 7", "arm 8"];

fn arm_swaps(src: &Source, body: Body) -> Vec<Found> {
    matches_in(src.ghost, body)
        .into_iter()
        .flat_map(|block| {
            let arms = arms(src.ghost, block);
            arms.windows(2).enumerate().filter_map(|(n, pair)| arm_pair(src, &pair[0], &pair[1], n)).collect::<Vec<_>>()
        })
        .collect()
}

/// Every `match` block inside the body, in source order.
fn matches_in(ghost: &[char], body: Body) -> Vec<Body> {
    words_in(ghost, (body.open + 1, body.close))
        .filter(|(_, w)| word_is(w, "match"))
        .filter_map(|(i, w)| block_after(ghost, i + w.len(), body.close))
        .collect()
}

/// The first `=>` at depth zero from `from`.
fn fat_arrow(ghost: &[char], from: usize, limit: usize) -> Option<usize> {
    let mut i = from;
    while i < limit {
        match ghost[i] {
            '(' | '[' | '{' => i = matching(ghost, i, ghost[i], closer(ghost[i]))? + 1,
            '=' if ghost.get(i + 1) == Some(&'>') && ghost[i - 1] != '=' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Where an arm's result ends: a `,` at depth zero, the end of a block-like
/// expression nothing continues, or `limit`.
fn expression_end(ghost: &[char], from: usize, limit: usize) -> usize {
    let mut i = from;
    while i < limit {
        match ghost[i] {
            '(' | '[' => i = matching(ghost, i, ghost[i], closer(ghost[i])).map_or(limit, |j| j + 1),
            '{' => {
                let end = matching(ghost, i, '{', '}').map_or(limit, |j| j + 1);
                if !continues(ghost, end, limit) {
                    return end;
                }
                i = end;
            }
            ',' => return i,
            _ => i += 1,
        }
    }
    limit
}

/// The arms of the `match` block, in order.
fn arms(ghost: &[char], block: Body) -> Vec<Arm> {
    let mut out = Vec::new();
    let mut at = next_code(ghost, block.open + 1);
    while let Some(start) = at.filter(|&s| s < block.close) {
        let Some(arrow) = fat_arrow(ghost, start, block.close) else { break };
        let Some(result_start) = next_code(ghost, arrow + 2).filter(|&s| s < block.close) else { break };
        let result_end = expression_end(ghost, result_start, block.close);
        out.push(arm(ghost, (start, arrow), (result_start, result_end)));
        let comma =
            next_code(ghost, result_end).filter(|&j| j < block.close && ghost[j] == ',').map_or(result_end, |j| j + 1);
        at = next_code(ghost, comma);
    }
    out
}

fn arm(ghost: &[char], pattern: Range, result: Range) -> Arm {
    let pattern_bound = names(ghost, trim_end(ghost, pattern));
    let block = (ghost[result.0] == '{')
        .then(|| matching(ghost, result.0, '{', '}'))
        .flatten()
        .filter(|&close| close + 1 == result.1)
        .map(|close| Body { open: result.0, close });
    match block {
        Some(b) => Arm { result: tail(ghost, b), bound: [pattern_bound, let_bound(ghost, b)].concat() },
        None => Arm { result: Some(trim_end(ghost, result)), bound: pattern_bound },
    }
}

fn arm_pair(src: &Source, a: &Arm, b: &Arm, n: usize) -> Option<Found> {
    let name = |k: usize| ARM_NAMES.get(k).copied().unwrap_or("a later arm");
    let first = Side { result: a.result?, bound: &a.bound, name: name(n) };
    let second = Side { result: b.result?, bound: &b.bound, name: name(n + 1) };
    exchange(src, Operator::ArmSwap, first, second)
}

// ---------------------------------------------------------------------------
// Branch swap
// ---------------------------------------------------------------------------

/// An `if` whose `else` is a plain block.
struct IfElse {
    then: Body,
    otherwise: Body,
    /// What an `if let` pattern binds in the then-block.
    pattern: Vec<String>,
}

fn branch_swaps(src: &Source, body: Body) -> Vec<Found> {
    words_in(src.ghost, (body.open + 1, body.close))
        .filter(|(_, w)| word_is(w, "if"))
        .filter_map(|(i, w)| if_else(src.ghost, i + w.len(), body.close))
        .filter_map(|branches| branch_pair(src, &branches))
        .collect()
}

fn if_else(ghost: &[char], from: usize, limit: usize) -> Option<IfElse> {
    let then = block_after(ghost, condition_start(ghost, from, limit), limit)?;
    let else_at = next_code(ghost, then.close + 1).filter(|&j| word_follows(ghost, j, "else"))?;
    let open = next_code(ghost, else_at + 4).filter(|&j| ghost[j] == '{')?;
    let otherwise = Body { open, close: matching(ghost, open, '{', '}')? };
    Some(IfElse { then, otherwise, pattern: if_let_bound(ghost, from, then.open) })
}

fn branch_pair(src: &Source, branches: &IfElse) -> Option<Found> {
    let then_bound = [branches.pattern.clone(), let_bound(src.ghost, branches.then)].concat();
    let else_bound = let_bound(src.ghost, branches.otherwise);
    let first = Side { result: tail(src.ghost, branches.then)?, bound: &then_bound, name: "then-branch" };
    let second = Side { result: tail(src.ghost, branches.otherwise)?, bound: &else_bound, name: "else-branch" };
    exchange(src, Operator::BranchSwap, first, second)
}

// ---------------------------------------------------------------------------
// Return drop
// ---------------------------------------------------------------------------

fn return_drops(src: &Source, body: Body) -> Vec<Found> {
    words_in(src.ghost, (body.open + 1, body.close))
        .filter(|(_, w)| word_is(w, "return"))
        .filter_map(|(i, w)| return_drop(src, body, i, i + w.len()))
        .collect()
}

/// `return …;` at `at`, dropped when it ends the then-block of an `if`
/// without `else`, so what remains still typechecks.
fn return_drop(src: &Source, body: Body, at: usize, after: usize) -> Option<Found> {
    let end = statement_end(src.ghost, after, body.close);
    let block = enclosing_block(src.ghost, body, at).filter(|b| *b != body)?;
    let ends_block = statements(src.ghost, block).last().is_some_and(|s| s.0 == at && s.1 == end);
    if src.ghost[end - 1] != ';'
        || !ends_block
        || !is_bare_then_block(src.ghost, body, block)
        || !src.exec_only((at, end))
    {
        return None;
    }
    Some(replacement(Operator::ReturnDrop, at, end - at, "", "early `return` dropped".to_string()))
}

/// `block` is the then-block of an `if` statement that has no `else`.
fn is_bare_then_block(ghost: &[char], body: Body, block: Body) -> bool {
    let Some(parent) = enclosing_block(ghost, body, block.open) else { return false };
    let Some(stmt) = statements(ghost, parent).into_iter().find(|s| s.0 <= block.open && block.open < s.1) else {
        return false;
    };
    let is_if = word_at(ghost, stmt.0).is_some_and(|w| word_is(w, "if"));
    is_if
        && block_after(ghost, condition_start(ghost, stmt.0 + 2, parent.close), parent.close) == Some(block)
        && !word_follows(ghost, block.close + 1, "else")
}

// ---------------------------------------------------------------------------
// Result negation
// ---------------------------------------------------------------------------

fn result_negation(src: &Source, body: Body) -> Vec<Found> {
    let Some(r) = tail(src.ghost, body) else { return Vec::new() };
    let text = src.text(r);
    if !src.exec_only(r) || text == "true" || text == "false" {
        return Vec::new();
    }
    vec![replacement(Operator::ResultNegation, r.0, r.1 - r.0, &format!("!({text})"), "result negated".to_string())]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"verus! {
pub fn clamp(x: i32, lo: i32, hi: i32) -> (r: i32)
    requires lo <= hi,
    ensures lo <= r <= hi, x < lo ==> r == lo,
{
    let ghost g = lo + hi;
    proof { assert(lo <= hi); }
    assert(lo <= hi) by { assert(true); };
    let mut i: usize = 0;
    while i < 3
        invariant i <= 3, lo <= hi,
        decreases 3 - i,
    {
        i = i + 1;
    }
    let keep = Ghost(lo + hi);
    if x < lo { lo } else if !(x <= hi) { hi } else { x }
}
}
"#;

    /// `derive`'s signature as verusfmt lays it out in proob-core, with
    /// struct patterns, closures, and a parenthesised struct literal in the
    /// clauses, over a body of two statements.
    const DERIVE: &str = r#"verus! {

/// The status of one property from its facts, the gate config, and the instant.
// proob:01M622RQ73XGJ3PRRVCY53FECA
pub fn derive(input: DeriveInput, cfg: &GateConfig, now: Secs) -> (s: Status)
    requires
        paths_distinct(input.scope@),
    ensures
        // Shape.
        s is Scoped <==> input.scope@.len() > 0,
        s matches Status::Scoped { kind, scope_hash, .. } ==> kind == input.kind && scope_hash@ == spec_scope_hash_of(
            input.scope@.to_multiset(),
        ),
        decl_of(s) matches Decl::Waived { window: Window::Active { until, .. }, .. } ==> now < until,
        s matches Status::Scoped { binding: Binding::Bound { binding_hash, approval, previously_approved, .. }, .. }
            ==> input.adapter_available && input.binding == Some(
            BindingFacts { binding_hash, approval, previously_approved },
        ),
        s matches Status::Scoped { kind, scope_hash, binding: Binding::Bound { binding_hash, primary: Primary::Fresh(o), .. }, .. }
            ==> exists|i: int|
            latest_primary(input.records@, input.property.id, kind, i) && (#[trigger] input.records@[i]).scope_hash
                == scope_hash && input.records@[i].binding_hash == binding_hash && input.records@[i].measured
                == Measured::Primary(kind, o),
{
    if input.scope.len() == 0 {
        return Status::ScopeEmpty { decl };
    }
    let bound = if !input.adapter_available {
        Binding::Unbound(UnboundWhy::NoAdapter)
    } else {
        match input.binding {
            None => Binding::Unbound(UnboundWhy::NoCheck),
            Some(f) => Binding::Bound { binding_hash: f.binding_hash },
        }
    };
    Status::Scoped { kind, decl, binding: bound }
}

} // verus!
"#;

    /// `eligibility`'s signature: parenthesised struct literals and `matches`
    /// with tuple-struct patterns, over a body whose `match` is followed by a
    /// `proof` block.
    const ELIGIBILITY: &str = r#"verus! {

// proob:01M622RQ76XZBSVRNHW3PC3Z5S
pub fn eligibility(intro: &Introduction, allow_self_signed: bool) -> (a: Approval)
    ensures
        eligibility_sound(*intro, allow_self_signed, a),
        *intro matches Introduction::Merged(f) ==> (has_eligible_review(f) ==> a == (Approval::Approved {
            sig: Sig::Team,
            pr: f.number,
            merge_sha: f.merge_sha,
        })),
        a == Approval::Unreviewed(UnreviewedReason::NoEligibleReview(Ineligible::BotOnly)) ==> (*intro matches Introduction::Merged(f)
            && forall|i: int|
            0 <= i < f.reviews@.len() && approving(#[trigger] f.reviews@[i]) ==> !(f.reviews@[i].user.kind is Human)),
{
    let a = match intro {
        Introduction::DirectPush => Approval::Unreviewed(UnreviewedReason::DirectPush),
        Introduction::Merged(f) => merged(f, allow_self_signed),
    };
    proof {
        lemma_reasoned_sound(*intro, allow_self_signed, a);
        if let Introduction::Merged(f) = intro {
            if let Approval::Unreviewed(UnreviewedReason::NoEligibleReview(why)) = a {
                lemma_reason_meaning(*f, why);
            }
        }
    }
    a
}

} // verus!
"#;

    /// `verdict`'s signature: long `matches` lines with nested struct
    /// patterns, over a two-arm `match` whose arms bind different names.
    const VERDICT: &str = r#"verus! {

/// The verdict of one property.
// proob:01M622RQ71P6ZXCKVVTRCA4DSE
pub fn verdict(s: &Status, g: &Governance) -> (v: Verdict)
    ensures
        // The table.
        v == spec_verdict(*s, *g),
        v is Holds ==> holds_requirements(*s, *g),
        (*s matches Status::Scoped { kind, decl: Decl::Approved { .. }, binding: Binding::Bound { provisional: Provisional::Provisional(o), .. }, .. }
            && negative(kind, o)) ==> v == Verdict::Broken(BrokenWhy::Negative(Source::ThisPr)),
        v == Verdict::Broken(BrokenWhy::RunnerErrored) ==> (*s matches Status::Scoped { binding: Binding::Bound { provisional: Provisional::Provisional(PrimaryOutcome::Errored), .. }, .. }),
        model_drift_unbroken(*s) matches Some(n) ==> v == Verdict::ReApprove(ReApproveWhy::SpecDrifted(n)),
        v matches Verdict::ReApprove(ReApproveWhy::SpecDrifted(n)) ==> (*s matches Status::Scoped { kind: Kind::Model, decl: Decl::Approved { drift, .. }, .. }
            && drift.commits == n && n > 0),
        (v is Holds || v is Checked) ==> !(g.config is Unreviewed) && !(g.evidence_workflow is Unreviewed),
{
    match *s {
        Status::ScopeEmpty { decl } => verdict_scope_empty(decl),
        Status::Scoped { kind, decl, binding, .. } => verdict_scoped(kind, decl, binding, g),
    }
}

} // verus!
"#;

    fn described(text: &str, line: usize) -> Vec<String> {
        mutants(text, line).iter().map(Mutant::describe).collect()
    }

    fn body_of(text: &str, line: usize) -> (Signature, String) {
        let chars: Vec<char> = text.chars().collect();
        let masked: Vec<char> = mask(text).chars().collect();
        let header = line_start(&chars, line).expect("line");
        let sig = signature(&masked, header).expect("a body");
        (sig, chars[sig.body.open..=sig.body.close].iter().collect())
    }

    /// The 1-based line of the first char at which two texts differ.
    fn first_diff_line(a: &str, b: &str) -> Option<usize> {
        let at = a
            .chars()
            .zip(b.chars())
            .position(|(x, y)| x != y)
            .or_else(|| (a.len() != b.len()).then_some(a.chars().count().min(b.chars().count())))?;
        Some(a.chars().take(at).filter(|c| *c == '\n').count() + 1)
    }

    #[test]
    fn ghost_code_and_the_signature_are_never_mutated() {
        let got = described(SAMPLE, 2);
        assert_eq!(
            got,
            vec![
                "10:13: `<` -> `>`",
                "10:13: `<` -> `<=`",
                "17:10: `<` -> `>`",
                "17:10: `<` -> `<=`",
                "17:34: `<=` -> `<`",
                "17:34: `<=` -> `>=`",
                "14:15: `+` -> `-`",
                "17:30: `!` removed",
                "17:43: then-branch and else-branch swapped",
            ]
        );
    }

    #[test]
    fn applying_a_mutant_changes_exactly_its_operator() {
        let all = mutants(SAMPLE, 2);
        let plus = all.iter().find(|m| m.change == "`+` -> `-`").expect("the arithmetic mutant");
        let mutated = apply(SAMPLE, plus);
        assert!(mutated.contains("i = i - 1;"), "{mutated}");
        assert_eq!(mutated.len(), SAMPLE.len());
        let removed = all.iter().find(|m| m.operator == Operator::Negation).expect("a negation");
        assert!(apply(SAMPLE, removed).contains("else if (x <= hi)"));
        let swapped = all.iter().find(|m| m.operator == Operator::BranchSwap).expect("a branch swap");
        assert!(apply(SAMPLE, swapped).contains("if !(x <= hi) { x } else { hi }"), "{}", apply(SAMPLE, swapped));
    }

    #[test]
    fn literals_generics_and_macros_are_handled() {
        let text = "fn f(v: Vec<u8>, b: bool) -> bool {\n    let ok = b && !done(v.len());\n    assert!(ok);\n    if ok { true } else { false }\n}\n";
        let got = described(text, 1);
        assert_eq!(
            got,
            vec![
                "2:16: `&&` -> `||`",
                "2:19: `!` removed",
                "4:13: `true` -> `false`",
                "4:27: `false` -> `true`",
                "4:13: then-branch and else-branch swapped",
                "4:5: result negated",
            ]
        );
        let negated = mutants(text, 1).into_iter().find(|m| m.operator == Operator::ResultNegation).expect("negated");
        assert!(
            apply(text, &negated).ends_with("    !(if ok { true } else { false })\n}\n"),
            "{}",
            apply(text, &negated)
        );
        assert!(mutants(text, 7).is_empty(), "no such line");
        assert!(mutants("spec fn s() -> bool;\n", 1).is_empty(), "no body");
    }

    // -- the body locator on the real signatures --------------------------

    #[test]
    fn the_body_of_derive_follows_its_clauses_not_the_first_struct_pattern() {
        let (sig, body) = body_of(DERIVE, 5);
        assert!(body.starts_with("{\n    if input.scope.len() == 0 {"), "{body}");
        assert!(body.ends_with("    Status::Scoped { kind, decl, binding: bound }\n}"), "{body}");
        assert_eq!(
            position(&DERIVE.chars().collect::<Vec<_>>(), sig.body.open).0,
            24,
            "the brace on its own line after the clauses"
        );
        assert!(!sig.returns_bool);
    }

    #[test]
    fn the_body_of_eligibility_follows_the_parenthesised_struct_literals() {
        let (_, body) = body_of(ELIGIBILITY, 4);
        assert!(body.starts_with("{\n    let a = match intro {"), "{body}");
        assert!(body.ends_with("    a\n}"), "{body}");
    }

    #[test]
    fn the_body_of_verdict_follows_the_nested_matches_patterns() {
        let (sig, body) = body_of(VERDICT, 5);
        assert_eq!(
            body,
            "{\n    match *s {\n        Status::ScopeEmpty { decl } => verdict_scope_empty(decl),\n        Status::Scoped { kind, decl, binding, .. } => verdict_scoped(kind, decl, binding, g),\n    }\n}"
        );
        assert_eq!(position(&VERDICT.chars().collect::<Vec<_>>(), sig.body.open).0, 17);
    }

    #[test]
    fn hand_written_signatures_without_trailing_commas_or_clauses_locate_too() {
        let (sig, body) = body_of("impl W {\n    pub fn get(&self) -> (r: u32) ensures r == self.0 { self.0 }\n}\n", 2);
        assert_eq!(body, "{ self.0 }");
        assert!(!sig.returns_bool);
        let (sig, body) = body_of("pub fn ok<T: Fn(u8) -> u8>(x: Vec<T>) -> bool where T: Copy { x.len() > 0 }\n", 1);
        assert_eq!(body, "{ x.len() > 0 }");
        assert!(sig.returns_bool);
        let (sig, _) = body_of("fn is_small(n: u8) -> (r: bool)\n    ensures r == (n < 8),\n{\n    n < 8\n}\n", 1);
        assert!(sig.returns_bool);
        let (_, body) = body_of(
            "fn abs(x: int) -> (r: int)\n    ensures r == if x < 0 { -x } else { x }, r matches Some(y) | None,\n{\n    if x < 0 { -x } else { x }\n}\n",
            1,
        );
        assert_eq!(body, "{\n    if x < 0 { -x } else { x }\n}");
        let (_, body) = body_of(
            "fn pick(s: Seq<u8>) -> (r: u8)\n    ensures forall|i: int| 0 <= i < s.len() ==> { s[i] >= r },\n{\n    0\n}\n",
            1,
        );
        assert_eq!(body, "{\n    0\n}");
    }

    /// A `match`, `if`, or pattern name inside parentheses is not at the
    /// clause's depth and claims nothing; otherwise the body would be read
    /// as part of the clause and the next function's body taken instead.
    #[test]
    fn clause_words_inside_parentheses_do_not_push_the_body_to_the_next_function() {
        let (_, body) = body_of(
            "fn f(x: u8) -> (r: u8)\n    ensures r == (match x { 0 => 1, _ => 2 }),\n{\n    1\n}\nfn g() -> u8 { 3 + 4 }\n",
            1,
        );
        assert_eq!(body, "{\n    1\n}");
        let (_, body) = body_of(
            "fn f(o: Option<u8>) -> (r: Option<u8>) ensures r matches Some(y) { o }\nfn g() -> u8 { 3 + 4 }\n",
            1,
        );
        assert_eq!(body, "{ o }");
        let (_, body) =
            body_of("fn f(c: bool) -> (r: u8)\n    ensures r == f(if c { 1 } else { 2 }),\n{\n    9\n}\n", 1);
        assert_eq!(body, "{\n    9\n}");
    }

    #[test]
    fn no_mutant_lands_inside_a_specification_clause() {
        for (text, line) in [(DERIVE, 5), (ELIGIBILITY, 4), (VERDICT, 5), (SAMPLE, 2)] {
            let (sig, _) = body_of(text, line);
            let chars: Vec<char> = text.chars().collect();
            let body_line = position(&chars, sig.body.open).0;
            let all = mutants(text, line);
            assert!(!all.is_empty(), "{text}");
            for m in &all {
                assert!(
                    m.edits.iter().all(|e| e.index > sig.body.open && e.index + e.len <= sig.body.close),
                    "{}",
                    m.describe()
                );
                let first = first_diff_line(text, &apply(text, m)).expect("a change");
                assert!(
                    first > body_line,
                    "{} changes line {first}, before the body's brace on line {body_line}",
                    m.describe()
                );
            }
        }
    }

    // -- the structural operators ------------------------------------------

    #[test]
    fn verdicts_arms_bind_different_names_so_only_the_one_way_copy_is_offered() {
        let got = described(VERDICT, 5);
        assert_eq!(got, vec!["20:55: arm 2 takes arm 1's result"]);
        let mutated = apply(VERDICT, &mutants(VERDICT, 5)[0]);
        assert!(
            mutated.contains("Status::Scoped { kind, decl, binding, .. } => verdict_scope_empty(decl),"),
            "{mutated}"
        );
        assert!(
            mutated.contains("Status::ScopeEmpty { decl } => verdict_scope_empty(decl),"),
            "arm 1 is unchanged: {mutated}"
        );
    }

    #[test]
    fn derive_gets_its_early_return_dropped_and_its_branches_and_arms_exchanged() {
        let got = described(DERIVE, 5);
        assert_eq!(
            got,
            vec![
                "25:26: `==` -> `!=`",
                "28:20: `!` removed",
                "33:24: arm 2 takes arm 1's result",
                "29:9: then-branch and else-branch swapped",
                "26:9: early `return` dropped",
            ]
        );
        let all = mutants(DERIVE, 5);
        let dropped = apply(DERIVE, &all[4]);
        assert!(dropped.contains("if input.scope.len() == 0 {\n        \n    }"), "{dropped}");
        let branch = apply(DERIVE, &all[3]);
        assert!(
            branch.contains("let bound = if !input.adapter_available {\n        match input.binding {"),
            "the moved match brings its own arm bindings: {branch}"
        );
        assert!(branch.contains("    } else {\n        Binding::Unbound(UnboundWhy::NoAdapter)\n    };"), "{branch}");
        let arm = apply(DERIVE, &all[2]);
        assert!(arm.contains("Some(f) => Binding::Unbound(UnboundWhy::NoCheck),"), "{arm}");
    }

    #[test]
    fn a_match_followed_by_a_proof_block_is_mutated_only_outside_it() {
        let got = described(ELIGIBILITY, 4);
        assert_eq!(got, vec!["18:36: arm 2 takes arm 1's result"]);
        let mutated = apply(ELIGIBILITY, &mutants(ELIGIBILITY, 4)[0]);
        assert!(
            mutated.contains("Introduction::Merged(f) => Approval::Unreviewed(UnreviewedReason::DirectPush),"),
            "{mutated}"
        );
        assert!(mutated.contains("lemma_reasoned_sound(*intro, allow_self_signed, a);"), "the proof is intact");
    }

    #[test]
    fn dispatch_arms_swap_pairwise_and_equal_results_give_nothing() {
        let text = "fn cap(o: Outcome, g: Grace) -> (r: Outcome)\n    ensures r == capped(o),\n{\n    match o {\n        Outcome::Block => Outcome::Block,\n        Outcome::Pass => Outcome::Warn,\n        Outcome::Warn => Outcome::Warn,\n        Outcome::Other => match g { Grace::Within => Outcome::Pass, Grace::After => Outcome::Warn },\n    }\n}\n";
        let got = described(text, 1);
        assert_eq!(
            got,
            vec!["5:27: arm 1 and arm 2 swapped", "7:26: arm 3 and arm 4 swapped", "8:54: arm 1 and arm 2 swapped"],
            "arms 2 and 3 return the same word"
        );
        let swapped = apply(text, &mutants(text, 1)[0]);
        assert!(
            swapped.contains("Outcome::Block => Outcome::Warn,\n        Outcome::Pass => Outcome::Block,"),
            "{swapped}"
        );
        let nested = apply(text, &mutants(text, 1)[2]);
        assert!(
            nested.contains("match g { Grace::Within => Outcome::Warn, Grace::After => Outcome::Pass }"),
            "{nested}"
        );
    }

    #[test]
    fn block_arms_swap_their_tails_and_keep_their_statements_and_proofs() {
        let text = "fn f(d: Decl, b: Binding) -> (v: Verdict)\n    ensures true,\n{\n    match d {\n        Decl::Proposed(why) => verdict_proposed(why),\n        Decl::Waived { sig, window, .. } => {\n            if lapsed(d) {\n                return Verdict::Broken;\n            }\n            match (sig, window) {\n                (Sig::Team, Window::Active { until, .. }) => match reapprove(b) {\n                    Some(why) => Verdict::ReApprove(why),\n                    None => Verdict::Waived { until },\n                },\n                _ => signed(Sig::SelfSigned, b),\n            }\n        },\n        Decl::Approved { sig, drift, .. } => {\n            proof { lemma(sig); }\n            let n = count(drift);\n            signed(sig, n)\n        },\n    }\n}\n";
        let got = described(text, 1);
        assert_eq!(
            got,
            vec![
                "11:62: arm 1 takes arm 2's result",
                "12:34: arm 1 takes arm 2's result",
                "8:17: early `return` dropped"
            ],
            "the outer arms each need a name the neighbour lacks (why; sig and window; drift through n)"
        );
        let all = mutants(text, 1);
        assert!(
            apply(text, &all[0]).contains("(Sig::Team, Window::Active { until, .. }) => signed(Sig::SelfSigned, b),")
        );
        assert!(apply(text, &all[1]).contains(
            "Some(why) => Verdict::Waived { until },\n                    None => Verdict::Waived { until },"
        ));
        assert!(
            apply(text, &all[2]).contains("if lapsed(d) {\n                \n            }"),
            "{}",
            apply(text, &all[2])
        );
    }

    #[test]
    fn returns_are_dropped_only_where_the_block_keeps_its_type() {
        let text = "fn f(xs: &Vec<u8>, o: Option<u8>) -> (r: u8)\n    ensures true,\n{\n    if let Some(x) = o {\n        return x;\n    }\n    let mut i: usize = 0;\n    while i < xs.len()\n        invariant i <= xs.len(),\n    {\n        if xs[i] == 7 {\n            assert(i < xs.len());\n            return 7;\n        }\n        i = i + 1;\n    }\n    match o {\n        Some(y) => { return y; },\n        None => {},\n    }\n    if xs.len() > 0 {\n        return 1;\n    } else {\n        return 2;\n    }\n}\n";
        let got: Vec<String> =
            mutants(text, 1).iter().filter(|m| m.operator == Operator::ReturnDrop).map(Mutant::describe).collect();
        assert_eq!(
            got,
            vec!["5:9: early `return` dropped", "13:13: early `return` dropped"],
            "not the arm's, not the if/else's, not one after a proof"
        );
        let last = "fn g(x: u8) -> (r: u8)\n    ensures true,\n{\n    return x;\n}\n";
        assert!(mutants(last, 1).iter().all(|m| m.operator != Operator::ReturnDrop), "the body's last statement stays");
    }

    #[test]
    fn a_match_guards_if_claims_no_later_block_and_struct_patterns_in_if_let_are_skipped() {
        let guard = "fn f(a: A, b: bool, p: Option<u8>) -> (r: u8)\n    ensures true,\n{\n    match a {\n        A::X if b => 1,\n        A::Y => if let Some(x) = p { x } else { 0 },\n    }\n}\n";
        let branches: Vec<String> =
            mutants(guard, 1).iter().filter(|m| m.operator == Operator::BranchSwap).map(Mutant::describe).collect();
        assert_eq!(
            branches,
            vec!["6:38: then-branch takes else-branch's result"],
            "one if/else, read with its `if let` binding"
        );
        let pattern = "fn g(x: P, q: u8) -> (r: u8)\n    ensures true,\n{\n    if let P { a, .. } = x {\n        a\n    } else {\n        q\n    }\n}\n";
        assert_eq!(described(pattern, 1), vec!["5:9: then-branch takes else-branch's result"]);
    }

    #[test]
    fn a_block_inside_a_loop_clause_does_not_end_the_ghost_mask() {
        let text = "fn f(a: &Vec<u8>, n: usize) -> (r: usize)\n    ensures true,\n{\n    let mut i: usize = 0;\n    while i < n\n        invariant\n            forall|j: int| 0 <= j < i ==> { a[j] > 0 },\n            i <= n,\n        decreases n - i,\n    {\n        i = i + 1;\n    }\n    i\n}\n";
        assert_eq!(
            described(text, 1),
            vec!["5:13: `<` -> `>`", "5:13: `<` -> `<=`", "11:15: `+` -> `-`"],
            "nothing from the invariant, nothing from decreases"
        );
    }

    #[test]
    fn if_let_bindings_and_ghost_results_limit_the_branch_swap() {
        let text = "fn f(o: Option<u8>, k: u8) -> (r: u8)\n    ensures true,\n{\n    if let Some(x) = o {\n        x\n    } else {\n        k\n    }\n}\n";
        assert_eq!(described(text, 1), vec!["5:9: then-branch takes else-branch's result"]);
        let ghost = "fn h(a: u8, b: u8) -> (r: u8)\n    ensures true,\n{\n    if a < b {\n        proof { lemma(a); }\n        keep(Ghost(a), b)\n    } else {\n        proof { lemma(b); }\n        a\n    }\n}\n";
        let got: Vec<Operator> = mutants(ghost, 1).iter().map(|m| m.operator).collect();
        assert_eq!(
            got,
            vec![Operator::Relational, Operator::Relational],
            "a result that passes a Ghost argument is not moved"
        );
    }

    #[test]
    fn a_bool_result_is_negated_unless_it_is_a_bare_literal() {
        let chain =
            "fn accept(a: bool, b: bool) -> (r: bool)\n    ensures r <==> (a && b),\n{\n    first(a) && second(b)\n}\n";
        assert_eq!(described(chain, 1), vec!["4:14: `&&` -> `||`", "4:5: result negated"]);
        assert!(apply(chain, &mutants(chain, 1)[1]).contains("{\n    !(first(a) && second(b))\n}"));
        let literal = "fn any(xs: &Vec<u8>) -> (r: bool)\n    ensures true,\n{\n    let n = xs.len();\n    if n == 0 {\n        return false;\n    }\n    true\n}\n";
        let kinds: Vec<Operator> = mutants(literal, 1).iter().map(|m| m.operator).collect();
        assert_eq!(
            kinds,
            vec![Operator::Relational, Operator::Literal, Operator::Literal, Operator::ReturnDrop],
            "the literal flip already covers `true`"
        );
        let unit = "fn touch(x: &mut u8)\n    ensures *x == 1,\n{\n    *x = 1;\n}\n";
        assert!(mutants(unit, 1).is_empty());
    }

    #[test]
    fn the_order_is_by_kind_then_position_and_the_edits_are_disjoint() {
        for (text, line) in [(DERIVE, 5), (VERDICT, 5), (SAMPLE, 2)] {
            let all = mutants(text, line);
            let keys: Vec<(Operator, usize)> = all.iter().map(|m| (m.operator, m.edits[0].index)).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted);
            for m in &all {
                assert!(m.edits.windows(2).all(|w| w[0].index + w[0].len <= w[1].index), "{}", m.describe());
            }
        }
    }
}
