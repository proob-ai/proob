//! How much of a scope's mutants one score runs, and how many at once, for
//! the adapters that drive an external mutation tool (cargo-mutants, mutmut).
//!
//! # Sample
//!
//! A score lists every mutant in scope first, then runs at most
//! `max_mutants` of them (config; default `DEFAULT_MAX_MUTANTS`). Which ones
//! is a pure function of the listed names and the property's scoring seed
//! (`Seed::for_scoring(id)`): each name is ranked by a hash of the seed and
//! the name, and the lowest ranks are run, so the same tree and property
//! always run the same sample, on every machine. The rest are reported as
//! `unsampled`, in no other count. A score's counts, and so the kill floor,
//! are those of the sample: a sampled score estimates the strength over the
//! scope and is never exhaustive.
//!
//! # Jobs
//!
//! Mutants run `jobs` at a time (config; by default `local_jobs`): half the
//! CPUs, and at most as many as 80% of the free space on the temporary
//! directory's volume holds copies of the tree, because cargo-mutants copies
//! the tree and builds it once per parallel job. A copy's size is measured by
//! the adapter when it can be (rust: the source tree plus its `target/debug`),
//! else taken as `BYTES_PER_JOB`. The score's text output says which bound won.
use crate::{Language, RegistryError, Seed};
use proob_facts::config::Adapter as AdapterConfig;
use std::path::Path;

/// Mutants run per score unless config says otherwise.
pub const DEFAULT_MAX_MUTANTS: u32 = 30;
/// Free bytes one parallel job is taken to need when its copy (a tree and
/// its build output) could not be measured.
pub const BYTES_PER_JOB: u64 = 1_500_000_000;

/// How many mutants run at once, and what chose that number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jobs {
    /// `jobs` in config.
    Configured(u32),
    /// `jobs(cpus, free, per_job)`; `measured` says whether `per_job` came
    /// from the tree or is `BYTES_PER_JOB`.
    Sized { jobs: u32, cpus: usize, free: Option<u64>, per_job: u64, measured: bool },
}

impl Jobs {
    pub fn count(self) -> u32 {
        match self {
            Jobs::Configured(n) | Jobs::Sized { jobs: n, .. } => n,
        }
    }

    /// One line for a score's text output: the count and why.
    pub fn line(self) -> String {
        let gb = |b: u64| format!("{:.1} GB", b as f64 / 1e9);
        match self {
            Jobs::Configured(n) => format!("{n} (set by `jobs` in proob/config.toml)"),
            Jobs::Sized { jobs, cpus, free, per_job, measured } => format!(
                "{jobs}: min(half of {cpus} CPUs, 80% of {} free / {} per copy{}), at least 1",
                free.map_or("unknown".to_string(), gb),
                gb(per_job),
                if measured { ", measured" } else { ", not measured: assumed" }
            ),
        }
    }
}

/// A score's cap and parallelism, past config validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_mutants: u32,
    /// `None`: computed on the machine at score time (`local_jobs`).
    pub jobs: Option<u32>,
}

impl Limits {
    pub const DEFAULT: Limits = Limits { max_mutants: DEFAULT_MAX_MUTANTS, jobs: None };

    /// The configured jobs, else the machine's bound for `dir` with copies
    /// of `per_job` bytes (`None`: unmeasured).
    pub fn jobs_for(&self, dir: &Path, per_job: Option<u64>) -> Jobs {
        self.jobs.map_or_else(|| local_jobs(dir, per_job), Jobs::Configured)
    }
}

/// `max_mutants` and `jobs` from a config entry; zero is refused for both.
pub fn limits(a: &AdapterConfig, language: Language) -> Result<Limits, RegistryError> {
    let refuse = |option, message| RegistryError::UnsupportedOption { language, option, message };
    match (a.max_mutants, a.jobs) {
        (Some(0), _) => Err(refuse("max_mutants", "must be at least 1: a score that runs no mutant measures nothing")),
        (_, Some(0)) => Err(refuse("jobs", "must be at least 1")),
        (max, jobs) => Ok(Limits { max_mutants: max.unwrap_or(DEFAULT_MAX_MUTANTS), jobs }),
    }
}

/// The indices of the names one score runs, in list order: all of them when
/// they fit under `cap`, else exactly `cap`, the lowest by `rank`. Pure.
pub fn sample(names: &[String], cap: u32, seed: Seed) -> Vec<usize> {
    let cap = cap as usize;
    if names.len() <= cap {
        return (0..names.len()).collect();
    }
    let mut ranked: Vec<(u64, usize)> = names.iter().enumerate().map(|(i, n)| (rank(seed, n), i)).collect();
    ranked.sort_unstable();
    let mut chosen: Vec<usize> = ranked.into_iter().take(cap).map(|(_, i)| i).collect();
    chosen.sort_unstable();
    chosen
}

/// A name's place in the seed's order: splitmix64 over the seed and the
/// name's FNV-1a hash, so it depends on nothing but the two.
fn rank(seed: Seed, name: &str) -> u64 {
    let fnv = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    let z = (seed.0 ^ fnv).wrapping_add(0x9e37_79b9_7f4a_7c15);
    let z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Parallel jobs for `cpus`, `free` bytes (unknown: no disk bound), and
/// copies of `per_job` bytes (unmeasured or zero: `BYTES_PER_JOB`):
/// `min(cpus / 2, floor(free * 0.8 / per_job))`, at least 1. Pure.
pub fn jobs(cpus: usize, free: Option<u64>, per_job: Option<u64>) -> Jobs {
    let measured = per_job.is_some_and(|b| b > 0);
    let per = per_job.filter(|b| *b > 0).unwrap_or(BYTES_PER_JOB);
    let by_cpu = u128::try_from(cpus / 2).unwrap_or(u128::MAX);
    let by_disk = free.map_or(u128::MAX, |f| u128::from(f) * 4 / 5 / u128::from(per));
    let n = u32::try_from(by_cpu.min(by_disk)).unwrap_or(u32::MAX).max(1);
    Jobs::Sized { jobs: n, cpus, free, per_job: per, measured }
}

/// `jobs` for this machine, with the free space of `dir`'s volume.
pub fn local_jobs(dir: &Path, per_job: Option<u64>) -> Jobs {
    jobs(std::thread::available_parallelism().map_or(1, |n| n.get()), free_bytes(dir), per_job)
}

/// Free bytes a run that builds must find on the volume it builds into.
pub const MIN_FREE_BYTES: u64 = 2_000_000_000;

/// Refuses, with the reason, a run that builds under `dir` when its volume
/// (that of its nearest existing ancestor) has under `MIN_FREE_BYTES` free;
/// unknown free space does not refuse.
pub fn room(dir: &Path) -> Result<(), String> {
    room_for(dir.ancestors().find_map(free_bytes), dir)
}

/// Pure: `room` with `free` bytes measured.
pub fn room_for(free: Option<u64>, dir: &Path) -> Result<(), String> {
    match free {
        Some(f) if f < MIN_FREE_BYTES => Err(format!(
            "only {:.1} GB free on the volume holding {}; proob needs at least {} GB there to build, so the run did not start; free some space and run again",
            f as f64 / 1e9,
            dir.display(),
            MIN_FREE_BYTES / 1_000_000_000
        )),
        _ => Ok(()),
    }
}

#[cfg(unix)]
fn free_bytes(dir: &Path) -> Option<u64> {
    rustix::fs::statvfs(dir).ok().map(|s| s.f_bavail.saturating_mul(s.f_frsize))
}

// shortcut: no free-space probe off unix, so only the CPU bound applies there; add one if Windows scoring matters.
#[cfg(not(unix))]
fn free_bytes(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("src/map.rs:{i}:5: replace f -> bool with true")).collect()
    }

    #[test]
    fn a_volume_under_two_gigabytes_refuses_a_build() {
        let dir = Path::new("/repo/target");
        let refused = room_for(Some(1_500_000_000), dir).unwrap_err();
        assert!(refused.starts_with("only 1.5 GB free on the volume holding /repo/target"), "{refused}");
        assert!(room_for(Some(MIN_FREE_BYTES), dir).is_ok());
        assert!(room_for(None, dir).is_ok(), "unknown free space does not refuse");
    }

    #[test]
    fn a_sample_is_exact_sized_ordered_and_a_function_of_seed_and_names() {
        let all = names(412);
        let s = sample(&all, 30, Seed(7));
        assert_eq!(s.len(), 30);
        assert!(s.windows(2).all(|w| w[0] < w[1]), "list order, no repeats");
        assert_eq!(s, sample(&all, 30, Seed(7)), "stable across runs");
        assert_ne!(s, sample(&all, 30, Seed(8)), "another property, another sample");
        assert_eq!(all.len() - s.len(), 382, "unsampled is the rest");
    }

    #[test]
    fn a_name_keeps_its_membership_when_the_list_is_reordered() {
        let all = names(100);
        let mut reversed = all.clone();
        reversed.reverse();
        let picked = |list: &[String]| -> Vec<String> {
            let mut v: Vec<String> = sample(list, 10, Seed(3)).into_iter().map(|i| list[i].clone()).collect();
            v.sort();
            v
        };
        assert_eq!(picked(&all), picked(&reversed));
    }

    #[test]
    fn under_the_cap_everything_runs() {
        assert_eq!(sample(&names(5), 30, Seed(1)), vec![0, 1, 2, 3, 4]);
        assert_eq!(sample(&names(30), 30, Seed(1)).len(), 30);
        assert!(sample(&[], 30, Seed(1)).is_empty());
    }

    #[test]
    fn jobs_are_half_the_cpus_bounded_by_80_percent_of_free_over_a_copy_and_at_least_one() {
        const GB: u64 = 1_000_000_000;
        let n = |cpus, free: Option<u64>, per: Option<u64>| jobs(cpus, free, per).count();
        assert_eq!(n(16, Some(100 * GB), Some(2 * GB)), 8, "the CPU bound");
        // The vaultwarden case: 6.6 GB free, 2 GB a copy: 5.28 / 2 = 2, not the 4 one-per-1.5-GB gave.
        assert_eq!(n(12, Some(6_600_000_000), Some(2 * GB)), 2);
        assert_eq!(n(12, Some(10 * GB), Some(2 * GB)), 4, "floor(8 / 2)");
        assert_eq!(n(12, Some(10 * GB), Some(2 * GB + 1)), 3, "floor, never round up");
        assert_eq!(n(16, Some(10 * GB), None), 5, "unmeasured: 1.5 GB a copy, floor(8 / 1.5)");
        assert_eq!(n(16, Some(10 * GB), Some(0)), 5, "a zero measurement is no measurement");
        assert_eq!(n(16, Some(GB), Some(2 * GB)), 1, "never below one");
        assert_eq!(n(1, Some(100 * GB), None), 1);
        assert_eq!(n(10, None, Some(GB)), 5, "unknown free space bounds nothing");
        assert_eq!(n(usize::MAX, Some(u64::MAX), Some(1)), u32::MAX, "no overflow");
        assert!(matches!(jobs(8, Some(GB), Some(0)), Jobs::Sized { measured: false, per_job: BYTES_PER_JOB, .. }));
    }

    #[test]
    fn the_jobs_line_says_the_count_and_why() {
        let sized = jobs(12, Some(6_600_000_000), Some(2_100_000_000));
        assert_eq!(sized.line(), "2: min(half of 12 CPUs, 80% of 6.6 GB free / 2.1 GB per copy, measured), at least 1");
        assert!(jobs(12, None, None).line().contains("unknown free / 1.5 GB per copy, not measured: assumed"));
        assert_eq!(Limits { max_mutants: 1, jobs: Some(3) }.jobs_for(Path::new("."), None), Jobs::Configured(3));
        assert_eq!(Jobs::Configured(3).line(), "3 (set by `jobs` in proob/config.toml)");
    }

    #[test]
    fn config_limits_refuse_zero() {
        let a = |max_mutants, jobs| AdapterConfig {
            root: ".".into(),
            language: Language::Rust,
            runner: "r".into(),
            mutation_tool: "m".into(),
            max_mutants,
            jobs,
            features: Vec::new(),
            all_features: None,
            no_default_features: None,
        };
        assert_eq!(limits(&a(None, None), Language::Rust), Ok(Limits::DEFAULT));
        assert_eq!(limits(&a(Some(5), Some(2)), Language::Rust), Ok(Limits { max_mutants: 5, jobs: Some(2) }));
        assert!(limits(&a(Some(0), None), Language::Rust).is_err());
        assert!(limits(&a(None, Some(0)), Language::Rust).is_err());
    }
}
