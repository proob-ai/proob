# Changelog

All notable changes to proob are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). The `v0` tag moves to the latest
release; customer workflows pin the Action to a full commit SHA with the
version as a comment.

## Unreleased

### Added

- Contributor standards: `CONTRIBUTING.md` (one `master` branch with
  releases as tags, Conventional Commits, the core and detection rules, adapters,
  releases), `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), `SECURITY.md`,
  issue forms, a pull request template, CODEOWNERS, and Dependabot.
- Labels as code (`.github/labels.yml`), synced on push to `master`, and area
  labels on pull requests from their changed paths.
- `cargo xtask fmt`, `cargo xtask clippy`, and `cargo xtask commit-lint`, run
  by `cargo xtask ci` and by CI's new `lint` job; pull requests have every
  commit subject and their title checked.

## v0.1.0 (2026-10-30)

### Added

- Mutation scoring samples at most `max_mutants` mutants per property (default
  30, chosen by the property id) and runs them in parallel; output says
  "killed K of N sampled (M in scope)". indexmap: 700 s to 66 s per property.
- Verdict soundness, scored the SV-COMP way over 240 generated worlds plus the
  scripted simulations (`cargo xtask bench soundness`); a wrong `Holds` fails CI.
- Rust adapter cargo features: `features = [..]`, `all_features`, and
  `no_default_features` in a rust `[[adapters]]` row go on every cargo call
  (check, mutant listing, mutation run). Other languages reject them. They live
  in the governed config, so changing them reads `Re-approve (config amended)`
  until approved. `proob init` adds a commented hint when a `build.rs`
  `compile_error!` or a `[[test]]` `required-features` shows a crate needs them.
- Parallel mutant jobs are sized from a measured copy (the tree plus its
  `target/debug`; 1.5 GB if unmeasured): `min(cpus / 2, 80% of free / copy)`.
  `proob run score` prints the chosen jobs and why.
- Bug set v2: 10 real bugs fixed after the agent model's training cutoff.
- The regression corpus replays every catch (84) with no model, in CI.
- `/proob:try` shows every check's result before any strength score.
- Faster strength scores on real repositories. The rust (cargo-mutants) and
  python (mutmut) adapters list every mutant in scope, then run at most
  `max_mutants` (default 30) chosen from the property id, so a rerun on the
  same tree runs the same sample. The rest are reported as `unsampled`, and
  `proob run score` and `proob shadow report` say "killed K of M sampled
  (S in scope)": the kill floor applies to the sample. Mutants run in
  parallel (`jobs`; default half the CPUs, bounded by free space).
  `/proob:try` shows the check results before it starts any score.
- Shadow mode: `/proob:try` and `proob shadow open|report|adopt|clean`,
  plus the `--shadow` flag. proob works in a private copy under
  `~/.proob/shadow/`; the repository is untouched until `proob shadow adopt`
  creates a branch, and the report shows the steps to make it real.
- `/proob:declare` infers its mode: first run, a requirement given as text or
  an issue, a feature branch (properties for new code on the same branch, a
  superseding property when a requirement changed), or the whole repository.
- The PR check and the session line list new code that no property covers
  ("New code without a property"); informational, never blocking.
  `uncovered_ignore` in `proob/config.toml` leaves paths out.
- The benchmark: method in `bench/README.md` (real-bug detection with
  fix-commit ground truth, time to bug over 30 seeds, SV-COMP verdict
  scoring), the harness and bug set version 1 in `bench/`, and the first
  results in `bench/results/v0.0.1.md`.
- `cargo xtask sim`: the whole lifecycle and a set of adversarial
  never-`Holds` cases against local stand-ins for GitHub, the release
  server, Claude Code's hooks, and (with Docker) GitHub Actions.
- README in English, Simplified Chinese, and Japanese, in standard-readme
  order; `cargo xtask readme-sync` fails when a translation is stale.
- The five skills: `/proob:sense` (the picture of a repository, one cache
  file), `/proob:declare` (plain-language proposals as files, validated,
  opened as a pull request that carries the init files on a first run),
  `/proob:falsify` (one property-based test per property, run through the
  adapter, opened as a pull request whose merge approves the check),
  `/proob:verify` (mutation score, survivors, the proof route for Rust under
  Verus), `/proob:guard` (counts, verification, governance footer, drift and
  re-affirmation).
- Plugin hooks: a one-line status at session start and schema feedback on
  writes under `proob/`; both fail open and never download.
- Team install: the committed `.claude/settings.json` recommendation and the
  teammate walk-through in `skills/guard/reference/team-install.md`.
- The Python adapter: Hypothesis through pytest for `check`, mutmut for
  `score`, bound by `# proob:<ULID>` above the test function. Tool pins:
  pytest 9.1.1, hypothesis 6.169.4, mutmut 3.8.0 (`tools/python-tools.txt`;
  any other mutmut is refused).
- `cargo xtask ci`, the one local command that runs what CI runs.
- The README and the Python demo under `examples/python-service/`.
- The evals workflow (`.github/workflows/evals.yml`): every skill's
  `claude plugin eval` cases on push to `master` and on dispatch; not a
  required check.

### Fixed

- A Python check whose project configures pytest with a plugin that is not
  installed (`addopts = --cov ...` without pytest-cov) now says which plugin
  and how to proceed, instead of "pytest produced no report".

- Rust: a `[[test]]` target declared at the path of an inferred one replaces
  it, as in cargo, so `time`'s `tests/integration/main.rs` binds as binary
  `time::tests`, not `time::integration` (nextest exited 94: no binary matched).

- CI on GitHub runners: tests no longer inherit the runner's `GITHUB_*`
  variables; the action self-test no longer greps another step's summary;
  the dogfood execute jobs can check out a private repository; `evals.yml`
  is valid and skips without `ANTHROPIC_API_KEY`.
- Python witnesses no longer include Hypothesis's explain-phase comments.

## v0.0.1

Pre-release for self-application: proob running on its own repository
before any customer-facing skill.

### Added

- The Cargo workspace and its layering rule (`tools/crate-graph-check.sh`):
  `proob-trusted`, `proob-core`, `proob-facts`, `proob-spawn`,
  `proob-effects`, `proob-adapters`, `proob`.
- The verified core (`proob-core`), checked by Verus under `--no-cheating`
  on every pull request, and its six assertions as `ensures` clauses
  (`crates/proob-core/ASSERTIONS.md`): a statement nobody else approved never
  holds; an unapproved check, config, or workflow never holds; stale evidence
  never holds; the strength floor and the model drift rule; a waiver lapses
  with its issue or its window; eligibility is sound and no verdict is an
  input. The trusted base is two functions (`crates/proob-trusted/README.md`).
- The facts, spawn, and effects crates: file and payload types with no
  verdict among them, child processes with property-derived arguments after
  `--`, GitHub facts, the fact cache, and evidence retrieval from
  default-branch runs.
- The CLI (`validate`, `hash`, `status`, `check`, `run`, `init`, `hook`,
  `action`) and the session hook.
- The Rust adapter (proptest through cargo-nextest, scored with
  cargo-mutants) and the Verus adapter (proof as evidence, mutation-under-proof
  as strength).
- The composite GitHub Action with its `execute` (no token) and `judge`
  (read-only token, no repository code) jobs; the two workflow templates
  `proob init` renders; the release pipeline (cargo-dist, SHA-256 checksums,
  GitHub artifact attestations) and the shim that downloads and verifies the
  binary.
- proob's own declaration: thirteen properties under `proob/properties/`,
  nine proof-backed and four test-backed, with the gate and evidence
  workflows on this repository.
- Verus pin: `release/0.2026.10.26.d17fbd0`, vstd `0.0.0-2026-10-25-0158`,
  Z3 4.16.0, Rust 1.99.1.
