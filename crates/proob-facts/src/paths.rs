//! Where proob's files live in a repository, named once so the CLI's `init`,
//! the governed-file rule, and the cache agree.
//!
//! A governed file is a property file, `proob/config.toml`, one of the two
//! proob workflow files, or a member of a bound check's binding set. The first
//! three classes are fixed by path; binding sets come from the adapters.

pub const PROOB_DIR: &str = "proob";
pub const CONFIG: &str = "proob/config.toml";
pub const PROPERTIES_DIR: &str = "proob/properties";
pub const CACHE_DIR: &str = "proob/.cache";
/// Where `proob run --record` appends `results.jsonl`, the execute job's handoff.
pub const RESULTS_DIR: &str = "proob/.cache/results";
/// Where the evidence judge job writes `evidence.jsonl` for the template to upload.
pub const EVIDENCE_DIR: &str = "proob/.cache/evidence";
pub const GATE_WORKFLOW: &str = ".github/workflows/proob-gate.yml";
pub const EVIDENCE_WORKFLOW: &str = ".github/workflows/proob-evidence.yml";
/// Where GitHub looks for CODEOWNERS, in its own order of precedence.
pub const CODEOWNERS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];

/// `proob/properties/<name>.toml`, one level deep.
pub fn is_property_file(path: &str) -> bool {
    path.strip_prefix(PROPERTIES_DIR)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|name| name.ends_with(".toml") && !name.contains('/'))
}

/// Governed by path alone: a property file, the config, or a proob workflow.
pub fn is_fixed_governed(path: &str) -> bool {
    path == CONFIG || path == GATE_WORKFLOW || path == EVIDENCE_WORKFLOW || is_property_file(path)
}
