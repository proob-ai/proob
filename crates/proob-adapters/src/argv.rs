//! The only way an adapter builds a child process: an argv array.
//!
//! Three kinds of argument exist, and the types keep them apart. A literal is
//! text the adapter wrote (`arg`). An owned path is a file or directory the
//! adapter created itself, such as a temporary report path (`own`). A
//! `Checked` value is anything derived from a property, a binding, or a
//! scope: a package name, a test path, a file to mutate. It is constructed
//! only by the functions here, each of which accepts a closed grammar and
//! refuses everything else, so no value reaches a tool that could read it as
//! an option, a parent directory, a glob, or a shell word. Where a tool
//! takes arguments after the end-of-options separator, the checked values
//! go there (`separator` then `tail`).
use proob_core::grammar::glob_ok_exec;
use proob_spawn::{Output, SpawnError};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A value that passed one of the grammars below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked(String);

impl Checked {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A relative path: the scope-glob grammar (never a leading `-` or `/`,
    /// never a `..` segment, a short ASCII alphabet) and none of the glob
    /// metacharacters, so a tool that reads globs reads it as one literal path.
    pub fn path(p: &Path) -> Result<Checked, String> {
        let text = p.to_str().ok_or_else(|| "not UTF-8".to_string())?;
        if !glob_ok_exec(text.as_bytes()) {
            return Err("not a root-relative path over the glob alphabet (no leading `-` or `/`, no `..`)".to_string());
        }
        if text.bytes().any(|b| matches!(b, b'*' | b'?' | b'[' | b']' | b'{' | b'}')) {
            return Err("contains a glob metacharacter".to_string());
        }
        Ok(Checked(text.to_string()))
    }

    /// A Rust path of identifiers, such as `m::my_test`.
    pub fn rust_path(text: &str) -> Result<Checked, String> {
        let ok = !text.is_empty() && text.split("::").all(ident);
        if ok {
            Ok(Checked(text.to_string()))
        } else {
            Err("not a `::`-separated path of Rust identifiers".to_string())
        }
    }

    /// One Rust identifier: a function name for a tool that would read `*`
    /// as a wildcard or `::` as a path (Verus's `--verify-function`).
    pub fn identifier(text: &str) -> Result<Checked, String> {
        if ident(text) { Ok(Checked(text.to_string())) } else { Err("not a Rust identifier".to_string()) }
    }

    /// A cargo package name: ASCII letters, digits, `_`, `-`, not starting with `-`.
    pub fn package(text: &str) -> Result<Checked, String> {
        let ok = text.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
            && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if ok { Ok(Checked(text.to_string())) } else { Err("not a cargo package name".to_string()) }
    }

    /// A nextest binary id: `pkg`, `pkg::test-name`, or `pkg::bin/name`.
    pub fn binary_id(text: &str) -> Result<Checked, String> {
        let (pkg, rest) = text.split_once("::").map_or((text, None), |(p, r)| (p, Some(r)));
        Checked::package(pkg)?;
        let rest_ok = rest.is_none_or(|r| {
            !r.is_empty() && r.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/' | b'.'))
        });
        if rest_ok { Ok(Checked(text.to_string())) } else { Err("not a nextest binary id".to_string()) }
    }

    /// nextest's filterset selecting exactly one test: the operators are
    /// literal text here, the operands are already checked.
    pub fn filterset(binary_id: &Checked, test_path: &Checked) -> Checked {
        Checked(format!("binary_id(={}) & test(={})", binary_id.0, test_path.0))
    }

    /// pytest's node id selecting exactly one module-level test function,
    /// `<file>::<function>`: the operands are already checked (a path and an
    /// identifier), the separator is literal.
    pub fn node_id(file: &Checked, function: &Checked) -> Checked {
        Checked(format!("{}::{}", file.0, function.0))
    }

    /// A regex matching exactly `text` (cargo-mutants' `--re`, against the
    /// names its `--list` printed): every regex metacharacter escaped,
    /// anchored at both ends, so it starts with `^` and is never an option.
    pub fn exact_regex(text: &str) -> Checked {
        let escaped: String = text
            .chars()
            .flat_map(|c| {
                let meta = matches!(
                    c,
                    '\\' | '.'
                        | '+'
                        | '*'
                        | '?'
                        | '('
                        | ')'
                        | '|'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '^'
                        | '$'
                        | '#'
                        | '&'
                        | '-'
                        | '~'
                );
                meta.then_some('\\').into_iter().chain([c])
            })
            .collect();
        Checked(format!("^{escaped}$"))
    }

    /// A mutmut mutant name, `<module>.x_<function>__mutmut_<n>` or with
    /// `ǁ` around a class: ASCII letters, digits, `_`, `.`, and `ǁ`, starting
    /// with a letter, digit, or `_`, so neither click nor `fnmatch` reads it
    /// as anything but one literal name.
    pub fn mutant_name(text: &str) -> Result<Checked, String> {
        let ok = text.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | 'ǁ'));
        if ok { Ok(Checked(text.to_string())) } else { Err("not a mutmut mutant name".to_string()) }
    }

    /// A cargo feature name (`proob_facts::config::feature_name`): one name
    /// or `package/name`, never an option, a list, or a shell word.
    pub fn feature(text: &str) -> Result<Checked, String> {
        if proob_facts::config::feature_name(text) {
            Ok(Checked(text.to_string()))
        } else {
            Err("not a cargo feature name".to_string())
        }
    }

    pub fn number(n: u64) -> Checked {
        Checked(n.to_string())
    }
}

fn ident(s: &str) -> bool {
    let mut bytes = s.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A child process to start: program, argv, working directory, extra
/// environment, and the time it may take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    program: PathBuf,
    args: Vec<OsString>,
    separated: bool,
    cwd: Option<PathBuf>,
    env: Vec<(OsString, OsString)>,
    timeout: Duration,
}

impl Command {
    pub fn new(program: impl Into<PathBuf>, timeout: Duration) -> Command {
        Command { program: program.into(), args: Vec::new(), separated: false, cwd: None, env: Vec::new(), timeout }
    }

    /// A literal the adapter wrote.
    pub fn arg(mut self, literal: &'static str) -> Command {
        self.args.push(OsString::from(literal));
        self
    }

    /// An option with a checked value, before the separator.
    pub fn opt(mut self, flag: &'static str, value: &Checked) -> Command {
        self.args.push(OsString::from(flag));
        self.args.push(OsString::from(&value.0));
        self
    }

    /// An option whose value is a path the adapter owns (a temporary file).
    pub fn own(mut self, flag: &'static str, path: &Path) -> Command {
        self.args.push(OsString::from(flag));
        self.args.push(path.as_os_str().to_os_string());
        self
    }

    /// The end-of-options separator; everything after it is a checked value.
    pub fn separator(mut self) -> Command {
        if !self.separated {
            self.args.push(OsString::from("--"));
            self.separated = true;
        }
        self
    }

    /// A checked value after the separator (which is inserted if absent).
    pub fn tail(self, value: &Checked) -> Command {
        let mut c = self.separator();
        c.args.push(OsString::from(&value.0));
        c
    }

    /// A flag and its checked value after the separator, for a tool that
    /// forwards everything past its own `--` to another tool's options.
    pub fn tail_opt(self, flag: &'static str, value: &Checked) -> Command {
        let mut c = self.separator();
        c.args.push(OsString::from(flag));
        c.args.push(OsString::from(&value.0));
        c
    }

    pub fn cwd(mut self, dir: &Path) -> Command {
        self.cwd = Some(dir.to_path_buf());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Command {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Several variables at once (a tool environment override).
    pub fn envs(mut self, vars: &[(OsString, OsString)]) -> Command {
        self.env.extend(vars.iter().cloned());
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    pub fn environment(&self) -> &[(OsString, OsString)] {
        &self.env
    }

    /// Where `--` sits in the argv, when it does.
    pub fn separator_index(&self) -> Option<usize> {
        self.args.iter().position(|a| a == "--")
    }

    /// Starts the process through proob-spawn.
    pub fn run(&self) -> Result<Output, SpawnError> {
        proob_spawn::run(&self.program, &self.args, self.cwd.as_deref(), &self.env, self.timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_grammars_refuse_steering_values() {
        assert!(Checked::path(Path::new("src/lib.rs")).is_ok());
        assert!(Checked::path(Path::new("-x.rs")).is_err());
        assert!(Checked::path(Path::new("../x.rs")).is_err());
        assert!(Checked::path(Path::new("/etc/passwd")).is_err());
        assert!(Checked::path(Path::new("src/*.rs")).is_err());
        assert!(Checked::rust_path("m::my_test").is_ok());
        assert!(Checked::rust_path("m::1x").is_err());
        assert!(Checked::rust_path("a b").is_err());
        assert!(Checked::identifier("combine_sorted").is_ok());
        assert!(Checked::identifier("combine_sorted*").is_err());
        assert!(Checked::identifier("a::b").is_err());
        assert!(Checked::package("my-crate_2").is_ok());
        assert!(Checked::package("--all").is_err());
        assert!(Checked::binary_id("pkg::bin/name").is_ok());
        assert!(Checked::binary_id("pkg::a b").is_err());
        let file = Checked::path(Path::new("tests/test_props.py")).expect("path");
        let function = Checked::identifier("test_merge").expect("identifier");
        assert_eq!(Checked::node_id(&file, &function).as_str(), "tests/test_props.py::test_merge");
    }

    #[test]
    fn mutant_selectors_are_literal() {
        assert_eq!(
            Checked::exact_regex("src/a.rs:4:5: replace f -> i64 with -1").as_str(),
            "^src/a\\.rs:4:5: replace f \\-> i64 with \\-1$"
        );
        assert_eq!(Checked::exact_regex("-x").as_str(), "^\\-x$", "never an option");
        assert!(Checked::mutant_name("interval.merge.x_merge__mutmut_3").is_ok());
        assert!(Checked::mutant_name("m.xǁCǁg__mutmut_1").is_ok());
        assert!(Checked::mutant_name("--max-children").is_err());
        assert!(Checked::mutant_name("m.x_*").is_err());
    }

    #[test]
    fn tail_values_follow_one_separator() {
        let a = Checked::number(1);
        let c = Command::new("tool", Duration::from_secs(1)).arg("--flag").tail(&a).tail_opt("-E", &a);
        let args: Vec<&str> = c.args().iter().filter_map(|a| a.to_str()).collect();
        assert_eq!(args, vec!["--flag", "--", "1", "-E", "1"]);
        assert_eq!(c.separator_index(), Some(1));
    }
}
