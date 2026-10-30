//! `proob init`: config with the detected adapters, both two-job workflows
//! with matching setup steps, `fetch-depth: 0`, the action pin, the settings
//! recommendation merged without clobbering, and an idempotent second run.
mod common;

use common::*;
use std::collections::BTreeMap;

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
