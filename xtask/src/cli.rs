//! The argv grammar: one subcommand, then arguments only `test` and `ci` accept.

use std::fmt;

use crate::plan::CI_ORDER;

/// One exec command, as ci.yml runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    Build,
    /// The arguments after `--`, handed to nextest.
    Test(Vec<String>),
    Verify,
    VerifyNegative,
    CrateGraph,
    ShimTests,
    HookTests,
    TrustedBase,
    Declaration,
    PluginValidate,
    ReadmeSync,
    /// The scenario names (or `--list`), handed to the sim binary.
    Sim(Vec<String>),
    /// The arguments after `bench replay`, handed to bench/harness/replay.py's `run`.
    BenchReplay(Vec<String>),
    /// Score target/sim/verdicts.jsonl with SV-COMP; fails on any wrong `Holds`.
    BenchSoundness,
    Fmt,
    Clippy,
    CommitLint(Lint),
}

/// What `commit-lint` checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lint {
    /// Every commit of `plan::DEFAULT_RANGE`.
    Default,
    /// Every commit of a revision range, `<base>..<head>`.
    Range(String),
    /// One subject line, a pull request title.
    Title(String),
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Run(Task),
    Ci { dry_run: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Unknown(String),
    Unexpected { command: &'static str, arg: String },
}

impl Task {
    pub fn name(&self) -> &'static str {
        match self {
            Task::Build => "build",
            Task::Test(_) => "test",
            Task::Verify => "verify",
            Task::VerifyNegative => "verify-negative",
            Task::CrateGraph => "crate-graph",
            Task::ShimTests => "shim-tests",
            Task::ReadmeSync => "readme-sync",
            Task::HookTests => "hook-tests",
            Task::TrustedBase => "trusted-base",
            Task::Declaration => "declaration",
            Task::PluginValidate => "plugin-validate",
            Task::Sim(_) => "sim",
            Task::BenchReplay(_) => "bench replay",
            Task::BenchSoundness => "bench soundness",
            Task::Fmt => "fmt",
            Task::Clippy => "clippy",
            Task::CommitLint(_) => "commit-lint",
        }
    }

    /// The one line `--help` prints for this task.
    pub fn describe(&self) -> &'static str {
        match self {
            Task::Build => "cargo build --locked --workspace",
            Task::Test(_) => "cargo nextest run --locked --workspace --no-fail-fast [-- nextest args]",
            Task::Verify => "cargo verus verify proob-core under --no-cheating (pinned Verus release)",
            Task::VerifyNegative => "tools/ci/verify-negative.sh: the verify job can fail (minutes)",
            Task::CrateGraph => "tools/crate-graph-check.sh, then tools/ci/crate-graph-negative.sh",
            Task::ShimTests => "tests/shim/test_shim.sh",
            Task::ReadmeSync => "tools/readme-sync.sh (translations match README.md)",
            Task::HookTests => "tests/hooks/test_hooks.sh",
            Task::TrustedBase => "line count of crates/proob-trusted/src/*.rs",
            Task::Declaration => "build proob, validate the declaration, derive status with no token",
            Task::PluginValidate => "claude plugin validate --strict .",
            Task::Sim(_) => "proob end to end against local stand-ins [scenario...] [--list]",
            Task::BenchReplay(_) => "replay the regression corpus bench/corpus/ [entry...] [--list] [-j N]",
            Task::BenchSoundness => "score target/sim/verdicts.jsonl (SV-COMP); exit 1 on any wrong Holds",
            Task::Fmt => "cargo fmt --check",
            Task::Clippy => "cargo clippy --workspace --all-targets --locked -- -D warnings",
            Task::CommitLint(_) => {
                "Conventional Commit subjects of [<base>..<head>] (default origin/master..HEAD) or --title <t>"
            }
        }
    }
}

pub fn parse(args: &[String]) -> Result<Command, ParseError> {
    let (head, rest) = match args.split_first() {
        None => return Ok(Command::Help),
        Some((head, rest)) => (head.as_str(), rest),
    };
    match head {
        "--help" | "-h" | "help" => Ok(Command::Help),
        "test" => parse_test(rest),
        "ci" => parse_ci(rest),
        "commit-lint" => parse_commit_lint(rest),
        "sim" => Ok(Command::Run(Task::Sim(rest.to_vec()))),
        "bench" => match rest.split_first() {
            Some((sub, args)) if sub == "replay" => Ok(Command::Run(Task::BenchReplay(args.to_vec()))),
            Some((sub, [])) if sub == "soundness" => Ok(Command::Run(Task::BenchSoundness)),
            Some((sub, [arg, ..])) if sub == "soundness" => {
                Err(ParseError::Unexpected { command: "bench soundness", arg: arg.clone() })
            }
            Some((sub, _)) => Err(ParseError::Unknown(format!("bench {sub}"))),
            None => Err(ParseError::Unknown("bench".to_owned())),
        },
        name => parse_plain(name, rest),
    }
}

fn parse_plain(name: &str, rest: &[String]) -> Result<Command, ParseError> {
    let task = plain_task(name).ok_or_else(|| ParseError::Unknown(name.to_owned()))?;
    match rest.first() {
        None => Ok(Command::Run(task)),
        Some(arg) => Err(ParseError::Unexpected { command: task.name(), arg: arg.clone() }),
    }
}

fn plain_task(name: &str) -> Option<Task> {
    match name {
        "build" => Some(Task::Build),
        "verify" => Some(Task::Verify),
        "verify-negative" => Some(Task::VerifyNegative),
        "crate-graph" => Some(Task::CrateGraph),
        "shim-tests" => Some(Task::ShimTests),
        "readme-sync" => Some(Task::ReadmeSync),
        "hook-tests" => Some(Task::HookTests),
        "trusted-base" => Some(Task::TrustedBase),
        "declaration" => Some(Task::Declaration),
        "plugin-validate" => Some(Task::PluginValidate),
        "fmt" => Some(Task::Fmt),
        "clippy" => Some(Task::Clippy),
        _ => None,
    }
}

fn parse_test(rest: &[String]) -> Result<Command, ParseError> {
    match rest.split_first() {
        None => Ok(Command::Run(Task::Test(Vec::new()))),
        Some((sep, extra)) if sep == "--" => Ok(Command::Run(Task::Test(extra.to_vec()))),
        Some((arg, _)) => Err(ParseError::Unexpected { command: "test", arg: arg.clone() }),
    }
}

fn parse_commit_lint(rest: &[String]) -> Result<Command, ParseError> {
    let lint = match rest {
        [] => Lint::Default,
        [flag, title] if flag == "--title" => Lint::Title(title.clone()),
        [range] if range.contains("..") => Lint::Range(range.clone()),
        [arg, ..] => return Err(ParseError::Unexpected { command: "commit-lint", arg: arg.clone() }),
    };
    Ok(Command::Run(Task::CommitLint(lint)))
}

fn parse_ci(rest: &[String]) -> Result<Command, ParseError> {
    match rest {
        [] => Ok(Command::Ci { dry_run: false }),
        [flag] if flag == "--dry-run" => Ok(Command::Ci { dry_run: true }),
        [arg, ..] => Err(ParseError::Unexpected { command: "ci", arg: arg.clone() }),
    }
}

pub fn help() -> String {
    let tasks: String = CI_ORDER.iter().map(|task| format!("  {:<16} {}\n", task.name(), task.describe())).collect();
    format!(
        "usage: cargo xtask <command> [args]\n\n\
         commands, in the order `ci` runs them:\n{tasks}\
         \x20 {:<16} all of the above; keeps going past failures, prints a summary, exits 1 on any\n\
         \x20 {:<16} print the plan without running anything\n\n\
         prerequisites and what each command runs: xtask/README.md\n",
        "ci", "ci --dry-run"
    )
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Unknown(name) => write!(f, "unknown command '{name}'; see cargo xtask --help"),
            ParseError::Unexpected { command: "test", arg } => {
                write!(f, "test: unexpected argument '{arg}' (nextest arguments go after --)")
            }
            ParseError::Unexpected { command: "commit-lint", arg } => {
                write!(f, "commit-lint: unexpected argument '{arg}' (takes <base>..<head> or --title <title>)")
            }
            ParseError::Unexpected { command: "ci", arg } => {
                write!(f, "ci: unexpected argument '{arg}' (only --dry-run is accepted)")
            }
            ParseError::Unexpected { command, arg } => {
                write!(f, "{command}: takes no arguments, got '{arg}'")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn nothing_or_a_help_flag_is_help() {
        for words in [&[][..], &["--help"], &["-h"], &["help"]] {
            assert_eq!(parse(&args(words)), Ok(Command::Help), "{words:?}");
        }
    }

    #[test]
    fn every_plain_command_parses_to_its_task() {
        let cases = [
            ("build", Task::Build),
            ("verify", Task::Verify),
            ("verify-negative", Task::VerifyNegative),
            ("crate-graph", Task::CrateGraph),
            ("shim-tests", Task::ShimTests),
            ("readme-sync", Task::ReadmeSync),
            ("hook-tests", Task::HookTests),
            ("trusted-base", Task::TrustedBase),
            ("declaration", Task::Declaration),
            ("plugin-validate", Task::PluginValidate),
            ("fmt", Task::Fmt),
            ("clippy", Task::Clippy),
        ];
        for (word, task) in cases {
            assert_eq!(parse(&args(&[word])), Ok(Command::Run(task.clone())));
            assert_eq!(task.name(), word);
        }
    }

    #[test]
    fn plain_commands_take_no_arguments() {
        assert_eq!(
            parse(&args(&["build", "--release"])),
            Err(ParseError::Unexpected { command: "build", arg: "--release".into() })
        );
    }

    #[test]
    fn test_passes_through_arguments_after_a_double_dash() {
        assert_eq!(parse(&args(&["test"])), Ok(Command::Run(Task::Test(vec![]))));
        assert_eq!(parse(&args(&["test", "--", "-p", "xtask"])), Ok(Command::Run(Task::Test(args(&["-p", "xtask"])))));
        assert_eq!(parse(&args(&["test", "--"])), Ok(Command::Run(Task::Test(vec![]))));
    }

    #[test]
    fn test_refuses_arguments_without_the_double_dash() {
        assert_eq!(parse(&args(&["test", "-p"])), Err(ParseError::Unexpected { command: "test", arg: "-p".into() }));
    }

    #[test]
    fn sim_passes_every_argument_to_the_sim_binary() {
        assert_eq!(parse(&args(&["sim"])), Ok(Command::Run(Task::Sim(vec![]))));
        assert_eq!(
            parse(&args(&["sim", "lifecycle", "honesty"])),
            Ok(Command::Run(Task::Sim(args(&["lifecycle", "honesty"]))))
        );
        assert_eq!(parse(&args(&["sim", "--list"])), Ok(Command::Run(Task::Sim(args(&["--list"])))));
    }

    #[test]
    fn bench_replay_passes_every_argument_to_the_replayer() {
        assert_eq!(parse(&args(&["bench", "replay"])), Ok(Command::Run(Task::BenchReplay(vec![]))));
        assert_eq!(
            parse(&args(&["bench", "replay", "-j", "2", "url-t1"])),
            Ok(Command::Run(Task::BenchReplay(args(&["-j", "2", "url-t1"]))))
        );
        assert_eq!(parse(&args(&["bench"])), Err(ParseError::Unknown("bench".into())));
        assert_eq!(parse(&args(&["bench", "run"])), Err(ParseError::Unknown("bench run".into())));
    }

    #[test]
    fn bench_soundness_takes_no_arguments() {
        assert_eq!(parse(&args(&["bench", "soundness"])), Ok(Command::Run(Task::BenchSoundness)));
        assert_eq!(
            parse(&args(&["bench", "soundness", "x"])),
            Err(ParseError::Unexpected { command: "bench soundness", arg: "x".into() })
        );
    }

    #[test]
    fn commit_lint_takes_a_range_or_a_title() {
        assert_eq!(parse(&args(&["commit-lint"])), Ok(Command::Run(Task::CommitLint(Lint::Default))));
        assert_eq!(
            parse(&args(&["commit-lint", "abc..def"])),
            Ok(Command::Run(Task::CommitLint(Lint::Range("abc..def".into()))))
        );
        assert_eq!(
            parse(&args(&["commit-lint", "--title", "feat: x"])),
            Ok(Command::Run(Task::CommitLint(Lint::Title("feat: x".into()))))
        );
        for bad in [&["commit-lint", "HEAD"][..], &["commit-lint", "--title"], &["commit-lint", "a..b", "c"]] {
            assert!(matches!(parse(&args(bad)), Err(ParseError::Unexpected { command: "commit-lint", .. })), "{bad:?}");
        }
    }

    #[test]
    fn ci_takes_only_dry_run() {
        assert_eq!(parse(&args(&["ci"])), Ok(Command::Ci { dry_run: false }));
        assert_eq!(parse(&args(&["ci", "--dry-run"])), Ok(Command::Ci { dry_run: true }));
        assert_eq!(
            parse(&args(&["ci", "--fast"])),
            Err(ParseError::Unexpected { command: "ci", arg: "--fast".into() })
        );
        assert_eq!(
            parse(&args(&["ci", "--dry-run", "x"])),
            Err(ParseError::Unexpected { command: "ci", arg: "--dry-run".into() })
        );
    }

    #[test]
    fn unknown_commands_are_named_in_the_error() {
        assert_eq!(parse(&args(&["bulid"])), Err(ParseError::Unknown("bulid".into())));
        assert_eq!(ParseError::Unknown("bulid".into()).to_string(), "unknown command 'bulid'; see cargo xtask --help");
    }

    #[test]
    fn help_lists_every_task_and_ci() {
        let text = help();
        for task in CI_ORDER.iter() {
            assert!(text.contains(task.name()), "help lacks {}", task.name());
            assert!(text.contains(task.describe()), "help lacks the line for {}", task.name());
        }
        assert!(text.contains("  ci "));
        assert!(text.contains("ci --dry-run"));
    }
}
