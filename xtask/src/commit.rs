//! Commit subjects against Conventional Commits 1.0.0, as CONTRIBUTING.md
//! states the rules. Pure: `run` reads the commits with `git log` and hands
//! each one here.

/// The types CONTRIBUTING.md allows.
pub const TYPES: &[&str] =
    &["feat", "fix", "docs", "refactor", "perf", "test", "ci", "build", "chore", "bench", "release", "revert"];

/// The longest subject line, in characters.
pub const MAX_LEN: usize = 72;

/// One commit as `git log --format=%H%x1f%P%x1f%s` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub parents: usize,
    pub subject: String,
}

/// Parses one line of that format; `None` for a line that is not one.
pub fn parse_log_line(line: &str) -> Option<Commit> {
    let mut fields = line.splitn(3, '\u{1f}');
    let id = fields.next()?.to_owned();
    let parents = fields.next()?.split_whitespace().count();
    let subject = fields.next()?.to_owned();
    Some(Commit { id, parents, subject })
}

/// Every problem with one commit, each a phrase; empty when it passes.
pub fn lint_commit(commit: &Commit) -> Vec<String> {
    match commit.parents > 1 {
        true => vec!["a merge commit; master's history is linear (rebase instead)".to_owned()],
        false => lint_subject(&commit.subject),
    }
}

/// Every problem with a subject line (a commit's or a pull request title).
pub fn lint_subject(subject: &str) -> Vec<String> {
    let autosquash = ["fixup!", "squash!", "amend!"].iter().find(|p| subject.starts_with(*p));
    if let Some(prefix) = autosquash {
        return vec![format!("a {prefix} commit; squash it before review (git rebase --autosquash)")];
    }
    let length = subject.chars().count();
    let too_long = (length > MAX_LEN).then(|| format!("{length} characters; at most {MAX_LEN}"));
    let shape = match header(subject) {
        Err(why) => vec![why],
        Ok(description) => lint_description(description),
    };
    too_long.into_iter().chain(shape).collect()
}

/// `type(scope)!: description` split at the colon; the description, or why not.
fn header(subject: &str) -> Result<&str, String> {
    let shape = || format!("not `type(scope): description` with a type among {}", TYPES.join(", "));
    let (prefix, description) = subject.split_once(": ").ok_or_else(shape)?;
    let prefix = prefix.strip_suffix('!').unwrap_or(prefix);
    let (kind, scope) = match prefix.split_once('(') {
        None => (prefix, None),
        Some((kind, rest)) => (kind, Some(rest.strip_suffix(')').ok_or_else(shape)?)),
    };
    match (TYPES.contains(&kind), scope) {
        (false, _) => Err(format!("unknown type '{kind}'; one of {}", TYPES.join(", "))),
        (true, Some(scope)) if !scope_ok(scope) => {
            Err(format!("scope '{scope}' is not a lowercase word (letters, digits, -, _, /, .)"))
        }
        (true, _) => Ok(description),
    }
}

fn scope_ok(scope: &str) -> bool {
    !scope.is_empty() && scope.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_/.".contains(c))
}

fn lint_description(description: &str) -> Vec<String> {
    let first = description.split_whitespace().next().unwrap_or("");
    [
        description.trim().is_empty().then(|| "no description after the colon".to_owned()),
        (description != description.trim_start()).then(|| "more than one space after the colon".to_owned()),
        description.ends_with('.').then(|| "ends with a period".to_owned()),
        (!first.is_empty() && !imperative(first))
            .then(|| format!("'{first}' is not imperative; write the subject as a command (add, not added or adds)")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Words ending in -ed or -ing that are themselves imperative.
const ED_ING_VERBS: &[&str] =
    &["embed", "exceed", "feed", "need", "proceed", "seed", "shed", "speed", "succeed", "bring", "ring", "string"];

/// Third-person forms of the verbs commit subjects most often start with.
const THIRD_PERSON: &[&str] = &[
    "adds",
    "allows",
    "bumps",
    "builds",
    "changes",
    "checks",
    "cleans",
    "corrects",
    "creates",
    "deletes",
    "disables",
    "documents",
    "drops",
    "enables",
    "ensures",
    "fixes",
    "handles",
    "implements",
    "improves",
    "introduces",
    "makes",
    "moves",
    "prevents",
    "removes",
    "renames",
    "replaces",
    "resolves",
    "reverts",
    "runs",
    "sets",
    "shows",
    "supports",
    "tests",
    "updates",
    "upgrades",
    "uses",
    "writes",
];

/// A heuristic, as every commit linter's is: past tense, gerunds, and the
/// common third-person forms are refused.
fn imperative(word: &str) -> bool {
    let word = word.to_ascii_lowercase();
    let past_or_gerund = (word.ends_with("ed") || word.ends_with("ing")) && !ED_ING_VERBS.contains(&word.as_str());
    !past_or_gerund && !THIRD_PERSON.contains(&word.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(parents: usize, subject: &str) -> Commit {
        Commit { id: "c0ffee".to_owned(), parents, subject: subject.to_owned() }
    }

    #[test]
    fn conventional_subjects_pass() {
        for subject in [
            "feat(adapter-python): add the mutmut score adapter",
            "fix: name a missing pytest plugin and its fix",
            "feat(cli)!: drop the --legacy flag",
            "refactor!: split the verdict table",
            "docs(readme): embed the demo",
            "ci: build and test macOS on the Intel runner",
            "revert: feat(sim): add the workflows scenario",
            "chore(proob-core): bump vstd",
        ] {
            assert_eq!(lint_subject(subject), Vec::<String>::new(), "{subject}");
        }
    }

    #[test]
    fn the_shape_is_required() {
        assert!(lint_subject("Add the thing")[0].starts_with("not `type(scope): description`"));
        assert!(lint_subject("feat:add")[0].starts_with("not `type"), "a space after the colon");
        assert!(lint_subject("feat(cli: add")[0].starts_with("not `type"), "an unclosed scope");
        assert_eq!(lint_subject("style: wrap lines"), ["unknown type 'style'; one of ".to_owned() + &TYPES.join(", ")]);
        assert_eq!(lint_subject("Feat: add x")[0], "unknown type 'Feat'; one of ".to_owned() + &TYPES.join(", "));
        assert!(lint_subject("feat(CLI): add x")[0].starts_with("scope 'CLI'"));
        assert!(lint_subject("feat(): add x")[0].starts_with("scope ''"));
        assert!(lint_subject("release, ci: build")[0].starts_with("unknown type 'release, ci'"));
    }

    #[test]
    fn the_description_is_checked() {
        assert_eq!(lint_subject("fix: "), ["no description after the colon"]);
        assert_eq!(lint_subject("fix:  two spaces"), ["more than one space after the colon"]);
        assert_eq!(lint_subject("fix: close the file."), ["ends with a period"]);
        assert!(lint_subject("fix: fixed the file")[0].starts_with("'fixed' is not imperative"));
        assert!(lint_subject("fix: Adds a check")[0].starts_with("'Adds' is not imperative"));
        assert!(lint_subject("feat: adding a check")[0].starts_with("'adding' is not imperative"));
        assert!(lint_subject("feat: speed up the scorer").is_empty(), "an -ed verb on the allowlist");
    }

    #[test]
    fn the_length_is_at_most_72_characters() {
        let at_limit = format!("docs: {}", "a".repeat(MAX_LEN - 6));
        assert_eq!(at_limit.chars().count(), MAX_LEN);
        assert!(lint_subject(&at_limit).is_empty());
        assert_eq!(lint_subject(&format!("{at_limit}a")), ["73 characters; at most 72"]);
        let wide = format!("docs: {}", "é".repeat(MAX_LEN - 6));
        assert!(lint_subject(&wide).is_empty(), "characters, not bytes");
    }

    #[test]
    fn autosquash_commits_are_refused() {
        for prefix in ["fixup!", "squash!", "amend!"] {
            let problems = lint_subject(&format!("{prefix} feat: add x"));
            assert_eq!(problems.len(), 1);
            assert!(problems[0].starts_with(&format!("a {prefix} commit")), "{problems:?}");
        }
    }

    #[test]
    fn merge_commits_are_refused() {
        for subject in [
            "Merge pull request #12 from proob-ai/develop",
            "Merge branch 'master' into feat/x",
            "Merge pull request #12 from someone/feature",
            "feat: add x",
        ] {
            assert!(lint_commit(&commit(2, subject))[0].starts_with("a merge commit"), "{subject}");
        }
        assert!(lint_commit(&commit(1, "Merge branch 'develop'"))[0].starts_with("not `type"), "not a merge");
    }

    #[test]
    fn log_lines_parse_with_their_parent_count() {
        let line = "abc\u{1f}p1 p2\u{1f}Merge branch 'develop'";
        assert_eq!(parse_log_line(line), Some(commit_with("abc", 2, "Merge branch 'develop'")));
        assert_eq!(parse_log_line("abc\u{1f}\u{1f}feat: a: b"), Some(commit_with("abc", 0, "feat: a: b")));
        assert_eq!(parse_log_line("abc"), None);
    }

    fn commit_with(id: &str, parents: usize, subject: &str) -> Commit {
        Commit { id: id.to_owned(), parents, subject: subject.to_owned() }
    }
}
