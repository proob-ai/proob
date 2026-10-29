//! `proob hook pre-tool-use`: fail-fast schema feedback on agent writes.
//!
//! Reads the PreToolUse payload for `Write` and `Edit`. When the target
//! resolves to a property file (`proob/properties/**/*.toml`) or to
//! `proob/config.toml`, it reconstructs the content the tool would leave
//! behind (a Write's content; an Edit's `old_string` replaced by
//! `new_string` in the file's current text, honouring `replace_all`) and runs
//! it through the parsers `proob validate` uses. Invalid content is denied
//! with one readable line naming the file and the rule. Anything else prints
//! nothing, which is no decision: valid content, another path, garbage input,
//! a missing file, an Edit the tool itself will refuse, an internal error.
//!
//! This is feedback, not the honesty boundary. The binary cannot know who
//! writes, so `reviewed` and `waiver` pass here and the skills forbid them in
//! prose; the boundary is derivation from GitHub's records (R3, R4). A hook
//! never exits non-zero (R12).
use crate::cli::workspace::clock;
use proob_core::types::Secs;
use proob_facts::config::{Config, parse_config};
use proob_facts::convert;
use proob_facts::paths::{CONFIG, PROPERTIES_DIR};
use proob_facts::property::parse_property;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

// ---------------------------------------------------------------------------
// Intent: what the tool is about to do, as far as this hook cares
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Write { path: PathBuf, content: String },
    Edit { path: PathBuf, old: String, new: String, all: bool },
    Other,
}

#[derive(Deserialize)]
struct Payload {
    tool_name: Option<String>,
    #[serde(default)]
    tool_input: ToolInput,
    cwd: Option<PathBuf>,
}

#[derive(Deserialize, Default)]
struct ToolInput {
    file_path: Option<String>,
    content: Option<String>,
    old_string: Option<String>,
    new_string: Option<String>,
    #[serde(default)]
    replace_all: bool,
}

/// One hook invocation: the tool's intent and the session's working
/// directory, which is the repository root this hook measures paths from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub cwd: Option<PathBuf>,
    pub intent: Intent,
}

/// The call in a payload; anything unreadable is `Other`.
pub fn call(payload: &[u8]) -> Call {
    let Ok(Payload { tool_name, tool_input, cwd }) = serde_json::from_slice::<Payload>(payload) else {
        return Call { cwd: None, intent: Intent::Other };
    };
    let at = |file: String| resolve(Path::new(&file), cwd.as_deref());
    let intent = match (tool_name.as_deref(), tool_input) {
        (Some("Write"), ToolInput { file_path: Some(f), content: Some(content), .. }) => {
            Intent::Write { path: at(f), content }
        }
        (
            Some("Edit"),
            ToolInput { file_path: Some(f), old_string: Some(old), new_string: Some(new), replace_all, .. },
        ) => Intent::Edit { path: at(f), old, new, all: replace_all },
        _ => Intent::Other,
    };
    Call { cwd, intent }
}

impl Intent {
    fn path(&self) -> Option<&Path> {
        match self {
            Intent::Write { path, .. } | Intent::Edit { path, .. } => Some(path),
            Intent::Other => None,
        }
    }
}

/// `path` under `cwd` when relative, with `.` and `..` folded lexically
/// (nothing is touched on disk).
fn resolve(path: &Path, cwd: Option<&Path>) -> PathBuf {
    let joined = match cwd {
        Some(dir) if path.is_relative() => dir.join(path),
        _ => path.to_path_buf(),
    };
    joined.components().fold(PathBuf::new(), |mut acc, c| {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                acc.pop();
            }
            other => acc.push(other.as_os_str()),
        }
        acc
    })
}

// ---------------------------------------------------------------------------
// Governed: which paths this hook reads, by their components alone
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    Property,
    Config,
}

/// A path this hook checks: the repository root (what precedes `proob/`),
/// the root-relative path for the message, and the schema that reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Governed {
    pub root: PathBuf,
    pub relative: String,
    pub schema: Schema,
}

fn parts(path: &Path) -> Option<Vec<&str>> {
    path.components().map(|c| c.as_os_str().to_str()).collect()
}

/// `proob/config.toml`: the index of its `proob` component.
fn config_at(parts: &[&str]) -> Option<usize> {
    let (dir, file) = CONFIG.split_once('/')?;
    let n = parts.len();
    (n >= 2 && parts[n - 2] == dir && parts[n - 1] == file).then_some(n - 2)
}

/// `proob/properties/**/<name>.toml`: the index of the last `proob` that
/// precedes `properties` with a file below it. The last one, because the
/// repository itself may live in a directory called `proob`.
fn property_at(parts: &[&str]) -> Option<usize> {
    let (dir, sub) = PROPERTIES_DIR.split_once('/')?;
    let toml = parts.last().is_some_and(|f| f.ends_with(".toml") && !f.starts_with('.'));
    let candidates = 0..parts.len().saturating_sub(2);
    toml.then(|| candidates.rev().find(|&i| parts[i] == dir && parts[i + 1] == sub)).flatten()
}

/// Where `proob/` starts in `parts`, and which schema reads the file.
fn locate(parts: &[&str]) -> Option<(usize, Schema)> {
    config_at(parts).map(|i| (i, Schema::Config)).or_else(|| property_at(parts).map(|i| (i, Schema::Property)))
}

/// `Some` when the hook checks this path. Inside the session's working
/// directory (the repository), the path must start with `proob/` right
/// there: `docs/proob/properties/x.toml` is documentation, not a property.
/// Outside it (another checkout, or no `cwd` in the payload) the components
/// alone decide.
pub fn governed(path: &Path, cwd: Option<&Path>) -> Option<Governed> {
    match cwd.and_then(|dir| path.strip_prefix(dir).ok().map(|rel| (dir, rel))) {
        Some((dir, rel)) => {
            let parts = parts(rel)?;
            let (at, schema) = locate(&parts).filter(|(at, _)| *at == 0)?;
            Some(Governed { root: dir.to_path_buf(), relative: parts[at..].join("/"), schema })
        }
        None => {
            let parts = parts(path)?;
            let (at, schema) = locate(&parts)?;
            Some(Governed { root: parts[..at].iter().collect(), relative: parts[at..].join("/"), schema })
        }
    }
}

// ---------------------------------------------------------------------------
// The would-be content and its problem
// ---------------------------------------------------------------------------

/// The text the tool leaves behind, or `None` when the tool itself will
/// refuse (an Edit whose `old_string` is absent, empty, or ambiguous without
/// `replace_all`), so there is nothing for this hook to say.
pub fn would_be(intent: &Intent, current: Option<&str>) -> Option<String> {
    match (intent, current) {
        (Intent::Write { content, .. }, _) => Some(content.clone()),
        (Intent::Edit { old, .. }, _) if old.is_empty() => None,
        (Intent::Edit { old, new, all, .. }, Some(text)) => match (text.matches(old.as_str()).count(), all) {
            (0, _) => None,
            (1, _) | (_, true) => Some(text.replace(old.as_str(), new)),
            (_, false) => None,
        },
        (Intent::Edit { .. }, None) | (Intent::Other, _) => None,
    }
}

/// A TOML error spans several lines (a code frame); the hook's reason is one.
fn one_line(message: &str) -> String {
    let frame = |l: &str| l.is_empty() || l.starts_with('|') || l.contains(" | ") || l.starts_with('^');
    message.lines().map(str::trim).filter(|l| !frame(l)).collect::<Vec<_>>().join("; ")
}

/// Why `text` is not an instance of `schema`, as one line; `None` is valid.
pub fn problem(schema: Schema, text: &str, now: Secs, waiver_max_days: u32) -> Option<String> {
    match schema {
        Schema::Config => parse_config(text).err().map(|e| one_line(&e.to_string())),
        Schema::Property => parse_property(text)
            .map_err(|e| one_line(&e.to_string()))
            .and_then(|file| convert::property(&file, now, waiver_max_days).map(drop).map_err(|e| e.to_string()))
            .err(),
    }
}

// ---------------------------------------------------------------------------
// Decide (pure over a reader) and run (the one place with effects)
// ---------------------------------------------------------------------------

pub type Reader<'a> = &'a dyn Fn(&Path) -> Option<String>;

/// `waiver_max_days` from the repository's config when it parses, else the
/// built-in default (the same fallback `proob validate` applies).
fn waiver_max_days(read: Reader<'_>, root: &Path) -> u32 {
    read(&root.join(CONFIG))
        .and_then(|t| parse_config(&t).ok())
        .map_or(Config::defaults().waiver_max_days, |c| c.waiver_max_days)
}

/// The reason to deny, or `None` for no decision.
pub fn decide(call: &Call, read: Reader<'_>, now: Secs) -> Option<String> {
    let intent = &call.intent;
    let path = intent.path()?;
    let g = governed(path, call.cwd.as_deref())?;
    let current = matches!(intent, Intent::Edit { .. }).then(|| read(path)).flatten();
    let text = would_be(intent, current.as_deref())?;
    let max_days = match g.schema {
        Schema::Property => waiver_max_days(read, &g.root),
        Schema::Config => 0,
    };
    problem(g.schema, &text, now, max_days).map(|why| format!("proob: {}: {why}", g.relative))
}

#[derive(Serialize)]
struct Output {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: HookSpecific,
}

#[derive(Serialize)]
struct HookSpecific {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'static str,
    #[serde(rename = "permissionDecision")]
    permission_decision: &'static str,
    #[serde(rename = "permissionDecisionReason")]
    permission_decision_reason: String,
}

/// The deny object, as Claude Code reads it.
pub fn deny_json(reason: String) -> String {
    let out = Output {
        hook_specific_output: HookSpecific {
            hook_event_name: "PreToolUse",
            permission_decision: "deny",
            permission_decision_reason: reason,
        },
    };
    serde_json::to_string(&out).unwrap_or_default()
}

/// Runs the hook over stdin. Prints the deny object or nothing; always exits
/// 0, even if something below panics.
pub fn run(stdin: &mut dyn Read) -> std::process::ExitCode {
    let mut payload = Vec::new();
    let _ = stdin.read_to_end(&mut payload);
    let read = |p: &Path| std::fs::read_to_string(p).ok();
    let decision = std::panic::catch_unwind(|| decide(&call(&payload), &read, clock())).ok().flatten();
    if let Some(reason) = decision {
        println!("{}", deny_json(reason));
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "id = \"01J9Z3W1V8Q4K7N2M5R8T1X4A1\"\ntitle = \"A\"\nstatement = \"A, always.\"\nscope = [\"src/lib.rs\"]\nseverity = \"high\"\nrationale = \"Because.\"\n";

    fn write(path: &str, content: &str) -> Call {
        Call {
            cwd: Some(PathBuf::from("/r")),
            intent: Intent::Write { path: PathBuf::from(path), content: content.to_string() },
        }
    }

    #[test]
    fn governed_paths_inside_the_working_directory_start_with_proob() {
        let cwd = Some(Path::new("/r"));
        let g = governed(Path::new("/r/proob/properties/x.toml"), cwd).expect("property");
        assert_eq!(
            (g.root.as_path(), g.relative.as_str(), g.schema),
            (Path::new("/r"), "proob/properties/x.toml", Schema::Property)
        );
        let nested = governed(Path::new("/r/proob/properties/drafts/y.toml"), cwd).expect("nested");
        assert_eq!(nested.relative, "proob/properties/drafts/y.toml");
        let c = governed(Path::new("/r/proob/config.toml"), cwd).expect("config");
        assert_eq!((c.schema, c.root), (Schema::Config, PathBuf::from("/r")));
        for other in [
            "/r/src/lib.rs",
            "/r/proob/properties/README.md",
            "/r/proob/properties",
            "/r/proob/other.toml",
            "/r/properties/x.toml",
            "/r/docs/proob/properties/x.toml",
            "/r/docs/proob/config.toml",
        ] {
            assert_eq!(governed(Path::new(other), cwd), None, "{other}");
        }
    }

    #[test]
    fn governed_paths_outside_the_working_directory_go_by_components() {
        // The repository itself may be called proob: the last proob/properties pair counts.
        let g = governed(Path::new("/home/u/proob/proob/properties/x.toml"), Some(Path::new("/elsewhere")))
            .expect("property");
        assert_eq!((g.root.as_path(), g.relative.as_str()), (Path::new("/home/u/proob"), "proob/properties/x.toml"));
        let c = governed(Path::new("/other/proob/config.toml"), None).expect("config");
        assert_eq!((c.schema, c.root), (Schema::Config, PathBuf::from("/other")));
        assert_eq!(governed(Path::new("proob/properties/x.toml"), None).map(|g| g.root), Some(PathBuf::new()));
        assert_eq!(governed(Path::new("/other/src/x.toml"), None), None);
    }

    #[test]
    fn relative_paths_resolve_against_cwd_lexically() {
        let payload = br#"{"tool_name":"Write","cwd":"/r/sub","tool_input":{"file_path":"../proob/properties/./x.toml","content":"x"}}"#;
        let c = call(payload);
        assert_eq!(c.intent, Intent::Write { path: PathBuf::from("/r/proob/properties/x.toml"), content: "x".into() });
        assert_eq!(c.cwd.as_deref(), Some(Path::new("/r/sub")));
    }

    #[test]
    fn unreadable_payloads_are_other() {
        assert_eq!(call(b"not json {").intent, Intent::Other);
        assert_eq!(call(b"").intent, Intent::Other);
        assert_eq!(
            call(br#"{"tool_name":"Write","tool_input":{"file_path":"/r/proob/config.toml"}}"#).intent,
            Intent::Other
        );
        assert_eq!(call(br#"{"tool_name":"Bash","tool_input":{"command":"rm"}}"#).intent, Intent::Other);
    }

    #[test]
    fn edits_apply_once_or_everywhere_and_never_when_the_tool_would_refuse() {
        let edit = |old: &str, new: &str, all: bool| Intent::Edit {
            path: PathBuf::from("/r/proob/properties/x.toml"),
            old: old.into(),
            new: new.into(),
            all,
        };
        assert_eq!(would_be(&edit("a", "b", false), Some("a-a")), None, "ambiguous");
        assert_eq!(would_be(&edit("a", "b", true), Some("a-a")).as_deref(), Some("b-b"));
        assert_eq!(would_be(&edit("a", "b", false), Some("x-a")).as_deref(), Some("x-b"));
        assert_eq!(would_be(&edit("z", "b", false), Some("a-a")), None, "absent");
        assert_eq!(would_be(&edit("", "b", false), Some("a-a")), None, "empty");
        assert_eq!(would_be(&edit("a", "b", false), None), None, "missing file");
    }

    #[test]
    fn problems_are_one_line_and_name_the_rule() {
        assert_eq!(problem(Schema::Property, VALID, 0, 90), None);
        let status = problem(Schema::Property, &format!("{VALID}status = \"holds\"\n"), 0, 90).expect("status");
        assert!(status.contains("`status`") && status.contains("derived"), "{status}");
        let unknown = problem(Schema::Property, &format!("{VALID}foo = 1\n"), 0, 90).expect("unknown");
        assert!(!unknown.contains('\n') && unknown.contains("foo"), "{unknown}");
        let empty = problem(Schema::Property, &VALID.replace("[\"src/lib.rs\"]", "[]"), 0, 90).expect("empty scope");
        assert!(empty.contains("scope"), "{empty}");
        let floor = problem(Schema::Config, "schema_version = 1\nkill_floor = 0\n[policy]\nholds = \"pass\"\nchecked = \"pass\"\nproposed = \"pass\"\nwaived = \"pass\"\nnot_checked_within_grace = \"pass\"\nnot_checked_after_grace = \"warn\"\nre_approve = \"warn\"\nbroken_affected = \"block\"\nbroken_unaffected = \"pass\"\n", 0, 0).expect("floor");
        assert!(floor.contains("kill_floor"), "{floor}");
    }

    #[test]
    fn decide_denies_only_invalid_governed_content() {
        let none: Reader<'_> = &|_| None;
        assert_eq!(decide(&write("/r/proob/properties/x.toml", VALID), none, 0), None);
        assert_eq!(decide(&write("/r/src/x.toml", "status = 1"), none, 0), None);
        let reason = decide(&write("/r/proob/properties/x.toml", "status = \"holds\"\n"), none, 0).expect("deny");
        assert!(reason.starts_with("proob: proob/properties/x.toml: "), "{reason}");
        assert!(!reason.contains('\n'));
        let json: serde_json::Value = serde_json::from_str(&deny_json(reason)).expect("json");
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(json["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    }
}
