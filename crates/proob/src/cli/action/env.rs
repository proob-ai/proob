//! The GitHub Actions environment, read once into a closed `ActionEnv`, and
//! the three channels a job writes back through: the step summary file, the
//! step outputs file, and workflow-command annotations on stdout.
//!
//! Every variable a job depends on is required and named in its error, so a
//! run outside Actions, or under a runner that stopped setting one, fails
//! with the variable's name rather than a wrong verdict. The event is a
//! closed sum over the four events the two proob workflows run on;
//! `pull_request_target` is refused by name, because it would run a fork's
//! code with the base repository's token.
use crate::cli::error::CliError;
use proob_facts::hex;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The event that started this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    PullRequest(PullRequestEvent),
    Push,
    Schedule,
    WorkflowDispatch,
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::PullRequest(_) => "pull_request",
            Event::Push => "push",
            Event::Schedule => "schedule",
            Event::WorkflowDispatch => "workflow_dispatch",
        }
    }
}

/// What a `pull_request` event says about the pull request: its number and
/// the base and head shas from the payload, the branch names from
/// `GITHUB_BASE_REF` and `GITHUB_HEAD_REF`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestEvent {
    pub number: u64,
    pub base_ref: String,
    pub base_sha: String,
    pub head_ref: String,
    pub head_sha: String,
}

/// `GITHUB_WORKFLOW_REF`, split: the workflow file's repository-relative path
/// and the ref it ran from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRef {
    pub path: String,
    pub git_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionEnv {
    pub event: Event,
    /// `owner/name`.
    pub repository: String,
    /// The repository's default branch, when the event payload names it.
    pub default_branch: Option<String>,
    /// `GITHUB_SHA`: the commit this run is about and the one checked out
    /// (the merge commit for a pull request).
    pub sha: String,
    /// `GITHUB_REF`.
    pub git_ref: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub workflow: WorkflowRef,
    pub step_summary: PathBuf,
    pub output: PathBuf,
    /// `RUNNER_TEMP`, when the runner sets it.
    pub runner_temp: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

fn missing(name: &str) -> CliError {
    CliError::Usage(format!("{name} is not set; `proob action` runs inside a GitHub Actions job"))
}

/// A required variable: set and non-empty.
fn var(name: &str) -> Result<String, CliError> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).ok_or_else(|| missing(name))
}

fn optional(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn parsed<T: std::str::FromStr>(name: &str, what: &str) -> Result<T, CliError> {
    let text = var(name)?;
    text.parse().map_err(|_| CliError::Usage(format!("{name} is not {what}: {text:?}")))
}

fn sha(name: &str, text: &str) -> Result<String, CliError> {
    hex::decode_oid(text)
        .map(|_| text.to_ascii_lowercase())
        .map_err(|e| CliError::Usage(format!("{name} is not a commit id: {e}")))
}

#[derive(Deserialize)]
struct Payload {
    #[serde(default)]
    pull_request: Option<PullRequestPayload>,
    #[serde(default)]
    repository: Option<RepositoryPayload>,
}

#[derive(Deserialize)]
struct PullRequestPayload {
    number: u64,
    base: RefPayload,
    head: RefPayload,
}

#[derive(Deserialize)]
struct RefPayload {
    sha: String,
}

#[derive(Deserialize)]
struct RepositoryPayload {
    #[serde(default)]
    default_branch: Option<String>,
}

fn payload() -> Result<Payload, CliError> {
    let path = var("GITHUB_EVENT_PATH")?;
    let text = std::fs::read_to_string(&path)
        .map_err(|e| CliError::failed(format!("cannot read GITHUB_EVENT_PATH {path}: {e}")))?;
    serde_json::from_str(&text).map_err(|e| CliError::invalid(path, format!("not a GitHub event payload: {e}")))
}

fn pull_request_event(payload: &Payload) -> Result<PullRequestEvent, CliError> {
    let pr = payload.pull_request.as_ref().ok_or_else(|| {
        CliError::Usage("GITHUB_EVENT_NAME is pull_request but the event payload has no `pull_request`".into())
    })?;
    Ok(PullRequestEvent {
        number: pr.number,
        base_ref: var("GITHUB_BASE_REF")?,
        base_sha: sha("pull_request.base.sha", &pr.base.sha)?,
        head_ref: var("GITHUB_HEAD_REF")?,
        head_sha: sha("pull_request.head.sha", &pr.head.sha)?,
    })
}

fn event(payload: &Payload) -> Result<Event, CliError> {
    let name = var("GITHUB_EVENT_NAME")?;
    match name.as_str() {
        "pull_request" => pull_request_event(payload).map(Event::PullRequest),
        "push" => Ok(Event::Push),
        "schedule" => Ok(Event::Schedule),
        "workflow_dispatch" => Ok(Event::WorkflowDispatch),
        "pull_request_target" => Err(CliError::Usage(
            "proob action refuses pull_request_target: it would run a fork's code with this repository's token; use `on: pull_request`".into(),
        )),
        other => Err(CliError::Usage(format!(
            "proob action runs on pull_request, push, schedule, or workflow_dispatch; this run's event is {other}"
        ))),
    }
}

fn workflow_ref(repository: &str) -> Result<WorkflowRef, CliError> {
    let text = var("GITHUB_WORKFLOW_REF")?;
    let rest = text
        .strip_prefix(&format!("{repository}/"))
        .ok_or_else(|| CliError::Usage(format!("GITHUB_WORKFLOW_REF {text:?} does not start with {repository}/")))?;
    let (path, git_ref) =
        rest.split_once('@').ok_or_else(|| CliError::Usage(format!("GITHUB_WORKFLOW_REF {text:?} has no `@<ref>`")))?;
    Ok(WorkflowRef { path: path.to_string(), git_ref: git_ref.to_string() })
}

/// The environment of this job, or the first thing missing from it.
pub fn read() -> Result<ActionEnv, CliError> {
    let payload = payload()?;
    let repository = var("GITHUB_REPOSITORY")?;
    Ok(ActionEnv {
        event: event(&payload)?,
        default_branch: payload.repository.and_then(|r| r.default_branch).filter(|b| !b.is_empty()),
        sha: sha("GITHUB_SHA", &var("GITHUB_SHA")?)?,
        git_ref: var("GITHUB_REF")?,
        run_id: parsed("GITHUB_RUN_ID", "a run id")?,
        run_attempt: parsed("GITHUB_RUN_ATTEMPT", "an attempt number")?,
        workflow: workflow_ref(&repository)?,
        step_summary: PathBuf::from(var("GITHUB_STEP_SUMMARY")?),
        output: PathBuf::from(var("GITHUB_OUTPUT")?),
        runner_temp: optional("RUNNER_TEMP").map(PathBuf::from),
        repository,
    })
}

/// The variables a token travels in. An execute job must see none of them.
pub const TOKEN_VARS: [&str; 2] = ["GITHUB_TOKEN", "GH_TOKEN"];

/// The first token variable that is set, by name only; its value is never read into a string that is printed.
pub fn token_present() -> Option<&'static str> {
    TOKEN_VARS.into_iter().find(|name| optional(name).is_some())
}

// ---------------------------------------------------------------------------
// Writing back
// ---------------------------------------------------------------------------

/// Appends `text` to the step summary or the outputs file.
pub fn append(path: &Path, text: &str) -> Result<(), CliError> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| CliError::failed(format!("cannot open {}: {e}", path.display())))?;
    file.write_all(text.as_bytes()).map_err(|e| CliError::failed(format!("cannot write {}: {e}", path.display())))
}

/// One `GITHUB_OUTPUT` entry: `name=value`, or the heredoc form when the
/// value spans lines.
pub fn output_line(name: &str, value: &str) -> String {
    if !value.contains(['\n', '\r']) {
        return format!("{name}={value}\n");
    }
    let delimiter =
        (0u32..).map(|n| format!("proob_output_{n}")).find(|d| !value.contains(d.as_str())).unwrap_or_default();
    format!("{name}<<{delimiter}\n{value}\n{delimiter}\n")
}

/// Writes every (name, value) pair to the outputs file.
pub fn set_outputs(path: &Path, pairs: &[(&str, String)]) -> Result<(), CliError> {
    let text: String = pairs.iter().map(|(k, v)| output_line(k, v)).collect();
    append(path, &text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Notice,
    Warning,
    Error,
}

fn escape_message(text: &str) -> String {
    text.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A")
}

fn escape_property(text: &str) -> String {
    escape_message(text).replace(':', "%3A").replace(',', "%2C")
}

/// A workflow command annotation, escaped as GitHub requires. `props` are
/// `title`, `file`, `line`, and the like.
pub fn annotation(level: Level, props: &[(&str, &str)], message: &str) -> String {
    let word = match level {
        Level::Notice => "notice",
        Level::Warning => "warning",
        Level::Error => "error",
    };
    let props: Vec<String> = props.iter().map(|(k, v)| format!("{k}={}", escape_property(v))).collect();
    let joined = if props.is_empty() { String::new() } else { format!(" {}", props.join(",")) };
    format!("::{word}{joined}::{}", escape_message(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotations_escape_newlines_and_property_separators() {
        let line = annotation(Level::Error, &[("file", "a,b:c.toml"), ("title", "proob")], "one\ntwo 100%");
        assert_eq!(line, "::error file=a%2Cb%3Ac.toml,title=proob::one%0Atwo 100%25");
        assert_eq!(annotation(Level::Notice, &[], "hi"), "::notice::hi");
    }

    #[test]
    fn multiline_outputs_use_a_delimiter_absent_from_the_value() {
        assert_eq!(output_line("verdict", "pass"), "verdict=pass\n");
        let multi = output_line("x", "a\nproob_output_0\nb");
        assert_eq!(multi, "x<<proob_output_1\na\nproob_output_0\nb\nproob_output_1\n");
    }
}
