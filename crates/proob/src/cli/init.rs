//! `proob init`: the files that make a repository checkable, written once.
//!
//! `proob/config.toml` with the detected adapters and `allow_self_signed =
//! true` explained; the two workflows rendered from `templates/workflows/`
//! with the setup steps the adapters need, `fetch-depth: 0`, the two-job
//! shape, and the SHA-pinned action; `.claude/settings.json` with the plugin
//! recommendation merged into whatever is there; and `proob/.cache/` in
//! `.gitignore`. Idempotent: a file that exists is kept as it is, the
//! settings merge adds nothing twice, so a second run changes no byte.
use crate::cli::error::CliError;
use crate::cli::workspace::Workspace;
use proob_facts::config::{self, PolicyTable, defaults};
use proob_facts::paths::{CACHE_DIR, CONFIG, EVIDENCE_WORKFLOW, GATE_WORKFLOW};
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::Path;

const GATE_TEMPLATE: &str = include_str!("../../../../templates/workflows/proob-gate.yml");
const EVIDENCE_TEMPLATE: &str = include_str!("../../../../templates/workflows/proob-evidence.yml");
const SETTINGS_TEMPLATE: &str = include_str!("../../../../templates/settings/claude-settings.json");
pub const SETTINGS: &str = ".claude/settings.json";
pub const GITIGNORE: &str = ".gitignore";

/// The action reference `init` writes, fixed at build time (build.rs).
pub const ACTION_PIN: &str = env!("PROOB_ACTION_PIN");
pub const ACTION_PIN_COMMENT: &str = env!("PROOB_ACTION_PIN_COMMENT");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Detected {
    /// `needs_features`: a crate here visibly needs cargo features to build
    /// its tests (`needs_features`), so the adapter block gets a commented hint.
    Rust {
        toolchain_file: bool,
        needs_features: bool,
    },
    Python {
        pyproject: bool,
        requirements: bool,
    },
    /// `package.json`: the TypeScript adapter arrives after launch.
    TypeScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Written,
    Kept,
    Merged,
    Unchanged,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub detected: Vec<Detected>,
    pub files: Vec<(String, Change)>,
    pub action_pin: String,
    pub action_pinned_by_sha: bool,
}

/// Adapters by the files at the repository root (monorepo detection is deferred).
pub fn detect(root: &Path) -> Vec<Detected> {
    let has = |name: &str| root.join(name).is_file();
    let mut found = Vec::new();
    if has("Cargo.toml") {
        found.push(Detected::Rust {
            toolchain_file: has("rust-toolchain.toml") || has("rust-toolchain"),
            needs_features: needs_features(root),
        });
    }
    let (pyproject, requirements) = (has("pyproject.toml") || has("setup.py"), has("requirements.txt"));
    if pyproject || requirements {
        found.push(Detected::Python { pyproject, requirements });
    }
    if has("package.json") {
        found.push(Detected::TypeScript);
    }
    found
}

/// Whether the root crate or a crate one directory down (a workspace member)
/// has a `build.rs` whose `compile_error!` mentions features, or a `[[test]]`
/// target with `required-features`. A text scan, so only the obvious cases.
pub fn needs_features(root: &Path) -> bool {
    let members = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("Cargo.toml").is_file());
    std::iter::once(root.to_path_buf()).chain(members).any(|dir| {
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
        build_needs_features(&read("build.rs")) || tests_require_features(&read("Cargo.toml"))
    })
}

/// A `build.rs` that refuses to build without a feature. Pure.
pub fn build_needs_features(build_rs: &str) -> bool {
    build_rs
        .split("compile_error!")
        .skip(1)
        .any(|after| after.split(';').next().is_some_and(|call| call.contains("feature")))
}

/// A manifest with a `[[test]]` table that sets `required-features`. Pure.
pub fn tests_require_features(manifest: &str) -> bool {
    manifest
        .lines()
        .map(str::trim)
        .scan(false, |in_test, line| {
            *in_test = if line.starts_with('[') { line.starts_with("[[test]]") } else { *in_test };
            Some(*in_test && line.starts_with("required-features"))
        })
        .any(|hit| hit)
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// The commented line `init` adds to a rust adapter block that needs features.
pub const FEATURES_HINT: &str = "# This crate's tests look like they need cargo features (build.rs compile_error!\n\
     # or [[test]] required-features); set one: features = [\"name\"], all_features = true, no_default_features = true\n";

fn adapter_block(d: Detected) -> String {
    match d {
        Detected::Rust { needs_features, .. } => format!(
            "[[adapters]]\nroot = \".\"\nlanguage = \"rust\"\nrunner = \"nextest\"\nmutation_tool = \"cargo-mutants\"\n{}",
            if needs_features { FEATURES_HINT } else { "" }
        ),
        Detected::Python { .. } => "[[adapters]]\nroot = \".\"\nlanguage = \"python\"\nrunner = \"pytest\"\nmutation_tool = \"mutmut\"\n".to_string(),
        Detected::TypeScript => "# package.json detected: the TypeScript adapter (fast-check, StrykerJS) arrives\n# after launch; until then TypeScript properties render Not checked (no adapter).\n".to_string(),
    }
}

fn outcome_name(o: config::OutcomeName) -> &'static str {
    match o {
        config::OutcomeName::Pass => "pass",
        config::OutcomeName::Warn => "warn",
        config::OutcomeName::Block => "block",
    }
}

fn policy_block(p: PolicyTable) -> String {
    format!(
        "[policy]\nholds = \"{}\"\nchecked = \"{}\"\nproposed = \"{}\"\nwaived = \"{}\"\nnot_checked_within_grace = \"{}\"\nnot_checked_after_grace = \"{}\"\nre_approve = \"{}\"\nbroken_affected = \"{}\"\nbroken_unaffected = \"{}\"\n",
        outcome_name(p.holds),
        outcome_name(p.checked),
        outcome_name(p.proposed),
        outcome_name(p.waived),
        outcome_name(p.not_checked_within_grace),
        outcome_name(p.not_checked_after_grace),
        outcome_name(p.re_approve),
        outcome_name(p.broken_affected),
        outcome_name(p.broken_unaffected),
    )
}

/// The config text: the built-in defaults, commented, with the detected adapters.
pub fn config_text(detected: &[Detected]) -> String {
    let adapters: String = detected.iter().map(|d| format!("\n{}", adapter_block(*d))).collect();
    let adapters = if detected.is_empty() {
        "\n# No adapter detected (Cargo.toml, pyproject.toml, setup.py, requirements.txt).\n# Properties render Not checked (no adapter) until one is listed here.\n".to_string()
    } else {
        adapters
    };
    format!(
        "# proob/config.toml, written by `proob init`. This file is governed: it takes\n\
         # effect only once the pull request that introduced it merges with an eligible\n\
         # review; until then the built-in defaults (these values) apply.\n\
         schema_version = {}\n\
         \n\
         # A merge with no eligible review (a single-maintainer repository, or the author\n\
         # merging their own change) is a weaker signature. With `true` such content\n\
         # renders Checked (self-signed) or Not checked (self-signed): never Holds, and\n\
         # never Proposed. Set it to false once a second maintainer with write access\n\
         # reviews, and the same content reads Proposed until they approve it.\n\
         # A property backed by a test renders Checked (tested) at best: a test samples\n\
         # its inputs, so only a proof or a model reaches Holds.\n\
         allow_self_signed = {}\n\
         \n\
         # Days after approval during which an unchecked property stays silent.\n\
         grace_days = {}\n\
         # Age beyond which score evidence (the nightly mutation run) is stale.\n\
         freshness_days = {}\n\
         # Mutants a bound check must kill to count as able to fail; at least 1.\n\
         kill_floor = {}\n\
         # Most proposals declare writes per pull request.\n\
         proposal_cap = {}\n\
         # Scope commits since approval above which guard proposes re-affirmation.\n\
         drift_threshold = {}\n\
         # GitHub calls per run; files beyond it render Unverified (rate limited).\n\
         call_budget = {}\n\
         # How much of a counterexample evidence carries: \"full\" or \"seed_only\".\n\
         witness = \"full\"\n\
         # The longest a waiver may run, in days.\n\
         waiver_max_days = {}\n\
         # Cases per property-based test in the scheduled evidence run; the gate and\n\
         # the push run use the adapter's own default (256 for proptest).\n\
         scheduled_cases = {}\n\
         # Changed source files that need no property, as scope globs (generated or\n\
         # vendored code). `proob check` lists other new code that no property covers;\n\
         # proob/, .github/, tests/fixtures/, and examples/ are always left out.\n\
         # uncovered_ignore = [\"src/generated/**\"]\n\
         {adapters}\n\
         # The gate policy, total over the verdicts. Only broken_affected may block: a\n\
         # pull request is blocked only by a Broken property it affects.\n\
         {}",
        defaults::SCHEMA_VERSION,
        defaults::ALLOW_SELF_SIGNED,
        defaults::GRACE_DAYS,
        defaults::FRESHNESS_DAYS,
        defaults::KILL_FLOOR,
        defaults::PROPOSAL_CAP,
        defaults::DRIFT_THRESHOLD,
        defaults::CALL_BUDGET,
        defaults::WAIVER_MAX_DAYS,
        defaults::SCHEDULED_CASES,
        policy_block(PolicyTable::DEFAULTS),
    )
}

// ---------------------------------------------------------------------------
// Workflows
// ---------------------------------------------------------------------------

const TAIKI_INSTALL_ACTION: &str = "taiki-e/install-action@1168f6810a3cb30a3514bf5756a3b7f0422c6df8 # v2.87.28";
const RUST_TOOLCHAIN_ACTION: &str = "dtolnay/rust-toolchain@683b588c487a69d01f6c6ddf40f036ff682cc2c8 # v1";
const SETUP_PYTHON_ACTION: &str = "actions/setup-python@6d4fd22b20465ccc0f7354476588e8df7c9ca0e0 # v7.1.0";

/// The Python adapter's tools as pip requirement arguments, from its one pin file.
fn python_pins() -> String {
    proob_adapters::python::tools::pins()
        .iter()
        .map(|(name, version)| format!("\"{name}=={version}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The setup steps one adapter needs, as workflow step lines (six-space indent).
fn setup_steps(d: Detected) -> Vec<String> {
    match d {
        Detected::Rust { toolchain_file: true, .. } => vec![
            "      - name: Install the Rust toolchain pinned in rust-toolchain.toml".into(),
            "        run: rustup toolchain install && rustup show active-toolchain".into(),
            format!("      - uses: {TAIKI_INSTALL_ACTION}"),
            "        with:".into(),
            "          tool: cargo-nextest@0.9.150,cargo-mutants@27.1.0".into(),
        ],
        Detected::Rust { toolchain_file: false, .. } => vec![
            format!("      - uses: {RUST_TOOLCHAIN_ACTION}"),
            "        with:".into(),
            "          toolchain: stable".into(),
            format!("      - uses: {TAIKI_INSTALL_ACTION}"),
            "        with:".into(),
            "          tool: cargo-nextest@0.9.150,cargo-mutants@27.1.0".into(),
        ],
        Detected::Python { pyproject, requirements } => {
            let install = match (pyproject, requirements) {
                (true, true) => "python -m pip install -e . -r requirements.txt",
                (true, false) => "python -m pip install -e .",
                _ => "python -m pip install -r requirements.txt",
            };
            vec![
                format!("      - uses: {SETUP_PYTHON_ACTION}"),
                "        with:".into(),
                "          python-version: \"3.12\"".into(),
                "      - name: Install the project and the Python adapter's pinned tools".into(),
                "        # The pins are the adapter's (tools/python-tools.txt in the proob repository); mutmut must match exactly.".into(),
                format!("        run: python -m pip install --upgrade pip && {install} && python -m pip install {}", python_pins()),
            ]
        }
        Detected::TypeScript => vec![
            "      # package.json detected: the TypeScript adapter arrives after launch; no setup rendered.".into(),
        ],
    }
}

fn all_setup_steps(detected: &[Detected]) -> String {
    let lines: Vec<String> = if detected.is_empty() {
        vec!["      # No adapter detected: nothing to set up until proob/config.toml lists one.".into()]
    } else {
        detected.iter().flat_map(|d| setup_steps(*d)).collect()
    };
    lines.join("\n")
}

/// A template with its placeholders filled; no placeholder survives.
pub fn render_workflow(template: &str, default_branch: &str, detected: &[Detected]) -> String {
    template
        .replace("@@SETUP_STEPS@@", &all_setup_steps(detected))
        .replace("@@ACTION_PIN@@", ACTION_PIN)
        .replace("@@ACTION_PIN_COMMENT@@", ACTION_PIN_COMMENT)
        .replace("@@DEFAULT_BRANCH@@", default_branch)
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

fn recommendation() -> Result<Map<String, Value>, CliError> {
    match serde_json::from_str(SETTINGS_TEMPLATE) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(CliError::failed("the embedded settings template is not a JSON object")),
    }
}

/// Objects merge key by key (ours fill in, theirs win on conflict for scalars
/// we do not own); everything else is left as found.
fn merge_into(existing: &mut Map<String, Value>, ours: Map<String, Value>) {
    for (key, value) in ours {
        match (existing.get_mut(&key), value) {
            (Some(Value::Object(theirs)), Value::Object(mine)) => merge_into(theirs, mine),
            (Some(_), _) => {}
            (None, value) => {
                existing.insert(key, value);
            }
        }
    }
}

/// The settings file after the merge, and whether anything changed.
pub fn merged_settings(existing: Option<&str>) -> Result<(String, Change), CliError> {
    let mut map = match existing {
        None => Map::new(),
        Some(text) => match serde_json::from_str(text) {
            Ok(Value::Object(map)) => map,
            Ok(_) => return Err(CliError::invalid(SETTINGS, "not a JSON object; proob will not overwrite it")),
            Err(e) => {
                return Err(CliError::invalid(SETTINGS, format!("not valid JSON ({e}); proob will not overwrite it")));
            }
        },
    };
    let before = Value::Object(map.clone());
    merge_into(&mut map, recommendation()?);
    let after = Value::Object(map);
    let text = serde_json::to_string_pretty(&after).map_err(CliError::failed)? + "\n";
    let change = match (existing, before == after) {
        (None, _) => Change::Written,
        (Some(old), true) if old == text => Change::Unchanged,
        (Some(_), true) => Change::Merged,
        (Some(_), false) => Change::Merged,
    };
    Ok((text, change))
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

fn write(ws: &Workspace, path: &str, text: &str) -> Result<(), CliError> {
    let full = ws.root.join(path);
    if let Some(dir) = full.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CliError::failed(format!("cannot create {}: {e}", dir.display())))?;
    }
    std::fs::write(&full, text).map_err(|e| CliError::failed(format!("cannot write {path}: {e}")))
}

/// Writes `path` unless it exists; an existing file is the team's and is kept.
fn write_new(ws: &Workspace, path: &str, text: &str) -> Result<Change, CliError> {
    match ws.read_optional(path)? {
        Some(_) => Ok(Change::Kept),
        None => write(ws, path, text).map(|()| Change::Written),
    }
}

/// `.gitignore` with the cache line added; `None` when it is already there.
pub fn gitignore_text(existing: Option<&str>) -> Option<String> {
    let line = format!("{CACHE_DIR}/");
    let present = existing.is_some_and(|t| t.lines().any(|l| l.trim() == line || l.trim() == CACHE_DIR));
    if present {
        return None;
    }
    let mut text = existing.unwrap_or_default().to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("# proob's fact cache: fetched GitHub payloads, never content to review.\n");
    text.push_str(&line);
    text.push('\n');
    Some(text)
}

fn gitignore(ws: &Workspace) -> Result<Change, CliError> {
    match gitignore_text(ws.read_optional(GITIGNORE)?.as_deref()) {
        None => Ok(Change::Unchanged),
        Some(text) => write(ws, GITIGNORE, &text).map(|()| Change::Merged),
    }
}

pub fn run(ws: &Workspace, default_branch: &str) -> Result<Outcome, CliError> {
    let detected = detect(&ws.root);
    let config = config_text(&detected);
    config::parse_config(&config)
        .map_err(|e| CliError::failed(format!("the rendered config does not parse: {e} (report this)")))?;
    let (settings, settings_change) = merged_settings(ws.read_optional(SETTINGS)?.as_deref())?;
    let files = vec![
        (CONFIG.to_string(), write_new(ws, CONFIG, &config)?),
        (
            GATE_WORKFLOW.to_string(),
            write_new(ws, GATE_WORKFLOW, &render_workflow(GATE_TEMPLATE, default_branch, &detected))?,
        ),
        (
            EVIDENCE_WORKFLOW.to_string(),
            write_new(ws, EVIDENCE_WORKFLOW, &render_workflow(EVIDENCE_TEMPLATE, default_branch, &detected))?,
        ),
        (SETTINGS.to_string(), write_settings(ws, &settings, settings_change)?),
        (GITIGNORE.to_string(), gitignore(ws)?),
    ];
    Ok(Outcome {
        detected,
        files,
        action_pin: format!("{ACTION_PIN} {ACTION_PIN_COMMENT}"),
        action_pinned_by_sha: ACTION_PIN_COMMENT.starts_with("# v"),
    })
}

fn write_settings(ws: &Workspace, text: &str, change: Change) -> Result<Change, CliError> {
    if change != Change::Unchanged {
        write(ws, SETTINGS, text)?;
    }
    Ok(change)
}

pub fn text(o: &Outcome) -> String {
    let mut out = String::new();
    let detected: Vec<&str> = o
        .detected
        .iter()
        .map(|d| match d {
            Detected::Rust { .. } => "rust",
            Detected::Python { .. } => "python",
            Detected::TypeScript => "typescript (adapter after launch)",
        })
        .collect();
    out.push_str(&format!(
        "detected adapters: {}\n",
        if detected.is_empty() { "none".to_string() } else { detected.join(", ") }
    ));
    for (path, change) in &o.files {
        let word = match change {
            Change::Written => "written",
            Change::Kept => "kept (exists)",
            Change::Merged => "merged",
            Change::Unchanged => "unchanged",
        };
        out.push_str(&format!("{word:>14}  {path}\n"));
    }
    out.push_str(&format!("action pin: {}\n", o.action_pin));
    if !o.action_pinned_by_sha {
        out.push_str("note: this proob was built without a release commit; the workflows pin a version tag. Replace it with the release's commit SHA before relying on the gate.\n");
    }
    out.push_str(
        "next: review proob/config.toml, commit, and open the pull request; the gate takes effect after it merges.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_need_is_read_from_compile_error_and_required_features_only() {
        assert!(build_needs_features("compile_error!(\"enable a DB backend with --features sqlite\");"));
        assert!(
            !build_needs_features("compile_error!(\"unsupported target\"); // features: none"),
            "the call's own text"
        );
        assert!(!build_needs_features("fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); } // feature"));
        assert!(tests_require_features("[[test]]\nname = \"t\"\nrequired-features = [\"std\"]\n"));
        assert!(
            !tests_require_features("[[bin]]\nrequired-features = [\"cli\"]\n[[test]]\nname = \"t\"\n"),
            "only test targets"
        );
        assert!(!tests_require_features("[features]\ndefault = [\"std\"]\n"));
    }
}
