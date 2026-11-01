//! `proob validate`: the proob files are what their schemas say, binding
//! tokens are well-formed and bind a property, the fact cache is untracked,
//! and no proposal carries a secret. Every problem names its file (and line
//! where there is one). The adapters' `discover` is the authority on binding
//! tokens in the sources they own; a cheap grep covers every other tracked
//! file, so a token in a language without an adapter is still checked
//! against the `proob:<ULID>` grammar.
use crate::cli::adapters::registry;
use crate::cli::error::CliError;
use crate::cli::workspace::{Workspace, parse_config_text};
use proob_adapters::{AdapterError, Availability, availability};
use proob_core::types::Kind;
use proob_effects::git::Repo;
use proob_facts::config::{Config, Language};
use proob_facts::convert;
use proob_facts::paths::{CACHE_DIR, CONFIG};
use proob_facts::property::{KindName, parse_property};
use proob_facts::ulid;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub message: String,
}

impl Problem {
    fn at(file: &str, line: Option<usize>, message: impl Into<String>) -> Problem {
        Problem { file: file.to_string(), line, message: message.into() }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(n) => write!(f, "{}:{n}: {}", self.file, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

/// Every problem in the working tree, in file order.
pub fn run(ws: &Workspace, repo: &dyn Repo) -> Result<Vec<Problem>, CliError> {
    let mut problems = Vec::new();
    let config = check_config(ws, &mut problems)?;
    let ids = check_properties(ws, config.waiver_max_days, &mut problems)?;
    let tracked = repo.ls_files().map_err(CliError::failed)?;
    check_cache_untracked(&tracked, &mut problems);
    let owned = check_bindings(ws, &config, &ids, &mut problems);
    check_tokens(ws, &tracked, &owned, &mut problems)?;
    Ok(problems)
}

fn check_config(ws: &Workspace, problems: &mut Vec<Problem>) -> Result<Config, CliError> {
    match ws.read_optional(CONFIG)? {
        None => {
            problems.push(Problem::at(CONFIG, None, "missing; run `proob init`"));
            Ok(Config::defaults())
        }
        Some(text) => {
            scan_secrets(CONFIG, &text, problems);
            match parse_config_text(&text, CONFIG) {
                Ok(c) => Ok(c),
                Err(e) => {
                    problems.push(Problem::at(
                        CONFIG,
                        None,
                        e.to_string().trim_start_matches(&format!("{CONFIG}: ")).to_string(),
                    ));
                    Ok(Config::defaults())
                }
            }
        }
    }
}

/// A property file as `check_bindings` needs it: its path and declared kind.
pub struct Declared {
    pub path: String,
    pub kind: Option<KindName>,
}

/// Checks every property file and returns the declarations by id.
fn check_properties(
    ws: &Workspace,
    waiver_max_days: u32,
    problems: &mut Vec<Problem>,
) -> Result<BTreeMap<u128, Declared>, CliError> {
    let mut seen: BTreeMap<u128, Declared> = BTreeMap::new();
    for path in ws.property_paths()? {
        let text = ws.read_text(&path)?;
        scan_secrets(&path, &text, problems);
        let file = match parse_property(&text) {
            Ok(file) => file,
            Err(e) => {
                problems.push(Problem::at(&path, None, e.to_string()));
                continue;
            }
        };
        if let Err(e) = convert::property(&file, ws.now, waiver_max_days) {
            problems.push(Problem::at(&path, None, e.to_string()));
        }
        if let Ok(id) = ulid::decode(&file.id)
            && let Some(other) = seen.insert(id, Declared { path: path.clone(), kind: file.kind })
        {
            problems.push(Problem::at(&path, None, format!("id {} is also the id of {}", file.id, other.path)));
        }
    }
    Ok(seen)
}

fn check_cache_untracked(tracked: &[String], problems: &mut Vec<Problem>) {
    let prefix = format!("{CACHE_DIR}/");
    for path in tracked.iter().filter(|p| p.starts_with(&prefix)) {
        problems.push(Problem::at(path, None, format!("{CACHE_DIR} must not be tracked: it holds fetched facts, never content to review; add `{CACHE_DIR}/` to .gitignore and `git rm --cached` it")));
    }
}

// ---------------------------------------------------------------------------
// Binding tokens
// ---------------------------------------------------------------------------

/// Discovery through every configured adapter: a malformed, dangling, or
/// duplicated token is a problem at its line, and so is a token whose id
/// names no property, or a proof bound to a property that does not say
/// `kind = "proof"` (its evidence would never count as positive). Returns
/// the source suffix and roots the adapters own, which the grep below then
/// leaves alone.
fn check_bindings(
    ws: &Workspace,
    config: &Config,
    ids: &BTreeMap<u128, Declared>,
    problems: &mut Vec<Problem>,
) -> Vec<(&'static str, String)> {
    let adapters = match registry(config) {
        Ok(adapters) => adapters,
        Err(e) => {
            problems.push(Problem::at(
                CONFIG,
                None,
                e.to_string().trim_start_matches(&format!("{CONFIG}: ")).to_string(),
            ));
            return Vec::new();
        }
    };
    for adapter in &adapters {
        match adapter.discover(&ws.root) {
            Ok(bindings) => {
                for b in &bindings {
                    match ids.get(&b.id) {
                        None => {
                            let path = b.token.path.to_string_lossy().into_owned();
                            problems.push(Problem::at(
                                &path,
                                Some(b.token.line),
                                format!(
                                    "binding token `proob:{}` names no property under proob/properties/",
                                    ulid::encode(b.id)
                                ),
                            ));
                        }
                        Some(d) if b.kind == Kind::Proof && d.kind.map(convert::kind) != Some(Kind::Proof) => {
                            problems.push(Problem::at(
                                &d.path,
                                None,
                                format!(
                                    "bound to {} at {}, so its primary evidence is a proof: add `kind = \"proof\"`",
                                    proof_item(adapter.language()),
                                    b.token
                                ),
                            ));
                        }
                        Some(d) if b.kind == Kind::Model && d.kind.map(convert::kind) != Some(Kind::Model) => {
                            problems.push(Problem::at(
                                &d.path,
                                None,
                                format!(
                                    "bound to a TLA+ definition at {}, so its primary evidence is a model check: set `kind = \"model\"`",
                                    b.token
                                ),
                            ));
                        }
                        Some(_) => {}
                    }
                }
            }
            Err(AdapterError::Token { at, problem }) => {
                problems.push(Problem::at(&at.path.to_string_lossy(), Some(at.line), problem.to_string()));
            }
            Err(AdapterError::Duplicate { id, first, second }) => {
                problems.push(Problem::at(
                    &second.path.to_string_lossy(),
                    Some(second.line),
                    format!("binding token `proob:{id}` is already bound at {first}; a property binds one check"),
                ));
            }
            Err(other) => problems.push(Problem::at(
                CONFIG,
                None,
                format!("adapter {}: {other}", proob_adapters::language_name(adapter.language())),
            )),
        }
    }
    config
        .adapters
        .iter()
        .filter(|a| availability(a.language) == Availability::Available)
        .map(|a| {
            let root = a.root.trim_start_matches("./").trim_end_matches('/').to_string();
            match a.language {
                Language::Rust | Language::Verus | Language::Kani => (".rs", root),
                Language::Python => (".py", root),
                Language::Tla => (".tla", root),
            }
        })
        .collect()
}

/// What a proof is bound to, by the language of the adapter that bound it.
pub fn proof_item(l: Language) -> &'static str {
    match l {
        Language::Kani => "a Kani harness",
        Language::Verus => "a verus! item",
        Language::Rust | Language::Python | Language::Tla => "a proof",
    }
}

const TOKEN_PREFIX: &str = "proob:";
const MAX_SCAN_BYTES: u64 = 2 * 1024 * 1024;

fn token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.'
}

/// Candidate tokens on a line: `proob:` followed by a run of token bytes at
/// least twenty long (shorter runs are skill names such as `/proob:sense`).
fn candidate_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.match_indices(TOKEN_PREFIX).filter_map(move |(at, _)| {
        let rest = &line[at + TOKEN_PREFIX.len()..];
        let len = rest.bytes().take_while(|b| token_byte(*b)).count();
        (len >= 20).then_some(&rest[..len])
    })
}

/// A file an adapter's `discover` already read.
fn owned_by_adapter(path: &str, owned: &[(&str, String)]) -> bool {
    owned.iter().any(|(suffix, root)| {
        path.ends_with(suffix) && (root.is_empty() || root == "." || path.starts_with(&format!("{root}/")))
    })
}

/// A grep over the tracked text files the adapters do not own, for
/// `proob:<ULID>` tokens that are not one: a truncated or versioned ULID
/// binds nothing.
fn check_tokens(
    ws: &Workspace,
    tracked: &[String],
    owned: &[(&str, String)],
    problems: &mut Vec<Problem>,
) -> Result<(), CliError> {
    for path in tracked.iter().filter(|p| !owned_by_adapter(p, owned)) {
        let full = ws.root.join(path);
        let small = std::fs::metadata(&full).map(|m| m.len() <= MAX_SCAN_BYTES).unwrap_or(false);
        if !small {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&full) else { continue };
        for (i, line) in text.lines().enumerate() {
            for token in candidate_tokens(line) {
                if let Err(e) = ulid::decode(token) {
                    problems.push(Problem::at(
                        path,
                        Some(i + 1),
                        format!("binding token `proob:{token}` is not `proob:<ULID>`: {e}"),
                    ));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

fn scan_secrets(path: &str, text: &str, problems: &mut Vec<Problem>) {
    for (i, line) in text.lines().enumerate() {
        if let Some(what) = secret_in(line) {
            problems.push(Problem::at(
                path,
                Some(i + 1),
                format!("looks like a secret ({what}); proposals must not carry credentials"),
            ));
        }
    }
}

fn run_len(bytes: &[u8], ok: impl Fn(u8) -> bool) -> usize {
    bytes.iter().take_while(|b| ok(**b)).count()
}

fn after<'a>(line: &'a str, prefix: &str) -> impl Iterator<Item = &'a [u8]> {
    line.match_indices(prefix).map(move |(at, _)| &line.as_bytes()[at + prefix.len()..])
}

fn secret_in(line: &str) -> Option<&'static str> {
    let upper_alnum = |b: u8| b.is_ascii_uppercase() || b.is_ascii_digit();
    let aws = ["AKIA", "ASIA"].iter().any(|p| after(line, p).any(|rest| run_len(rest, upper_alnum) >= 16));
    if aws {
        return Some("AWS access key id");
    }
    let gh_classic = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
        .iter()
        .any(|p| after(line, p).any(|rest| run_len(rest, |b| b.is_ascii_alphanumeric()) >= 36));
    if gh_classic
        || after(line, "github_pat_").any(|rest| run_len(rest, |b| b.is_ascii_alphanumeric() || b == b'_') >= 22)
    {
        return Some("GitHub token");
    }
    if line.contains("-----BEGIN") {
        return Some("private key or certificate block");
    }
    assigned_blob(line).then_some("long value assigned to a secret, token, or password key")
}

/// `secret|token|password`, optional spaces, `=` or `:`, optional spaces and
/// quote, then thirty-two or more base64-ish characters.
fn assigned_blob(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    ["secret", "token", "password"].iter().any(|key| {
        after(&lower, key).any(|rest| {
            let mut i = run_len(rest, |b| b == b' ' || b == b'\t');
            if !matches!(rest.get(i), Some(b'=' | b':')) {
                return false;
            }
            i += 1;
            i += run_len(&rest[i..], |b| b == b' ' || b == b'\t');
            if matches!(rest.get(i), Some(b'"' | b'\'')) {
                i += 1;
            }
            run_len(&rest[i..], |b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'_' | b'-')) >= 32
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_patterns() {
        assert_eq!(secret_in("key = \"AKIAIOSFODNN7EXAMPLE\""), Some("AWS access key id"));
        assert_eq!(secret_in("ghp_abcdefghijklmnopqrstuvwxyz0123456789"), Some("GitHub token"));
        assert_eq!(secret_in("github_pat_11ABCDEFG0123456789abcdefghij"), Some("GitHub token"));
        assert_eq!(secret_in("-----BEGIN RSA PRIVATE KEY-----"), Some("private key or certificate block"));
        assert!(secret_in("password = 'c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0'").is_some());
        assert_eq!(secret_in("statement = \"The token bucket never overfills.\""), None);
        assert_eq!(secret_in("rationale = \"A password is checked in constant time.\""), None);
    }

    #[test]
    fn token_candidates() {
        assert_eq!(candidate_tokens("/proob:sense and proob:declare").count(), 0);
        let line = "fn proob_01J9Z3W1V8Q4K7N2M5R8T1X4C6() {} // proob:01J9Z3W1V8Q4K7N2M5R8T1X4C6 proob:01J9Z3W1V8Q4K7N2M5R8T1X4C";
        let tokens: Vec<&str> = candidate_tokens(line).collect();
        assert_eq!(tokens, vec!["01J9Z3W1V8Q4K7N2M5R8T1X4C6", "01J9Z3W1V8Q4K7N2M5R8T1X4C"]);
    }
}
