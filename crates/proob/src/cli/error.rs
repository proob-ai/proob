//! Every way a command fails to do what was asked. All exit 2: a block is not
//! an error but an `Outcome`, and a verdict is never an error.
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// The invocation cannot be carried out as asked; says what to do instead.
    Usage(String),
    /// A proob file is not valid; names the file and the field.
    Invalid { file: String, message: String },
    /// An effect (git, the file system, GitHub) failed in a way no view can
    /// absorb.
    Failed(String),
    /// A shadow-mode command cannot proceed; the message carries the fix in
    /// the same line and prints as `shadow: <message>`.
    Shadow(String),
}

impl CliError {
    pub fn invalid(file: impl Into<String>, message: impl fmt::Display) -> CliError {
        CliError::Invalid { file: file.into(), message: message.to_string() }
    }

    pub fn failed(message: impl fmt::Display) -> CliError {
        CliError::Failed(message.to_string())
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(m) => f.write_str(m),
            CliError::Invalid { file, message } => write!(f, "{file}: {message}"),
            CliError::Failed(m) => f.write_str(m),
            CliError::Shadow(m) => write!(f, "shadow: {m}"),
        }
    }
}

impl std::error::Error for CliError {}
