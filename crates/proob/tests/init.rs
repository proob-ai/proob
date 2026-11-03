//! `proob init`: config with the detected adapters, both two-job workflows
//! with matching setup steps, `fetch-depth: 0`, the action pin, the settings
//! recommendation merged without clobbering, and an idempotent second run.
//! `init --kani`: the Kani adapter, its pinned and cached install, the build
//! probe, and what to add where proob's files already exist.
mod common;

use common::*;
use proob_facts::config::Language;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FILES: [&str; 5] = [
    "proob/config.toml",
    ".github/workflows/proob-gate.yml",
    ".github/workflows/proob-evidence.yml",
    ".claude/settings.json",
    ".gitignore",
];

fn snapshot(repo: &TempRepo) -> BTreeMap<&'static str, String> {
    FILES.iter().map(|f| (*f, repo.read(f))).collect()
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn init_on_a_rust_repository_with_a_toolchain_file() {
    let repo = TempRepo::new();
    repo.write("Cargo.toml", "[package]\nname = \"x\"\n");
    repo.write("rust-toolchain.toml", "[toolchain]\nchannel = \"1.99.1\"\n");
    repo.write(
        ".claude/settings.json",
        "{\n  \"permissions\": {\"allow\": [\"Bash(ls)\"]},\n  \"enabledPlugins\": {\"other@shop\": true}\n}\n",
    );
    repo.commit("start", NOW - DAY);
    let out = run(repo.proob().arg("init"));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("detected adapters: rust"), "{}", out.stdout);

    let config = repo.read("proob/config.toml");
    assert!(config.contains("allow_self_signed = true"));
    assert!(config.contains("Checked (self-signed)"), "the comment explains the word");
    assert!(config.contains("Checked (tested)"), "the comment says a test never reaches Holds");
    assert!(config.contains("\ntested = \"pass\"\n"), "the tested row gates a passing test-backed property");
    assert!(config.contains("language = \"rust\""));
    assert!(config.contains("runner = \"nextest\""));
    assert!(!config.contains("features"), "nothing about features without a sign of needing them");
    proob_facts::config::parse_config(&config).expect("the rendered config parses");
    assert!(!config.contains("kani") && !out.stdout.contains("kani"), "Kani only with --kani");
    assert!(!repo.root().join("proob/.cache/kani-probe.toml").exists(), "no probe without --kani");

    let gate = repo.read(".github/workflows/proob-gate.yml");
    let evidence = repo.read(".github/workflows/proob-evidence.yml");
    for wf in [&gate, &evidence] {
        assert!(!wf.contains("@@"), "a placeholder survived:\n{wf}");
        assert_eq!(
            count(wf, "\n          fetch-depth: 0\n"),
            count(wf, "actions/checkout@"),
            "every checkout is full depth"
        );
        assert!(wf.contains("rustup toolchain install"), "setup step for rust-toolchain.toml");
        assert!(wf.contains("cargo-nextest@"), "the adapter's tools are installed");
        assert!(!wf.contains("setup-python"), "no python setup for a rust repo");
        assert!(!wf.contains("kani"), "no Kani install without --kani");
        assert!(wf.contains("  execute:\n    name: proob execute\n    runs-on: ubuntu-latest\n    # No token: this job runs repository code.\n    permissions: {}\n"));
        assert!(wf.contains("    needs: execute\n"));
        assert!(!wf.lines().any(|l| l.trim_end().ends_with(": write")), "no write permission anywhere:\n{wf}");
        assert_eq!(count(wf, &format!("uses: {}", proob_action_pin())), 2, "both jobs use the pinned action:\n{wf}");
        for third_party in
            ["actions/checkout@", "actions/upload-artifact@", "actions/download-artifact@", "taiki-e/install-action@"]
        {
            for line in wf.lines().filter(|l| l.contains(third_party)) {
                let sha = line.split('@').nth(1).and_then(|s| s.split_whitespace().next()).expect("sha");
                assert_eq!(sha.len(), 40, "{line}");
                assert!(line.contains("# v"), "version comment: {line}");
            }
        }
    }
    assert!(gate.contains("on:\n  pull_request:\n"));
    assert!(gate.contains("    name: proob gate\n"));
    assert!(evidence.contains("    branches: [main]\n"));
    assert!(evidence.contains("  schedule:\n"));
    assert!(evidence.contains("  workflow_dispatch:\n"));
    assert!(evidence.contains("    name: proob evidence\n"));
    assert!(evidence.contains("name: proob-evidence\n"), "the evidence artifact");

    let settings: serde_json::Value = serde_json::from_str(&repo.read(".claude/settings.json")).expect("json");
    assert_eq!(settings["permissions"]["allow"][0], "Bash(ls)", "other keys survive");
    assert_eq!(settings["enabledPlugins"]["other@shop"], true, "other plugins survive");
    assert_eq!(settings["enabledPlugins"]["proob@proob"], true);
    assert_eq!(settings["extraKnownMarketplaces"]["proob"]["source"]["repo"], "proob-ai/proob");
    assert!(repo.read(".gitignore").contains("proob/.cache/"));

    let first = snapshot(&repo);
    let again = run(repo.proob().arg("init"));
    assert_eq!(again.code, 0);
    assert_eq!(snapshot(&repo), first, "a second run changes no byte");
    assert!(again.stdout.contains("kept (exists)"), "{}", again.stdout);
}

fn proob_action_pin() -> String {
    let out = run(TempRepo::new().proob().args(["init", "--json"]));
    let json: serde_json::Value = serde_json::from_str(&out.stdout).expect("json");
    json["action_pin"].as_str().expect("pin").to_string()
}

#[test]
fn the_action_pin_is_a_sha_or_says_it_is_not() {
    let repo = TempRepo::new();
    let out = run(repo.proob().args(["init", "--json"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let json: serde_json::Value = serde_json::from_str(&out.stdout).expect("json");
    let pin = json["action_pin"].as_str().expect("pin");
    let by_sha = json["action_pinned_by_sha"].as_bool().expect("flag");
    let reference = pin.split_whitespace().next().expect("ref");
    let (_, version) = reference.split_once('@').expect("owner/repo@ref");
    if by_sha {
        assert_eq!(version.len(), 40, "{pin}");
        assert!(pin.contains("# v"), "{pin}");
    } else {
        assert!(version.starts_with('v'), "{pin}");
        assert!(pin.contains("pin to a commit SHA"), "{pin}");
    }
    let gate = repo.read(".github/workflows/proob-gate.yml");
    assert!(gate.contains(pin), "the workflow carries the pin verbatim");
}

#[test]
fn init_hints_at_cargo_features_when_a_crate_visibly_needs_them() {
    for (path, text) in [
        (
            "build.rs",
            "fn main() {\n    #[cfg(not(any(feature = \"sqlite\", feature = \"mysql\")))]\n    compile_error!(\"You need to enable one DB backend: cargo build --features sqlite\");\n}\n",
        ),
        ("member/Cargo.toml", "[package]\nname = \"m\"\n\n[[test]]\nname = \"t\"\nrequired-features = [\"std\"]\n"),
    ] {
        let repo = TempRepo::new();
        repo.write("Cargo.toml", "[workspace]\nmembers = [\"member\"]\n");
        repo.write(path, text);
        repo.commit("start", NOW - DAY);
        let out = run(repo.proob().arg("init"));
        assert_eq!(out.code, 0, "{}", out.stderr);
        let config = repo.read("proob/config.toml");
        assert!(config.contains("# This crate's tests look like they need cargo features"), "{path}:\n{config}");
        assert!(config.contains("features = [\"name\"], all_features = true, no_default_features = true"), "{config}");
        let parsed = proob_facts::config::parse_config(&config).expect("the rendered config parses");
        assert!(!parsed.adapters[0].sets_cargo_features(), "a hint, not a setting");
    }
}

#[test]
fn init_on_a_python_repository_renders_setup_python() {
    let repo = TempRepo::new();
    repo.write("pyproject.toml", "[project]\nname = \"svc\"\n");
    repo.write("requirements.txt", "hypothesis\n");
    repo.write("package.json", "{}");
    let out = run(repo.proob().arg("init"));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("python"), "{}", out.stdout);
    assert!(out.stdout.contains("typescript (adapter after launch)"), "{}", out.stdout);
    let config = repo.read("proob/config.toml");
    assert!(config.contains("language = \"python\""));
    assert!(config.contains("TypeScript adapter"), "the note about typescript:\n{config}");
    proob_facts::config::parse_config(&config).expect("parses");
    let gate = repo.read(".github/workflows/proob-gate.yml");
    assert!(gate.contains("actions/setup-python@6d4fd22b20465ccc0f7354476588e8df7c9ca0e0 # v7.1.0"), "{gate}");
    assert!(gate.contains("pip install -e . -r requirements.txt"), "{gate}");
    assert!(!gate.contains("rustup"), "{gate}");
    assert!(gate.contains("TypeScript adapter arrives after launch"), "{gate}");
    // No settings file existed: one is written with the recommendation alone.
    let settings: serde_json::Value = serde_json::from_str(&repo.read(".claude/settings.json")).expect("json");
    assert_eq!(settings.as_object().expect("object").len(), 2);
    assert!(!repo.root().join("rust-toolchain.toml").exists());
}

#[test]
fn init_without_an_adapter_says_so_and_keeps_existing_files() {
    let repo = TempRepo::new();
    repo.write("proob/config.toml", "# mine\nschema_version = 1\n[policy]\nholds = \"pass\"\nchecked = \"pass\"\nproposed = \"pass\"\nwaived = \"pass\"\nnot_checked_within_grace = \"pass\"\nnot_checked_after_grace = \"warn\"\nre_approve = \"warn\"\nbroken_affected = \"block\"\nbroken_unaffected = \"pass\"\n");
    repo.write(".gitignore", "target/\nproob/.cache/\n");
    let out = run(repo.proob().arg("init"));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("detected adapters: none"), "{}", out.stdout);
    assert!(repo.read("proob/config.toml").starts_with("# mine"), "an existing config is the team's");
    assert_eq!(repo.read(".gitignore"), "target/\nproob/.cache/\n");
    let gate = repo.read(".github/workflows/proob-gate.yml");
    assert!(gate.contains("No adapter detected"), "{gate}");
}

#[test]
fn init_refuses_to_clobber_an_unreadable_settings_file() {
    let repo = TempRepo::new();
    repo.write(".claude/settings.json", "{ not json");
    let out = run(repo.proob().arg("init"));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains(".claude/settings.json"), "{}", out.stderr);
    assert_eq!(repo.read(".claude/settings.json"), "{ not json");
}

// ---------------------------------------------------------------------------
// --tla
// ---------------------------------------------------------------------------

#[test]
fn init_tla_writes_the_adapter_and_installs_the_pinned_tlc_in_both_workflows() {
    let repo = TempRepo::new();
    repo.write("specs/Mutex.tla", "---- MODULE Mutex ----\n====\n");
    repo.commit("start", NOW - DAY);
    let out = run(repo.proob().args(["init", "--tla"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    let parsed = proob_facts::config::parse_config(&repo.read("proob/config.toml")).expect("parses");
    let languages: Vec<Language> = parsed.adapters.iter().map(|a| a.language).collect();
    assert_eq!(languages, vec![Language::Tla]);
    assert!(out.stdout.contains("PROOB_TLA2TOOLS"), "{}", out.stdout);
    let pin = &*proob_adapters::tla::tools::PIN;
    for wf in [".github/workflows/proob-gate.yml", ".github/workflows/proob-evidence.yml"] {
        let text = repo.read(wf);
        assert!(
            text.contains(&pin.url) && text.contains(&format!("echo \"{}  $jar\" | sha256sum -c -", pin.sha256)),
            "{text}"
        );
        assert!(text.contains("echo \"PROOB_TLA2TOOLS=$jar\" >> \"$GITHUB_ENV\""), "{text}");
    }

    // A config that exists is kept; the block it lacks is printed.
    let repo = TempRepo::new();
    repo.write("Cargo.toml", "[package]\nname = \"x\"\n");
    repo.commit("start", NOW - DAY);
    assert_eq!(run(repo.proob().arg("init")).code, 0);
    let out = run(repo.proob().args(["init", "--tla"]));
    assert!(
        out.stdout.contains("tla: proob/config.toml exists and was kept; add this adapter block:"),
        "{}",
        out.stdout
    );
    assert!(!repo.read("proob/config.toml").contains("language = \"tla\""));
    // So are the workflows; the steps they lack are printed.
    assert!(
        out.stdout
            .contains("tla: .github/workflows/proob-gate.yml exists and was kept; add these steps to its execute job:"),
        "{}",
        out.stdout
    );
    assert!(!repo.read(".github/workflows/proob-gate.yml").contains("PROOB_TLA2TOOLS"));
}

#[test]
fn doctor_after_init_tla_names_the_pinned_tlc_and_finds_it_once_set() {
    let repo = TempRepo::new();
    repo.write("specs/Mutex.tla", "---- MODULE Mutex ----\n====\n");
    repo.commit("start", NOW - DAY);
    assert_eq!(run(repo.proob().args(["init", "--tla"])).code, 0);
    let out = run(repo.proob().args(["doctor", "--json"]).env_remove("PROOB_TLA2TOOLS"));
    assert_eq!(out.code, 1, "{}", out.stderr);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).expect("json");
    assert_eq!(v["ok"], false);
    assert_eq!(
        (v["adapters"][0]["language"].as_str(), v["adapters"][0]["ready"].as_bool()),
        (Some("tla"), Some(false))
    );
    let reason = v["adapters"][0]["reason"].as_str().expect("a reason");
    assert!(
        reason.starts_with("TLC was not found") && reason.contains(&proob_adapters::tla::tools::PIN.sha256),
        "{reason}"
    );

    // With the pinned jar set (the ci workflow's tlc job; skipped where TLC does not resolve).
    use proob_adapters::tla::tools::{Resolution, resolve};
    match resolve(&[]) {
        Resolution::Ready(_) => {
            // The harness narrows PATH; TLC needs the java this test found.
            let out = run(repo.proob().arg("doctor").env("PATH", std::env::var_os("PATH").unwrap_or_default()));
            assert_eq!(out.code, 0, "{}", out.stdout);
            assert!(out.stdout.starts_with("tla at .: ready (java "), "{}", out.stdout);
        }
        Resolution::Missing { reason, .. } if std::env::var("PROOB_REQUIRE_TLC").as_deref() == Ok("1") => {
            panic!("PROOB_REQUIRE_TLC=1 but TLC does not resolve: {reason}")
        }
        Resolution::Missing { reason, .. } => eprintln!("skipped: {reason}"),
    }
}

// ---------------------------------------------------------------------------
// --kani
// ---------------------------------------------------------------------------

const PROBE: &str = "proob/.cache/kani-probe.toml";

fn rust_repo() -> TempRepo {
    let repo = TempRepo::new();
    repo.write("Cargo.toml", "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n");
    repo.write("src/lib.rs", "pub fn one() -> u8 {\n    1\n}\n");
    repo.commit("start", NOW - DAY);
    repo
}

/// The steps `init --kani` renders, as they appear in a workflow it writes.
fn kani_steps(wf: &str) -> &str {
    let start = wf.find("      - name: Cache Kani").expect("the Kani cache step");
    let end = wf[start..].find("cargo kani setup\n").expect("the Kani install step") + "cargo kani setup\n".len();
    &wf[start..start + end]
}

#[test]
fn init_kani_writes_the_adapter_and_a_pinned_cached_install_in_both_workflows() {
    let repo = rust_repo();
    // The sandbox PATH holds only git: no Kani here, so the probe records why.
    let out = run(repo.proob().args(["init", "--kani"]));
    assert_eq!(out.code, 0, "{}", out.stderr);

    let config = repo.read("proob/config.toml");
    let parsed = proob_facts::config::parse_config(&config).expect("the rendered config parses");
    let languages: Vec<Language> = parsed.adapters.iter().map(|a| a.language).collect();
    assert_eq!(languages, vec![Language::Rust, Language::Kani], "{config}");
    let kani = &parsed.adapters[1];
    assert_eq!((kani.root.as_str(), kani.runner.as_str(), kani.mutation_tool.as_str()), (".", "kani", "cargo-mutants"));
    assert_eq!(kani.max_mutants, None, "the adapter's default sample");

    for wf in [repo.read(".github/workflows/proob-gate.yml"), repo.read(".github/workflows/proob-evidence.yml")] {
        assert!(!wf.contains("@@"), "a placeholder survived:\n{wf}");
        let steps = kani_steps(&wf);
        assert!(wf.find("cargo-nextest@").expect("rust steps") < wf.find(steps).expect("kani steps"), "after cargo");
        assert!(steps.contains("cargo install --locked kani-verifier --version 0.68.0 && cargo kani setup"), "{steps}");
        assert!(steps.contains("key: kani-${{ runner.os }}-${{ runner.arch }}-0.68.0\n"), "{steps}");
        assert!(steps.contains("            ~/.kani\n"), "{steps}");
        assert!(steps.contains("            ~/.rustup/toolchains/nightly-2026-08-21-*\n"), "{steps}");
        assert!(steps.contains("if: steps.kani-cache.outputs.cache-hit != 'true'"), "{steps}");
        let cache = steps.lines().find(|l| l.contains("actions/cache@")).expect("the cache action");
        let sha = cache.split('@').nth(1).and_then(|s| s.split_whitespace().next()).expect("sha");
        assert_eq!(sha.len(), 40, "{cache}");
        assert!(cache.contains("# v"), "version comment: {cache}");
    }

    let probe = repo.read(PROBE);
    assert!(probe.starts_with("kani_probe = \"failed: the Kani toolchain was not found"), "{probe}");
    assert!(
        out.stdout.contains("kani probe (kept in proob/.cache/kani-probe.toml): failed: the Kani toolchain"),
        "{}",
        out.stdout
    );
    assert!(repo.read(".gitignore").contains("proob/.cache/"), "the probe is never committed");
    assert!(!out.stdout.contains("exists and was kept"), "nothing to add by hand:\n{}", out.stdout);

    let first = snapshot(&repo);
    let again = run(repo.proob().args(["init", "--kani"]));
    assert_eq!(again.code, 0, "{}", again.stderr);
    assert_eq!(snapshot(&repo), first, "a second run changes no byte");
    assert!(!again.stdout.contains("add this adapter block"), "the config has the adapter:\n{}", again.stdout);
}

#[test]
fn init_kani_in_an_initialised_repository_keeps_the_files_and_prints_what_to_add() {
    let repo = rust_repo();
    assert_eq!(run(repo.proob().arg("init")).code, 0);
    let before = snapshot(&repo);
    let out = run(repo.proob().args(["init", "--kani"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(snapshot(&repo), before, "the team's files are kept");

    // Exactly what a fresh `init --kani` writes.
    let fresh = rust_repo();
    assert_eq!(run(fresh.proob().args(["init", "--kani"])).code, 0);
    let config = fresh.read("proob/config.toml");
    let block = &config[config.find("[[adapters]]\n# Kani").expect("the Kani block")
        ..config.find("\n# The gate policy").expect("policy")];
    let steps = kani_steps(&fresh.read(".github/workflows/proob-gate.yml")).to_string();
    assert_eq!(steps, kani_steps(&fresh.read(".github/workflows/proob-evidence.yml")));
    assert!(
        out.stdout
            .contains(&format!("kani: proob/config.toml exists and was kept; add this adapter block:\n\n{block}")),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(&format!(
            "kani: .github/workflows/proob-gate.yml and .github/workflows/proob-evidence.yml exist and were kept; add these steps to each one's execute job, after the Rust setup steps:\n\n{steps}"
        )),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(&format!("must be {} or later", env!("CARGO_PKG_VERSION"))),
        "the oldest proob that reads a Kani record:\n{}",
        out.stdout
    );
    assert!(repo.read(PROBE).starts_with("kani_probe = "), "the probe ran here too");

    let json = run(repo.proob().args(["init", "--kani", "--json"])).json();
    assert_eq!(json["kani"]["to_add"].as_array().map(Vec::len), Some(3), "{json}");
    assert_eq!(json["kani"]["min_proob_version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn init_kani_needs_a_rust_crate() {
    let repo = TempRepo::new();
    repo.write("pyproject.toml", "[project]\nname = \"svc\"\n");
    let out = run(repo.proob().args(["init", "--kani"]));
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("--kani needs a Rust crate"), "{}", out.stderr);
    assert!(!repo.root().join("proob/config.toml").exists(), "nothing written");
}

// ---------------------------------------------------------------------------
// The build probe, with the real Kani (skipped without it)
// ---------------------------------------------------------------------------

fn kani_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/kani-crate").canonicalize().expect("fixture")
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("read_dir").filter_map(Result::ok) {
        let name = entry.file_name();
        if name == "target" || name == "proob" {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        if src.is_dir() {
            std::fs::create_dir_all(&dst).expect("mkdir");
            copy_tree(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).expect("copy");
        }
    }
}

/// Whether the pinned toolchain resolves here; a note when it does not.
/// `PROOB_REQUIRE_KANI` (CI's kani job) turns that skip into a failure.
fn kani_present() -> bool {
    use proob_adapters::kani::tools::{Resolution, resolve};
    match resolve(&[]) {
        Resolution::Ready(_) => true,
        Resolution::Missing { reason, .. } => {
            assert!(std::env::var_os("PROOB_REQUIRE_KANI").is_none(), "PROOB_REQUIRE_KANI is set but {reason}");
            eprintln!("skipped: {reason}");
            false
        }
    }
}

/// `init --kani` with the real PATH, building in the Kani fixture's target
/// directory so dependencies build under Kani once per machine.
fn probed(repo: &TempRepo) -> String {
    let mut cmd = repo.proob();
    cmd.env("PATH", std::env::var_os("PATH").expect("PATH")).env("CARGO_TARGET_DIR", kani_fixture().join("target"));
    let out = run(cmd.args(["init", "--kani"]));
    assert_eq!(out.code, 0, "{}", out.stderr);
    repo.read(PROBE)
}

#[test]
fn the_build_probe_passes_on_the_kani_fixture_and_fails_on_a_crate_newer_than_kanis_nightly() {
    if !kani_present() {
        return;
    }
    let repo = TempRepo::new();
    copy_tree(&kani_fixture(), repo.root());
    repo.commit("the kani fixture", NOW - DAY);
    let lib = repo.read("src/lib.rs");
    assert_eq!(probed(&repo), "kani_probe = \"passed\"\n");
    assert_eq!(repo.read("src/lib.rs"), lib, "the probe's harness goes in a copy, never the tree");
    assert_eq!(
        repo.git(&["status", "--porcelain", "--", "src", "Cargo.toml", "Cargo.lock"]),
        "",
        "the tree is untouched"
    );

    // A crate that asks for a compiler newer than Kani's nightly.
    let newer = rust_repo();
    newer.write(
        "Cargo.toml",
        &newer.read("Cargo.toml").replace("edition = \"2024\"\n", "edition = \"2024\"\nrust-version = \"1.999\"\n"),
    );
    newer.commit("needs rustc 1.999", NOW - DAY);
    let probe = probed(&newer);
    assert!(probe.starts_with("kani_probe = \"failed: the crate did not compile under Kani"), "{probe}");
    assert!(probe.contains("is not supported by the following package"), "{probe}");
}

#[test]
fn the_build_probe_builds_with_the_kani_rows_cargo_features() {
    if !kani_present() {
        return;
    }
    let repo = rust_repo();
    repo.write("Cargo.toml", &repo.read("Cargo.toml").replace("\n[workspace]", "\n[features]\ndb = []\n\n[workspace]"));
    repo.write(
        "build.rs",
        "fn main() {\n    #[cfg(not(feature = \"db\"))]\n    compile_error!(\"enable a backend: --features db\");\n}\n",
    );
    repo.commit("needs a feature", NOW - DAY);
    let probe = probed(&repo);
    assert!(probe.contains("enable a backend"), "fails without it: {probe}");
    let config = repo.read("proob/config.toml");
    let kani = &config[config.find("language = \"kani\"").expect("a kani block")..];
    assert!(kani.contains("look like they need cargo features"), "the kani block has the hint: {config}");
    repo.write(
        "proob/config.toml",
        &config.replace("language = \"kani\"\n", "language = \"kani\"\nfeatures = [\"db\"]\n"),
    );
    assert_eq!(probed(&repo), "kani_probe = \"passed\"\n", "a rerun probes with the row's features");
}
