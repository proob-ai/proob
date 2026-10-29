//! `proob hook session-start`: the one-line status at session start.
//!
//! Reads the hook payload from stdin (tolerating none), looks for a `proob/`
//! directory in the session's working directory, and prints nothing when
//! there is none, unless the repository has a shadow (`proob shadow open`):
//! then the line is the shadow's (`proob (shadow): …`). Otherwise it prints one JSON object whose `systemMessage`
//! (what the user sees; SessionStart shows text only this way) and
//! `additionalContext` (what the model sees) are the same `--summary` line,
//! with the `unverified` marker when the run had no token. An error becomes a
//! line starting `proob:` and still exits 0.
use crate::cli::error::CliError;
use crate::cli::workspace::Workspace;
use crate::cli::{Common, summary_for};
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct Output {
    #[serde(rename = "systemMessage")]
    system_message: String,
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: HookSpecific,
}

#[derive(Serialize)]
struct HookSpecific {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'static str,
    #[serde(rename = "additionalContext")]
    additional_context: String,
}

/// `--repo` when given, else the working directory the payload names, else
/// the process's.
fn cwd_from(payload: &[u8], repo: Option<&Path>) -> Option<PathBuf> {
    repo.map(Path::to_path_buf)
        .or_else(|| {
            serde_json::from_slice::<serde_json::Value>(payload)
                .ok()
                .and_then(|v| v.get("cwd").and_then(|c| c.as_str()).map(PathBuf::from))
        })
        .or_else(|| std::env::current_dir().ok())
}

fn line_for(cwd: &Path) -> Result<String, CliError> {
    let ws = Workspace::open(Some(cwd))?;
    summary_for(&ws, &Common { repo: Some(cwd.to_path_buf()), json: false, default_branch: None, shadow: false })
}

/// The JSON the hook prints, or `None` when the directory has neither a
/// `proob/` nor a shadow.
pub fn output(payload: &[u8], repo: Option<&Path>) -> Option<String> {
    let cwd = cwd_from(payload, repo)?;
    let line = if cwd.join(proob_facts::paths::PROOB_DIR).is_dir() {
        line_for(&cwd).unwrap_or_else(|e| format!("proob: {e}"))
    } else {
        crate::cli::shadow::hook_line(&cwd)?
    };
    let out = Output {
        system_message: line.clone(),
        hook_specific_output: HookSpecific { hook_event_name: "SessionStart", additional_context: line },
    };
    Some(serde_json::to_string(&out).unwrap_or_else(|e| format!("{{\"systemMessage\":\"proob: {e}\"}}")))
}

/// Runs the hook over stdin. Always exits 0.
pub fn run(stdin: &mut dyn Read, repo: Option<&Path>) -> std::process::ExitCode {
    let mut payload = Vec::new();
    let _ = stdin.read_to_end(&mut payload);
    if let Some(json) = output(&payload, repo) {
        println!("{json}");
    }
    std::process::ExitCode::SUCCESS
}
