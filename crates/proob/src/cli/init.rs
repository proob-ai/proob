//! `proob init`: the files that make a repository checkable, written once.
//!
//! `proob/config.toml` with the detected adapters and `allow_self_signed =
//! true` explained; the two workflows rendered from `templates/workflows/`
//! with the setup steps the adapters need, `fetch-depth: 0`, the two-job
//! shape, and the SHA-pinned action; `.claude/settings.json` with the plugin
//! recommendation merged into whatever is there; and `proob/.cache/` in
//! `.gitignore`. Idempotent: a file that exists is kept as it is, the
//! settings merge adds nothing twice, so a second run changes no byte.
//!
//! With `--project <language>:<dir>` (repeatable): a Rust or Python project
//! below the repository root gets its adapter block and its setup steps too.
//! `init` looks for marker files at the root only; where the projects of a
//! monorepo are is for the caller to say (an agent reads the repository), and
//! the runner, the mutation tool, and the install then follow from what is at
//! the named root. A config that already exists is kept; the block it lacks is
//! printed.
//!
//! With `--kani` (a Rust repository only): the Kani adapter block, a pinned,
//! cached Kani install in both workflows' setup steps, and the build probe,
//! whose result is kept in `proob/.cache/kani-probe.toml`. A config or
//! workflow that already exists is still kept; the block and steps it lacks
//! are printed instead, with the oldest proob that reads a Kani record.
//!
//! With `--tla`: the TLA+ adapter block (TLC, spec mutants) and, in both
//! workflows' setup steps, the pinned tla2tools.jar checked against its
//! SHA-256 with `PROOB_TLA2TOOLS` set to it; a kept workflow without them is
//! given the steps to add.
use crate::cli::error::CliError;
use crate::cli::workspace::Workspace;
use proob_adapters::argv::Checked;
use proob_adapters::kani::{self, KaniAdapter};
use proob_adapters::tla;
use proob_facts::config::{self, Language, PolicyTable, defaults};
use proob_facts::paths::{CACHE_DIR, CONFIG, EVIDENCE_WORKFLOW, GATE_WORKFLOW, KANI_PROBE};
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

/// A detected project and its root. The ones at the repository root `init`
/// finds itself; one below it is named by the caller (`--project`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Project {
    /// Relative to the repository, in `Checked::path`'s grammar; `.` is the root.
    pub root: String,
    pub detected: Detected,
}

impl Project {
    pub fn at_root(detected: Detected) -> Project {
        Project { root: ".".to_string(), detected }
    }

    /// `<language>:<dir>` read against the repository at `repo`. Rust needs
    /// its `Cargo.toml` there; Python needs no marker file, since the caller
    /// has said what the directory is.
    pub fn named(repo: &Path, spec: &str) -> Result<Project, CliError> {
        let bad = |why: String| {
            CliError::Usage(format!(
                "--project {spec}: {why}; name a project as rust:<dir> or python:<dir>, the directory relative to the repository root"
            ))
        };
        let (language, dir) = spec.split_once(':').ok_or_else(|| bad("there is no `:`".into()))?;
        Checked::path(Path::new(dir.trim_end_matches('/'))).map_err(|rule| bad(format!("`{dir}`: {rule}")))?;
        let root = normal(dir);
        // Checked again as it will be written: `./-x` is `-x` once normalised.
        Checked::path(Path::new(&root)).map_err(|rule| bad(format!("`{dir}`: {rule}")))?;
        if root.starts_with('~') {
            return Err(bad(format!("`{dir}`: a leading `~` is the shell's home directory")));
        }
        let at = repo.join(&root);
        // By its exact name: a symlink or another spelling is a second name for a directory, or a way out.
        if !at.is_dir() || at.canonicalize().ok() != repo.canonicalize().ok().map(|r| r.join(&root)) {
            return Err(bad(format!("`{root}` is not a committed directory of the repository")));
        }
        let mut found = detect(&at).into_iter();
        let detected = match language {
            "rust" => found
                .find(|d| matches!(d, Detected::Rust { .. }))
                .ok_or_else(|| bad(format!("there is no Cargo.toml in `{root}`")))?,
            "python" => found
                .find(|d| matches!(d, Detected::Python { .. }))
                .unwrap_or(Detected::Python { pyproject: false, requirements: false }),
            other => return Err(bad(format!("proob has no adapter for `{other}`"))),
        };
        Ok(Project { root, detected })
    }

    /// The project of a config's Rust or Python adapter; none for another
    /// language, or when its directory is not there.
    fn listed(repo: &Path, a: &config::Adapter) -> Option<Project> {
        let language = match a.language {
            Language::Rust => "rust",
            Language::Python => "python",
            _ => return None,
        };
        Project::named(repo, &format!("{language}:{}", a.root)).ok()
    }

    /// Whether `a` is this project's adapter: the same root and language.
    pub fn is_adapter(&self, a: &config::Adapter) -> bool {
        let language = match self.detected {
            Detected::Rust { .. } => Some(Language::Rust),
            Detected::Python { .. } => Some(Language::Python),
            Detected::TypeScript => None,
        };
        language == Some(a.language) && normal(&a.root) == self.root
    }

    /// `file` at the project's root, as a step run from the repository root names it.
    fn path(&self, file: &str) -> String {
        if self.root == "." { file.to_string() } else { format!("{}/{file}", self.root) }
    }
}

/// `dir` as one spelling: no `./`, no doubled or trailing `/`, no `.` segment; `.` is the root.
fn normal(dir: &str) -> String {
    let parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    if parts.is_empty() { ".".to_string() } else { parts.join("/") }
}

/// The projects to set up: the root's own, then each named one not already among them.
fn with_named(detected: &[Detected], named: &[Project]) -> Vec<Project> {
    let same = |a: &Project, b: &Project| {
        a.root == b.root && std::mem::discriminant(&a.detected) == std::mem::discriminant(&b.detected)
    };
    detected.iter().copied().map(Project::at_root).chain(named.iter().cloned()).fold(Vec::new(), |mut all, p| {
        if !all.iter().any(|q| same(q, &p)) {
            all.push(p);
        }
        all
    })
}

/// What `init` sets up beyond what it detects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// `--kani`: the Kani adapter, its install in both workflows, and the probe.
    pub kani: Option<KaniSetup>,
    /// `--tla`: the TLA+ adapter.
    pub tla: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KaniSetup {
    /// The adapter's `max_mutants` when not its default (the shadow's sample).
    pub max_mutants: Option<u32>,
}

/// The oldest proob that reads a Kani `verified` record (one with `checks`):
/// an older one fails to read every evidence record of the repository.
pub const KANI_READER_VERSION: &str = env!("CARGO_PKG_VERSION");

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
    /// The projects named with `--project`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<Project>,
    /// The kept files that lack a named project: each path and the text to add.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub projects_to_add: Vec<(String, String)>,
    pub files: Vec<(String, Change)>,
    pub action_pin: String,
    pub action_pinned_by_sha: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kani: Option<KaniOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tla: Option<TlaOutcome>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TlaOutcome {
    /// A config that existed and was kept without the TLA+ adapter: its path and the block to add.
    pub to_add: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KaniOutcome {
    /// `passed` or `failed: <reason>`, as kept in `proob/.cache/kani-probe.toml`.
    pub probe: String,
    /// Files that existed and were kept without Kani: each path and the text to add.
    pub to_add: Vec<(String, String)>,
    /// `KANI_READER_VERSION`: the gate, the evidence workflow, and the local shim need it or later.
    pub min_proob_version: String,
}

/// Adapters by the files at `root`: the repository's, or a named project's.
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

/// The commented line `init` adds to a rust or kani adapter block that needs features.
pub const FEATURES_HINT: &str = "# This crate's tests look like they need cargo features (build.rs compile_error!\n\
     # or [[test]] required-features); set one: features = [\"name\"], all_features = true, no_default_features = true\n";

fn adapter_block(p: &Project) -> String {
    let root = &p.root;
    match p.detected {
        Detected::Rust { needs_features, .. } => format!(
            "[[adapters]]\nroot = \"{root}\"\nlanguage = \"rust\"\nrunner = \"nextest\"\nmutation_tool = \"cargo-mutants\"\n{}",
            if needs_features { FEATURES_HINT } else { "" }
        ),
        Detected::Python { .. } => format!("[[adapters]]\nroot = \"{root}\"\nlanguage = \"python\"\nrunner = \"pytest\"\nmutation_tool = \"mutmut\"\n"),
        Detected::TypeScript => "# package.json detected: the TypeScript adapter (fast-check, StrykerJS) arrives\n# after launch; until then TypeScript properties render Not checked (no adapter).\n".to_string(),
    }
}

/// The Kani adapter block; `max_mutants` only when it is not the default,
/// and the features hint when a crate visibly needs them (`needs_features`).
pub fn kani_block(k: KaniSetup, needs_features: bool) -> String {
    let sample = k.max_mutants.map_or(String::new(), |n| {
        format!(
            "# A smaller mutant sample than the default ({}), for a first look.\nmax_mutants = {n}\n",
            kani::DEFAULT_MAX_MUTANTS
        )
    });
    format!(
        "[[adapters]]\n# Kani harnesses (#[kani::proof]) as proofs, each verified within its bounds.\nroot = \".\"\nlanguage = \"kani\"\nrunner = \"{}\"\nmutation_tool = \"{}\"\n{sample}{}",
        kani::RUNNER,
        kani::MUTATION_TOOL,
        if needs_features { FEATURES_HINT } else { "" }
    )
}

/// The TLA+ adapter block.
pub fn tla_block() -> String {
    format!(
        "[[adapters]]\n# TLA+ invariants and properties as models, each checked by TLC over the spec's .cfg constants.\n# TLC runs on Java with PROOB_TLA2TOOLS set to the path of a tla2tools.jar.\nroot = \".\"\nlanguage = \"tla\"\nrunner = \"{}\"\nmutation_tool = \"{}\"\n",
        tla::RUNNER,
        tla::MUTATION_TOOL
    )
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
        "[policy]\nholds = \"{}\"\n\
         # A Checked property with a defect pending: strength pending or self-signed.\n\
         checked = \"{}\"\n\
         # A test-backed property that passes renders Checked (tested); this row gates it.\n\
         tested = \"{}\"\nproposed = \"{}\"\nwaived = \"{}\"\nnot_checked_within_grace = \"{}\"\nnot_checked_after_grace = \"{}\"\nre_approve = \"{}\"\nbroken_affected = \"{}\"\nbroken_unaffected = \"{}\"\n",
        outcome_name(p.holds),
        outcome_name(p.checked),
        outcome_name(p.tested),
        outcome_name(p.proposed),
        outcome_name(p.waived),
        outcome_name(p.not_checked_within_grace),
        outcome_name(p.not_checked_after_grace),
        outcome_name(p.re_approve),
        outcome_name(p.broken_affected),
        outcome_name(p.broken_unaffected),
    )
}

/// The config text: the built-in defaults, commented, with the projects'
/// adapters and, when asked, Kani's.
pub fn config_text(projects: &[Project], opts: Options) -> String {
    let needs =
        projects.iter().any(|p| p.root == "." && matches!(p.detected, Detected::Rust { needs_features: true, .. }));
    let adapters: String = projects
        .iter()
        .map(adapter_block)
        .chain(opts.kani.map(|k| kani_block(k, needs)))
        .chain(opts.tla.then(tla_block))
        .map(|b| format!("\n{b}"))
        .collect();
    let adapters = if adapters.is_empty() {
        "\n# No adapter detected (Cargo.toml, pyproject.toml, setup.py, requirements.txt at the\n# repository root). For a project in a subdirectory, `proob init --project python:<dir>`\n# (or rust:<dir>) prints its block. Properties render Not checked (no adapter) until\n# one is listed here.\n".to_string()
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
const CACHE_ACTION: &str = "actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0";

/// The Python adapter's tools as pip requirement arguments, from its one pin file.
fn python_pins() -> String {
    proob_adapters::python::tools::pins()
        .iter()
        .map(|(name, version)| format!("\"{name}=={version}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The setup steps one project's adapter needs, as workflow step lines
/// (six-space indent). Steps run from the repository root, so a project
/// below it is installed by its path.
fn setup_steps(p: &Project) -> Vec<String> {
    match p.detected {
        Detected::Rust { toolchain_file: true, .. } => [
            format!("      - name: Install the Rust toolchain pinned in {}", p.path("rust-toolchain.toml")),
            "        run: rustup toolchain install && rustup show active-toolchain".into(),
        ]
        .into_iter()
        .chain((p.root != ".").then(|| format!("        working-directory: \"{}\"", p.root)))
        .chain([
            format!("      - uses: {TAIKI_INSTALL_ACTION}"),
            "        with:".into(),
            "          tool: cargo-nextest@0.9.150,cargo-mutants@27.1.0".into(),
        ])
        .collect(),
        Detected::Rust { toolchain_file: false, .. } => vec![
            format!("      - uses: {RUST_TOOLCHAIN_ACTION}"),
            "        with:".into(),
            "          toolchain: stable".into(),
            format!("      - uses: {TAIKI_INSTALL_ACTION}"),
            "        with:".into(),
            "          tool: cargo-nextest@0.9.150,cargo-mutants@27.1.0".into(),
        ],
        Detected::Python { pyproject, requirements } => {
            // Neither file (a named project without one): only the adapter's tools.
            let project: Vec<String> = pyproject
                .then(|| format!("-e {}", p.root))
                .into_iter()
                .chain(requirements.then(|| format!("-r {}", p.path("requirements.txt"))))
                .collect();
            let install = if project.is_empty() {
                String::new()
            } else {
                format!("python -m pip install {} && ", project.join(" "))
            };
            vec![
                format!("      - uses: {SETUP_PYTHON_ACTION}"),
                "        with:".into(),
                "          python-version: \"3.12\"".into(),
                "      - name: Install the project and the Python adapter's pinned tools".into(),
                "        # The pins are the adapter's (tools/python-tools.txt in the proob repository); mutmut must match exactly.".into(),
                format!("        run: python -m pip install --upgrade pip && {install}python -m pip install {}", python_pins()),
            ]
        }
        Detected::TypeScript => vec![
            "      # package.json detected: the TypeScript adapter arrives after launch; no setup rendered.".into(),
        ],
    }
}

/// Kani's install, at the version the adapter drives, cached on that pin with
/// its bundle and nightly; after the Rust steps, which bring cargo.
pub fn kani_steps() -> Vec<String> {
    let pin = &*kani::tools::PIN;
    vec![
        format!("      - name: Cache Kani {} and its nightly", pin.version),
        "        id: kani-cache".into(),
        format!("        uses: {CACHE_ACTION}"),
        "        with:".into(),
        "          path: |".into(),
        "            ~/.cargo/bin/cargo-kani".into(),
        "            ~/.cargo/bin/kani".into(),
        format!("            ~/{}", kani::tools::DEFAULT_HOME),
        format!("            ~/.rustup/toolchains/{}-*", pin.nightly),
        format!("          key: kani-${{{{ runner.os }}}}-${{{{ runner.arch }}}}-{}", pin.version),
        format!("      - name: Install Kani {} (the release this proob drives; it refuses any other)", pin.version),
        "        if: steps.kani-cache.outputs.cache-hit != 'true'".into(),
        format!("        run: cargo install --locked kani-verifier --version {} && cargo kani setup", pin.version),
    ]
}

/// TLC as the workflows install it: the pinned jar, refused unless its
/// SHA-256 matches, on the runner's Java, named by `PROOB_TLA2TOOLS`.
pub fn tla_steps() -> Vec<String> {
    let pin = &*tla::tools::PIN;
    vec![
        format!("      - name: Install TLC (TLA+ {}, the release this proob is tested with)", pin.version),
        "        run: |".into(),
        "          jar=\"$RUNNER_TEMP/tla2tools.jar\"".into(),
        format!("          curl -fsSL --retry 3 -o \"$jar\" {}", pin.url),
        format!("          echo \"{}  $jar\" | sha256sum -c -", pin.sha256),
        "          java -version".into(),
        format!("          echo \"{}=$jar\" >> \"$GITHUB_ENV\"", tla::tools::JAR_ENV),
    ]
}

fn all_setup_steps(projects: &[Project], opts: Options) -> String {
    let tla = opts.tla.then(tla_steps).into_iter().flatten();
    let lines: Vec<String> = if projects.is_empty() && !opts.tla {
        vec!["      # No adapter detected: nothing to set up until proob/config.toml lists one.".into()]
    } else {
        projects
            .iter()
            .flat_map(setup_steps)
            .chain(opts.kani.into_iter().flat_map(|_| kani_steps()))
            .chain(tla)
            .collect()
    };
    lines.join("\n")
}

/// A template with its placeholders filled; no placeholder survives.
pub fn render_workflow(template: &str, default_branch: &str, projects: &[Project], opts: Options) -> String {
    template
        .replace("@@SETUP_STEPS@@", &all_setup_steps(projects, opts))
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

/// `named`: the projects below the root that the caller names (`--project`).
pub fn run(ws: &Workspace, default_branch: &str, opts: Options, named: &[Project]) -> Result<Outcome, CliError> {
    let detected = detect(&ws.root);
    if opts.kani.is_some() && !detected.iter().any(|d| matches!(d, Detected::Rust { .. })) {
        return Err(CliError::Usage("--kani needs a Rust crate: there is no Cargo.toml at the repository root".into()));
    }
    let projects = with_named(&detected, named);
    let config = config_text(&projects, opts);
    config::parse_config(&config)
        .map_err(|e| CliError::failed(format!("the rendered config does not parse: {e} (report this)")))?;
    let (settings, settings_change) = merged_settings(ws.read_optional(SETTINGS)?.as_deref())?;
    // Read before writing: what a kept file lacks is printed, not written.
    let to_add = match opts.kani {
        Some(k) => kani_to_add(ws, k)?,
        None => Vec::new(),
    };
    let tla = if opts.tla { Some(TlaOutcome { to_add: tla_to_add(ws)? }) } else { None };
    let projects_to_add = projects_to_add(ws, named)?;
    let files = vec![
        (CONFIG.to_string(), write_new(ws, CONFIG, &config)?),
        (
            GATE_WORKFLOW.to_string(),
            write_new(ws, GATE_WORKFLOW, &render_workflow(GATE_TEMPLATE, default_branch, &projects, opts))?,
        ),
        (
            EVIDENCE_WORKFLOW.to_string(),
            write_new(ws, EVIDENCE_WORKFLOW, &render_workflow(EVIDENCE_TEMPLATE, default_branch, &projects, opts))?,
        ),
        (SETTINGS.to_string(), write_settings(ws, &settings, settings_change)?),
        (GITIGNORE.to_string(), gitignore(ws)?),
    ];
    let kani = match opts.kani {
        Some(_) => Some(KaniOutcome { probe: probe(ws)?, to_add, min_proob_version: KANI_READER_VERSION.to_string() }),
        None => None,
    };
    Ok(Outcome {
        detected,
        projects: named.to_vec(),
        projects_to_add,
        files,
        action_pin: format!("{ACTION_PIN} {ACTION_PIN_COMMENT}"),
        action_pinned_by_sha: ACTION_PIN_COMMENT.starts_with("# v"),
        kani,
        tla,
    })
}

/// The adapter block of each named project that the existing config has no
/// adapter for; none when there is no config yet (it is about to be written
/// with them).
pub fn project_blocks(ws: &Workspace, named: &[Project]) -> Result<Vec<String>, CliError> {
    let Some(text) = ws.read_optional(CONFIG)? else { return Ok(Vec::new()) };
    let listed = config::parse_config(&text).map(|c| c.adapters).unwrap_or_default();
    let named = with_named(&[], named);
    Ok(named.iter().filter(|p| !listed.iter().any(|a| p.is_adapter(a))).map(adapter_block).collect())
}

/// The existing files that lack a named project, each with the exact text to
/// add: its adapter block to a config without that adapter, its setup steps
/// to a workflow without them.
fn projects_to_add(ws: &Workspace, named: &[Project]) -> Result<Vec<(String, String)>, CliError> {
    let steps: Vec<String> = with_named(&[], named).iter().map(|p| setup_steps(p).join("\n")).collect();
    let mut out: Vec<(String, String)> =
        project_blocks(ws, named)?.into_iter().map(|b| (CONFIG.to_string(), b)).collect();
    for wf in [GATE_WORKFLOW, EVIDENCE_WORKFLOW] {
        if let Some(text) = ws.read_optional(wf)? {
            out.extend(steps.iter().filter(|s| !text.contains(s.as_str())).map(|s| (wf.to_string(), format!("{s}\n"))));
        }
    }
    Ok(out)
}

/// For a workspace `init` has set up already (the shadow): each named project
/// its config lacks is added. A file is written again, for every project the
/// config then lists, only when it is `own` (`init` wrote it; the repository
/// did not commit it) and still reads exactly as `init` wrote it. Any other
/// config gains the blocks, and any other workflow is kept: an edit made
/// since is never lost. Nothing changes when the config lacks none.
pub fn add_projects(
    ws: &Workspace,
    default_branch: &str,
    opts: Options,
    named: &[Project],
    own: &dyn Fn(&str) -> bool,
) -> Result<(), CliError> {
    let blocks = project_blocks(ws, named)?;
    let Some(text) = ws.read_optional(CONFIG)?.filter(|_| !blocks.is_empty()) else { return Ok(()) };
    let adapters = config::parse_config(&text).map_err(|e| CliError::invalid(CONFIG, e))?.adapters;
    let listed: Vec<Project> = adapters.iter().filter_map(|a| Project::listed(&ws.root, a)).collect();
    let before = with_named(&detect(&ws.root), &listed);
    let projects = with_named(&detect(&ws.root), &[listed, named.to_vec()].concat());
    let as_written = |path: &str, was: String| own(path) && ws.read_optional(path).is_ok_and(|now| now == Some(was));
    let config = if as_written(CONFIG, config_text(&before, opts)) {
        config_text(&projects, opts)
    } else {
        format!("{}\n\n{}", text.trim_end(), blocks.join("\n"))
    };
    config::parse_config(&config)
        .map_err(|e| CliError::invalid(CONFIG, format!("with the named projects added it does not parse: {e}")))?;
    write(ws, CONFIG, &config)?;
    [(GATE_WORKFLOW, GATE_TEMPLATE), (EVIDENCE_WORKFLOW, EVIDENCE_TEMPLATE)]
        .into_iter()
        .filter(|(path, template)| as_written(path, render_workflow(template, default_branch, &before, opts)))
        .try_for_each(|(path, template)| write(ws, path, &render_workflow(template, default_branch, &projects, opts)))
}

/// The existing files that lack TLA+, each with the exact text to add: the
/// adapter block to a config without a tla adapter, the steps to a workflow
/// without TLC's install.
fn tla_to_add(ws: &Workspace) -> Result<Vec<(String, String)>, CliError> {
    let has_tla =
        |text: &str| config::parse_config(text).is_ok_and(|c| c.adapters.iter().any(|a| a.language == Language::Tla));
    let lacks = ws.read_optional(CONFIG)?.is_some_and(|text| !has_tla(&text));
    let steps = tla_steps().join("\n") + "\n";
    let mut out: Vec<(String, String)> = lacks.then(|| (CONFIG.to_string(), tla_block())).into_iter().collect();
    for wf in [GATE_WORKFLOW, EVIDENCE_WORKFLOW] {
        if ws.read_optional(wf)?.is_some_and(|text| !text.contains(tla::tools::JAR_ENV)) {
            out.push((wf.to_string(), steps.clone()));
        }
    }
    Ok(out)
}

/// The existing files that lack Kani, each with the exact text to add: the
/// adapter block to a config without a kani adapter, the steps to a
/// workflow without Kani's install.
fn kani_to_add(ws: &Workspace, k: KaniSetup) -> Result<Vec<(String, String)>, CliError> {
    let has_kani =
        |text: &str| config::parse_config(text).is_ok_and(|c| c.adapters.iter().any(|a| a.language == Language::Kani));
    let config_lacks = ws.read_optional(CONFIG)?.is_some_and(|text| !has_kani(&text));
    let steps = kani_steps().join("\n") + "\n";
    let mut out: Vec<(String, String)> =
        config_lacks.then(|| (CONFIG.to_string(), kani_block(k, needs_features(&ws.root)))).into_iter().collect();
    for wf in [GATE_WORKFLOW, EVIDENCE_WORKFLOW] {
        if ws.read_optional(wf)?.is_some_and(|text| !text.contains("kani-verifier")) {
            out.push((wf.to_string(), steps.clone()));
        }
    }
    Ok(out)
}

/// Runs the build probe with the config's kani row (its root and cargo
/// features) and keeps its line in `proob/.cache/kani-probe.toml`.
fn probe(ws: &Workspace) -> Result<String, CliError> {
    let result = kani::probe::run(&kani_adapter(ws)?, &ws.root);
    write(ws, KANI_PROBE, &result.record())?;
    Ok(result.line())
}

/// The adapter of the config's kani row; the default one at the root when
/// the config has none or does not parse (`validate` says why).
fn kani_adapter(ws: &Workspace) -> Result<KaniAdapter, CliError> {
    let row = ws
        .read_optional(CONFIG)?
        .and_then(|text| config::parse_config(&text).ok())
        .and_then(|c| c.adapters.into_iter().find(|a| a.language == Language::Kani));
    let Some(a) = row else { return Ok(KaniAdapter::at(".")) };
    let root = Checked::path(Path::new(&a.root))
        .map_err(|rule| CliError::invalid(CONFIG, format!("kani adapter root `{}`: {rule}", a.root)))?;
    KaniAdapter::configured(&a, root).map_err(|e| CliError::invalid(CONFIG, e))
}

fn write_settings(ws: &Workspace, text: &str, change: Change) -> Result<Change, CliError> {
    if change != Change::Unchanged {
        write(ws, SETTINGS, text)?;
    }
    Ok(change)
}

pub fn text(o: &Outcome) -> String {
    let mut out = String::new();
    let word = |d: &Detected| match d {
        Detected::Rust { .. } => "rust",
        Detected::Python { .. } => "python",
        Detected::TypeScript => "typescript (adapter after launch)",
    };
    let detected: Vec<&str> = o.detected.iter().map(word).collect();
    out.push_str(&format!(
        "detected adapters: {}\n",
        if detected.is_empty() { "none".to_string() } else { detected.join(", ") }
    ));
    for p in &o.projects {
        out.push_str(&format!("named project: {} at {}\n", word(&p.detected), p.root));
    }
    for (path, change) in &o.files {
        let word = match change {
            Change::Written => "written",
            Change::Kept => "kept (exists)",
            Change::Merged => "merged",
            Change::Unchanged => "unchanged",
        };
        out.push_str(&format!("{word:>14}  {path}\n"));
    }
    for (path, text) in &o.projects_to_add {
        let what = if path == CONFIG { "this adapter block" } else { "these steps to its execute job" };
        out.push_str(&format!("project: {path} exists and was kept; add {what}:\n\n{text}\n"));
    }
    out.push_str(&format!("action pin: {}\n", o.action_pin));
    if !o.action_pinned_by_sha {
        out.push_str("note: this proob was built without a release commit; the workflows pin a version tag. Replace it with the release's commit SHA before relying on the gate.\n");
    }
    if let Some(k) = &o.kani {
        out.push_str(&kani_text(k));
    }
    if let Some(t) = &o.tla {
        for (path, text) in &t.to_add {
            let what = if path == CONFIG { "this adapter block" } else { "these steps to its execute job" };
            out.push_str(&format!("tla: {path} exists and was kept; add {what}:\n\n{text}\n"));
        }
        out.push_str(&format!(
            "tla: the workflows install TLC (TLA+ {}); here, TLC runs on Java with {} set to the path of that tla2tools.jar: `proob doctor` says what is missing, /proob:setup installs it\n",
            tla::tools::PIN.version,
            tla::tools::JAR_ENV
        ));
    }
    out.push_str(
        "next: review proob/config.toml, commit, and open the pull request; the gate takes effect after it merges.\n",
    );
    out
}

fn kani_text(k: &KaniOutcome) -> String {
    let (config, workflows): (Vec<_>, Vec<_>) = k.to_add.iter().partition(|(path, _)| path == CONFIG);
    let config = config
        .iter()
        .map(|(path, text)| format!("kani: {path} exists and was kept; add this adapter block:\n\n{text}\n"));
    // Both workflows take the same steps: one listing names them together.
    let paths: Vec<&str> = workflows.iter().map(|(path, _)| path.as_str()).collect();
    let steps = workflows.first().map(|(_, text)| {
        let (them, each) =
            if paths.len() == 1 { ("exists and was kept", "its") } else { ("exist and were kept", "each one's") };
        format!(
            "kani: {} {them}; add these steps to {each} execute job, after the Rust setup steps:\n\n{text}\n",
            paths.join(" and ")
        )
    });
    let adds: String = config.chain(steps).collect();
    format!(
        "kani probe (kept in {KANI_PROBE}): {}\n{adds}kani: an older proob cannot read a Kani proof record and fails to read every record; the gate's and the evidence workflow's proob action and the local proob (the plugin's shim) must be {v} or later\n",
        k.probe,
        v = k.min_proob_version
    )
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

    #[test]
    fn the_shadows_kani_block_samples_fewer_mutants() {
        let rust = Project::at_root(Detected::Rust { toolchain_file: false, needs_features: false });
        let opts = Options { kani: Some(KaniSetup { max_mutants: Some(10) }), tla: false };
        let parsed = config::parse_config(&config_text(&[rust], opts)).expect("parses");
        let kani = parsed.adapters.iter().find(|a| a.language == Language::Kani).expect("a kani adapter");
        assert_eq!(kani.max_mutants, Some(10));
    }

    #[test]
    fn the_kani_block_carries_the_features_hint_when_a_crate_needs_features() {
        let rust = Project::at_root(Detected::Rust { toolchain_file: false, needs_features: true });
        let text = config_text(&[rust], Options { kani: Some(KaniSetup::default()), tla: false });
        let kani = &text[text.find("language = \"kani\"").expect("a kani block")..];
        assert!(kani.contains(FEATURES_HINT), "{text}");
        assert!(!kani_block(KaniSetup::default(), false).contains("features"), "no sign, no hint");
        let parsed = config::parse_config(&text).expect("parses");
        assert!(parsed.adapters.iter().all(|a| !a.sets_cargo_features()), "a hint, not a setting");
    }

    #[test]
    fn tla_adds_its_adapter_even_without_a_detected_language() {
        let parsed = config::parse_config(&config_text(&[], Options { kani: None, tla: true })).expect("parses");
        let tla = parsed.adapters.iter().find(|a| a.language == Language::Tla).expect("a tla adapter");
        assert_eq!((tla.runner.as_str(), tla.mutation_tool.as_str()), ("tlc", "spec-mutants"));
        assert!(all_setup_steps(&[], Options { kani: None, tla: true }).contains("PROOB_TLA2TOOLS"));
    }

    #[test]
    fn a_project_below_the_root_is_set_up_by_its_path() {
        let at = |root: &str, detected| Project { root: root.to_string(), detected };
        let python = |pyproject, requirements| Detected::Python { pyproject, requirements };
        let steps = |p: &Project| setup_steps(p).join("\n");
        assert!(steps(&at("svc", python(true, true))).contains("pip install -e svc -r svc/requirements.txt && "));
        assert!(steps(&at(".", python(true, true))).contains("pip install -e . -r requirements.txt && "));
        let bare = steps(&at("svc", python(false, false)));
        assert!(bare.contains("--upgrade pip && python -m pip install \"") && !bare.contains("svc"), "{bare}");
        let rust = steps(&at("core", Detected::Rust { toolchain_file: true, needs_features: false }));
        assert!(rust.contains("core/rust-toolchain.toml") && rust.contains("working-directory: \"core\""), "{rust}");
        assert_eq!(["./svc", "svc//", "a/./b/", "./", "."].map(normal), ["svc", "svc", "a/b", ".", "."]);
        let named = [at(".", python(false, false)), at("svc", python(true, false)), at("svc", python(true, false))];
        let all = with_named(&[python(false, true)], &named);
        assert_eq!(all.iter().map(|p| p.root.as_str()).collect::<Vec<_>>(), [".", "svc"], "listed once each");
        let parsed = config::parse_config(&config_text(&all, Options::default())).expect("parses");
        assert!(all.iter().all(|p| parsed.adapters.iter().any(|a| p.is_adapter(a))), "each has its adapter");
    }
}
