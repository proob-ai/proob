//! The TLC the adapter drives: `java` from `PATH` and the `tla2tools.jar`
//! that `PROOB_TLA2TOOLS` names, a fact of the machine like the memory
//! budget. proob never looks for the jar anywhere else: a guessed path could
//! be any release, or not TLC at all. Both are located before any run; a
//! missing one is an errored check naming how to install it. Every result
//! records `java`, and `tlc` once a run has printed its release.
use crate::ToolVersions;
use crate::argv::Command;
use crate::kani::tools::var;
use crate::rust::tools::find_on_path;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

pub const JAR_ENV: &str = "PROOB_TLA2TOOLS";
/// `tools/tla.toml`: the one place the TLA+ release, its URL, and its SHA-256 are written.
pub const PIN_TEXT: &str = include_str!("../../../../tools/tla.toml");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

/// The embedded pin; `tools/tla.toml` is checked by this module's tests.
pub static PIN: LazyLock<Pin> = LazyLock::new(|| {
    let table: toml::Table = PIN_TEXT.parse().expect("tools/tla.toml is TOML");
    let field = |key: &str| table.get(key).and_then(toml::Value::as_str).expect("tools/tla.toml names it").to_string();
    Pin { version: field("version"), url: field("url"), sha256: field("sha256") }
});
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub java: PathBuf,
    pub jar: PathBuf,
    pub versions: ToolVersions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ready(Tools),
    /// Why a run cannot start, and what was learnt before that.
    Missing {
        reason: String,
        versions: ToolVersions,
    },
}

/// The reason a run cannot start without the jar; it begins with the stable
/// phrase "TLC was not found".
pub fn hint(detail: &str) -> String {
    format!(
        "TLC was not found ({detail}): download tla2tools.jar of TLA+ {v} ({url}, sha256 {sha}) and set {JAR_ENV} to its path; proob never guesses where it is",
        v = PIN.version,
        url = PIN.url,
        sha = PIN.sha256
    )
}

pub fn resolve(tool_env: &[(OsString, OsString)]) -> Resolution {
    let missing = |reason: String| Resolution::Missing { reason, versions: Vec::new() };
    let Some(jar) = var(tool_env, JAR_ENV).filter(|v| !v.is_empty()).map(PathBuf::from) else {
        return missing(hint(&format!("{JAR_ENV} is not set")));
    };
    if !jar.is_file() {
        return missing(hint(&format!("{JAR_ENV} names {}, which is not a file", jar.display())));
    }
    let Some(java) = find_on_path("java", tool_env) else {
        return missing(
            "Java was not found on PATH: TLC runs on a Java runtime (11 or later); install one".to_string(),
        );
    };
    match java_version(&java, tool_env) {
        Ok(v) => Resolution::Ready(Tools { java, jar, versions: vec![("java".to_string(), v)] }),
        Err(e) => {
            missing(format!("`java -version` failed, so TLC cannot run: {e}; install a Java runtime (11 or later)"))
        }
    }
}

/// `java -version` prints to stderr; a launcher with no runtime behind it
/// (macOS's `/usr/bin/java`) exits non-zero.
fn java_version(java: &std::path::Path, tool_env: &[(OsString, OsString)]) -> Result<String, String> {
    let out = Command::new(java, PROBE_TIMEOUT).arg("-version").envs(tool_env).run().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stderr);
    if !out.success() {
        return Err(text.trim().to_string());
    }
    Ok(version_of(&text))
}

/// `openjdk version "25" 2025-09-16` → `25`; else the first line.
pub fn version_of(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    first.split('"').nth(1).unwrap_or(first).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_names_a_release_a_url_and_a_digest() {
        assert!(PIN.url.ends_with(&format!("/v{}/tla2tools.jar", PIN.version)), "{:?}", *PIN);
        assert!(PIN.sha256.len() == 64 && PIN.sha256.bytes().all(|b| b.is_ascii_hexdigit()), "{:?}", *PIN);
        assert!(hint("x").contains(&PIN.sha256));
    }

    #[test]
    fn java_version_lines() {
        assert_eq!(version_of("openjdk version \"25\" 2025-09-16\nOpenJDK Runtime Environment\n"), "25");
        assert_eq!(version_of("java version \"1.8.0_402\"\n"), "1.8.0_402");
    }

    #[test]
    fn no_jar_is_missing_with_how_to_set_it() {
        let unset = [(OsString::from(JAR_ENV), OsString::new())];
        let Resolution::Missing { reason, .. } = resolve(&unset) else { panic!("resolved") };
        assert!(reason.starts_with("TLC was not found (PROOB_TLA2TOOLS is not set)"), "{reason}");
        let dir = tempfile::tempdir().expect("tempdir");
        let absent = [(OsString::from(JAR_ENV), dir.path().join("tla2tools.jar").into_os_string())];
        let Resolution::Missing { reason, .. } = resolve(&absent) else { panic!("resolved") };
        assert!(reason.contains("which is not a file"), "{reason}");
    }

    #[test]
    fn no_java_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let jar = dir.path().join("tla2tools.jar");
        std::fs::write(&jar, b"").expect("write");
        let env = [
            (OsString::from(JAR_ENV), jar.into_os_string()),
            (OsString::from("PATH"), dir.path().join("nothing").into_os_string()),
        ];
        let Resolution::Missing { reason, .. } = resolve(&env) else { panic!("resolved") };
        assert!(reason.starts_with("Java was not found on PATH"), "{reason}");
    }
}
