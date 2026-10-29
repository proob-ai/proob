//! Embeds the Action reference that `proob init` writes into customer
//! workflows. A release build runs in GitHub Actions, where `GITHUB_SHA` is the
//! release commit, so the rendered workflows pin `proob-ai/proob@<sha>` with
//! the version as a comment. `PROOB_ACTION_SHA` overrides it. A build with
//! neither (a local build) can only pin the version tag, and says so in the
//! comment it writes.
fn main() {
    println!("cargo:rerun-if-env-changed=PROOB_ACTION_SHA");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let sha = ["PROOB_ACTION_SHA", "GITHUB_SHA"]
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| is_full_sha(s));
    let (pin, comment) = match sha {
        Some(sha) => (format!("proob-ai/proob@{sha}"), format!("# v{version}")),
        None => (format!("proob-ai/proob@v{version}"), "# pin to a commit SHA before relying on this gate".to_string()),
    };
    println!("cargo:rustc-env=PROOB_ACTION_PIN={pin}");
    println!("cargo:rustc-env=PROOB_ACTION_PIN_COMMENT={comment}");
    println!("cargo:rustc-env=PROOB_ACTION_PINNED_BY_SHA={}", comment.starts_with("# v"));
}

/// Forty (SHA-1) or sixty-four (SHA-256) hex characters.
fn is_full_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}
