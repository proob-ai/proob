//! New code without a property: the changed source files no property's
//! scope matches and no bound check reads.
//!
//! Informational and unverified on purpose. The list decides nothing: it is
//! never an input to `derive`, `verdict`, or `policy`, so it cannot change a
//! word or an exit code. It lives here rather than in proob-core because its
//! inputs (globs, extensions, an ignore list) are conveniences, not governed
//! facts, and the verified `affected` already answers the question that
//! matters for the gate: which properties a change touches.
//!
//! A changed path is uncovered when all of these hold:
//! - it still exists (a deletion adds no behaviour; a rename counts by its new path);
//! - its extension belongs to a language some configured adapter handles;
//! - no property's scope glob matches it, and it is in no binding set;
//! - it is not under `proob/`, `.github/`, `tests/fixtures/`, or `examples/`,
//!   and no `uncovered_ignore` glob matches it.
use crate::cli::backend::Bindings;
use crate::cli::error::CliError;
use crate::cli::workspace::LoadedProperty;
use proob_effects::gather::{GlobSet, glob_set};
use proob_facts::config::{Config, Language};
use std::collections::BTreeSet;
use std::path::Path;

/// Directories whose files never need a property of their own.
const BUILT_IN_IGNORE: [&str; 4] = ["proob/", ".github/", "tests/fixtures/", "examples/"];

/// The most paths a rendering lists before "and N more".
pub const SHOWN: usize = 10;

/// One changed path, as the tree has it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Added, modified, or the new side of a rename.
    Present(String),
    /// Deleted, or the old side of a rename.
    Gone(String),
}

/// What covers a path.
pub struct Coverage<'a> {
    pub extensions: BTreeSet<&'static str>,
    pub scopes: &'a GlobSet,
    pub bindings: BTreeSet<&'a str>,
    pub ignore: &'a GlobSet,
}

/// The file extensions an adapter's language is written in.
fn extensions_of(language: Language) -> &'static [&'static str] {
    match language {
        Language::Rust | Language::Verus | Language::Kani => &["rs"],
        Language::Python => &["py"],
        Language::Tla => &["tla"],
    }
}

fn is_source(path: &str, extensions: &BTreeSet<&'static str>) -> bool {
    Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| extensions.contains(e))
}

fn is_ignored(path: &str, c: &Coverage<'_>) -> bool {
    BUILT_IN_IGNORE.iter().any(|dir| path.starts_with(dir)) || c.ignore.is_match(path)
}

fn is_uncovered(path: &str, c: &Coverage<'_>) -> bool {
    is_source(path, &c.extensions) && !c.scopes.is_match(path) && !c.bindings.contains(path) && !is_ignored(path, c)
}

/// The uncovered paths among `changes`, sorted and distinct.
pub fn uncovered(changes: &[Change], c: &Coverage<'_>) -> Vec<String> {
    let found: BTreeSet<&str> = changes
        .iter()
        .filter_map(|ch| match ch {
            Change::Present(p) => Some(p.as_str()),
            Change::Gone(_) => None,
        })
        .filter(|p| is_uncovered(p, c))
        .collect();
    found.into_iter().map(str::to_string).collect()
}

fn globs(strings: impl IntoIterator<Item = impl AsRef<str>>) -> Result<GlobSet, CliError> {
    let bytes: Vec<Vec<u8>> = strings.into_iter().map(|s| s.as_ref().as_bytes().to_vec()).collect();
    glob_set(&bytes).map_err(CliError::failed)
}

/// The uncovered list for changed paths of the tree at `root`: the
/// properties' scopes, the binding sets, and `config`'s adapters and ignore list.
pub fn for_tree(
    root: &Path,
    changed: Vec<String>,
    properties: &[LoadedProperty],
    bindings: Option<&Bindings>,
    config: &Config,
) -> Result<Vec<String>, CliError> {
    let scopes = globs(properties.iter().flat_map(|p| p.file.scope.iter()))?;
    let ignore = globs(&config.uncovered_ignore)?;
    let coverage = Coverage {
        extensions: config.adapters.iter().flat_map(|a| extensions_of(a.language).iter().copied()).collect(),
        scopes: &scopes,
        bindings: bindings
            .map(|b| b.by_id.values().flat_map(|s| s.files.iter().map(String::as_str)).collect())
            .unwrap_or_default(),
        ignore: &ignore,
    };
    let changes: Vec<Change> = changed
        .into_iter()
        .map(|p| if root.join(&p).is_file() { Change::Present(p) } else { Change::Gone(p) })
        .collect();
    Ok(uncovered(&changes, &coverage))
}

/// "a, b and N more": at most `SHOWN` paths.
pub fn listing(paths: &[String]) -> String {
    let shown = paths.iter().take(SHOWN).map(String::as_str).collect::<Vec<_>>().join(", ");
    match paths.len().saturating_sub(SHOWN) {
        0 => shown,
        more => format!("{shown} and {more} more"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(globs: &[&str]) -> GlobSet {
        super::globs(globs).expect("globs")
    }

    fn present(paths: &[&str]) -> Vec<Change> {
        paths.iter().map(|p| Change::Present((*p).to_string())).collect()
    }

    fn run(changes: &[Change], scopes: &[&str], bindings: &[&str], ignore: &[&str]) -> Vec<String> {
        let (scopes, ignore) = (set(scopes), set(ignore));
        let c = Coverage {
            extensions: ["py", "rs"].into_iter().collect(),
            scopes: &scopes,
            bindings: bindings.iter().copied().collect(),
            ignore: &ignore,
        };
        uncovered(changes, &c)
    }

    #[test]
    fn a_path_a_scope_matches_is_covered() {
        let got = run(&present(&["src/refunds/service.py", "src/billing/charge.py"]), &["src/billing/*.py"], &[], &[]);
        assert_eq!(got, vec!["src/refunds/service.py"]);
    }

    #[test]
    fn a_single_star_does_not_cross_a_slash() {
        let got = run(&present(&["src/a/b.py"]), &["src/*.py"], &[], &[]);
        assert_eq!(got, vec!["src/a/b.py"]);
    }

    #[test]
    fn a_binding_set_member_is_covered() {
        let got = run(&present(&["tests/test_refunds.py", "src/refunds.py"]), &[], &["tests/test_refunds.py"], &[]);
        assert_eq!(got, vec!["src/refunds.py"]);
    }

    #[test]
    fn built_in_directories_and_the_ignore_list_are_left_out() {
        let changes = present(&[
            "proob/x.py",
            ".github/x.py",
            "tests/fixtures/x.py",
            "examples/x.py",
            "src/gen/x.py",
            "src/x.py",
            "lib/tests/fixtures/y.py",
        ]);
        assert_eq!(run(&changes, &[], &[], &["src/gen/**"]), vec!["lib/tests/fixtures/y.py", "src/x.py"]);
    }

    #[test]
    fn only_source_files_of_a_configured_language_count() {
        let changes = present(&["README.md", "src/a.py", "src/b.rs", "src/c.ts", "Makefile", "src/d.pyc"]);
        assert_eq!(run(&changes, &[], &[], &[]), vec!["src/a.py", "src/b.rs"]);
        let (s, i) = (set(&[]), set(&[]));
        let python_only = Coverage {
            extensions: extensions_of(Language::Python).iter().copied().collect(),
            scopes: &s,
            bindings: BTreeSet::new(),
            ignore: &i,
        };
        assert_eq!(uncovered(&changes, &python_only), vec!["src/a.py"]);
        let tla = Coverage {
            extensions: extensions_of(Language::Tla).iter().copied().collect(),
            scopes: &s,
            bindings: BTreeSet::new(),
            ignore: &i,
        };
        assert!(uncovered(&changes, &tla).is_empty(), "a TLA+ adapter lists only .tla files");
    }

    #[test]
    fn deleted_files_are_left_out_and_a_rename_counts_by_its_new_path() {
        let changes = vec![
            Change::Gone("src/old.py".into()),
            Change::Present("src/new.py".into()),
            Change::Gone("src/removed.py".into()),
        ];
        assert_eq!(run(&changes, &[], &[], &[]), vec!["src/new.py"]);
    }

    #[test]
    fn the_result_is_sorted_and_distinct() {
        let changes = present(&["src/b.py", "src/a.py", "src/b.py"]);
        assert_eq!(run(&changes, &[], &[], &[]), vec!["src/a.py", "src/b.py"]);
    }

    #[test]
    fn listing_caps_at_ten_with_and_n_more() {
        let paths: Vec<String> = (0..13).map(|i| format!("src/m{i:02}.py")).collect();
        let text = listing(&paths);
        assert!(text.starts_with("src/m00.py, src/m01.py"), "{text}");
        assert!(text.ends_with("src/m09.py and 3 more"), "{text}");
        assert_eq!(listing(&paths[..2]), "src/m00.py, src/m01.py");
    }
}
