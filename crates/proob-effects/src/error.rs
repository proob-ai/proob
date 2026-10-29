//! Every way an effect can fail, each naming what it was about. Nothing here
//! is a verdict: a failure leaves a file `Unverified`, never `Holds` and
//! never `Broken`.
use proob_facts::error::{ConvertError, ParseError};
use proob_spawn::SpawnError;
use std::fmt;
use std::path::PathBuf;

/// git could not run, failed, or printed something that is not what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    Spawn(SpawnError),
    Failed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
    Unparsable {
        command: String,
        output: String,
    },
    /// A revision argument that could be read as an option; refused before git sees it.
    UnsafeRevision {
        rev: String,
    },
    /// Reading a working-tree file failed for a reason other than absence.
    Read {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::Spawn(e) => write!(f, "git: {e}"),
            GitError::Failed { command, code, stderr } => {
                write!(f, "`{command}` exited with {code:?}: {}", stderr.trim())
            }
            GitError::Unparsable { command, output } => {
                write!(f, "`{command}` printed something unexpected: {output:?}")
            }
            GitError::UnsafeRevision { rev } => write!(f, "revision {rev:?} starts with `-` and was not passed to git"),
            GitError::Read { path, message } => write!(f, "cannot read {}: {message}", path.display()),
        }
    }
}

impl std::error::Error for GitError {}

/// GitHub could not be asked, refused, or answered something unexpected.
/// `path` is the request path without query or token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// `GITHUB_TOKEN` is unset and `gh auth token` gave none.
    NoToken,
    /// The per-run call budget is spent, or GitHub's rate limit is.
    RateLimited,
    /// 401, or a 403 that is not a rate limit.
    Unauthorized {
        path: String,
    },
    NotFound {
        path: String,
    },
    Http {
        status: u16,
        path: String,
    },
    Transport {
        path: String,
        message: String,
    },
    Decode {
        path: String,
        message: String,
    },
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::NoToken => write!(f, "no GitHub token: set GITHUB_TOKEN or sign in with `gh auth login`"),
            ApiError::RateLimited => write!(f, "GitHub call budget spent or rate limit reached"),
            ApiError::Unauthorized { path } => write!(f, "GitHub refused {path}: the token lacks access"),
            ApiError::NotFound { path } => write!(f, "GitHub has no {path}"),
            ApiError::Http { status, path } => write!(f, "GitHub answered {status} for {path}"),
            ApiError::Transport { path, message } => write!(f, "could not reach GitHub for {path}: {message}"),
            ApiError::Decode { path, message } => {
                write!(f, "GitHub's answer for {path} was not the expected shape: {message}")
            }
        }
    }
}

impl std::error::Error for ApiError {}

/// The cache could not be written. Reads never fail: an unreadable entry is a miss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot write cache entry {}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for CacheError {}

/// Facts about an introduction could not be assembled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactsError {
    Api(ApiError),
    Convert(ConvertError),
    /// The PR would merge into a branch other than the default one, so an
    /// "if merged now" row has no meaning for it.
    NotIntoDefaultBranch {
        number: u64,
        base: String,
    },
    /// Two paths of one binding set are equal.
    DuplicatePath {
        path: String,
    },
}

impl fmt::Display for FactsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FactsError::Api(e) => write!(f, "{e}"),
            FactsError::Convert(e) => write!(f, "{e}"),
            FactsError::NotIntoDefaultBranch { number, base } => {
                write!(f, "PR #{number} targets {base}, not the default branch")
            }
            FactsError::DuplicatePath { path } => write!(f, "{path} appears twice in one binding set"),
        }
    }
}

impl std::error::Error for FactsError {}

impl From<ApiError> for FactsError {
    fn from(e: ApiError) -> Self {
        FactsError::Api(e)
    }
}

impl From<ConvertError> for FactsError {
    fn from(e: ConvertError) -> Self {
        FactsError::Convert(e)
    }
}

/// Evidence could not be retrieved. A malformed line is refused by number;
/// an unknown kind is not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceError {
    Api(ApiError),
    Convert(ConvertError),
    Archive { run_id: u64, artifact: u64, message: String },
    MalformedLine { run_id: u64, artifact: u64, line: usize, message: String },
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvidenceError::Api(e) => write!(f, "{e}"),
            EvidenceError::Convert(e) => write!(f, "{e}"),
            EvidenceError::Archive { run_id, artifact, message } => {
                write!(f, "artifact {artifact} of run {run_id} is not a readable evidence archive: {message}")
            }
            EvidenceError::MalformedLine { run_id, artifact, line, message } => {
                write!(f, "artifact {artifact} of run {run_id}, line {line}: {message}")
            }
        }
    }
}

impl std::error::Error for EvidenceError {}

impl From<ApiError> for EvidenceError {
    fn from(e: ApiError) -> Self {
        EvidenceError::Api(e)
    }
}

impl From<ConvertError> for EvidenceError {
    fn from(e: ConvertError) -> Self {
        EvidenceError::Convert(e)
    }
}

/// The facts for one property, or the governance facts, could not be gathered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatherError {
    Git(GitError),
    Api(ApiError),
    Facts(FactsError),
    Evidence(EvidenceError),
    Convert(ConvertError),
    Parse {
        path: String,
        error: ParseError,
    },
    Glob {
        pattern: String,
        message: String,
    },
    /// Two resolved scope or binding paths are equal: a defect in resolution,
    /// reported rather than handed to `derive`.
    DuplicatePath {
        path: String,
    },
    /// A blob git should have (the file at its own introducing commit) is missing.
    MissingBlob {
        rev: String,
        path: String,
    },
    /// A waiver's `issue` does not name an issue number.
    WaiverIssue {
        path: String,
        issue: String,
    },
    /// The kill floor is zero; the config parser refuses this, so it means a hand-built config.
    FloorZero,
}

impl GatherError {
    /// The budget or GitHub's rate limit stopped this file; it renders
    /// `Unverified (rate limited)` rather than `GitHubUnreachable`.
    pub fn is_rate_limited(&self) -> bool {
        matches!(
            self,
            GatherError::Api(ApiError::RateLimited)
                | GatherError::Facts(FactsError::Api(ApiError::RateLimited))
                | GatherError::Evidence(EvidenceError::Api(ApiError::RateLimited))
        )
    }
}

impl fmt::Display for GatherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GatherError::Git(e) => write!(f, "{e}"),
            GatherError::Api(e) => write!(f, "{e}"),
            GatherError::Facts(e) => write!(f, "{e}"),
            GatherError::Evidence(e) => write!(f, "{e}"),
            GatherError::Convert(e) => write!(f, "{e}"),
            GatherError::Parse { path, error } => write!(f, "{path}: {error}"),
            GatherError::Glob { pattern, message } => write!(f, "scope glob {pattern:?}: {message}"),
            GatherError::DuplicatePath { path } => write!(f, "{path} was resolved twice"),
            GatherError::MissingBlob { rev, path } => write!(f, "git has no blob for {path} at {rev}"),
            GatherError::WaiverIssue { path, issue } => {
                write!(f, "{path}: waiver.issue {issue:?} does not name an issue (use #123 or the issue URL)")
            }
            GatherError::FloorZero => write!(f, "kill floor is zero; the schema minimum is one"),
        }
    }
}

impl std::error::Error for GatherError {}

impl From<GitError> for GatherError {
    fn from(e: GitError) -> Self {
        GatherError::Git(e)
    }
}

impl From<ApiError> for GatherError {
    fn from(e: ApiError) -> Self {
        GatherError::Api(e)
    }
}

impl From<FactsError> for GatherError {
    fn from(e: FactsError) -> Self {
        GatherError::Facts(e)
    }
}

impl From<EvidenceError> for GatherError {
    fn from(e: EvidenceError) -> Self {
        GatherError::Evidence(e)
    }
}

impl From<ConvertError> for GatherError {
    fn from(e: ConvertError) -> Self {
        GatherError::Convert(e)
    }
}
