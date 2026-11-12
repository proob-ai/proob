# Changelog

All notable changes to proob are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). The `v0` tag moves to the latest
release; customer workflows pin the Action to a full commit SHA with the
version as a comment.

## Unreleased

### Added

- Monorepos: `proob init` and `proob shadow open` take `--project
  <language>:<dir>` (`rust` or `python`, repeatable) for a project below the
  repository root. proob still looks for a project by itself at the root
  only; `/proob:try`, `/proob:sense`, and `/proob:declare` now read where the
  projects are and pass the flag, and proob then picks the adapter, the
  runner, the mutation tool, and the workflows' install steps from what is at
  the named directory. Existing files are kept: `init` prints the adapter
  block and the workflow steps to add. `shadow open` adds the block to the
  shadow's config and renders again the workflows it wrote itself; a workflow
  the repository committed, or one edited in the shadow since, is left as it
  is.
- `proob shadow open` sets up proob's own Python environment in the shadow
  (`proob/.cache/python`) for a Python project: the pinned tools, then the
  project's requirement files (`requirements.txt`, `requirements-test.txt`,
  and the like) and the project itself when it is a package. It is made with
  Python 3.12 (the gate's) from `PATH` or from uv, else another `python3`
  the pins install on; a `python3` that already has every pinned tool is
  used as it is. The environment is kept across commits and filled again
  when the pins or a requirement file change. `/proob:try` no longer
  installs anything before a check.

### Changed

- A missing Python tool on a Python older than 3.11, which the pinned
  Hypothesis does not install on, now gets the command that makes proob's own
  environment with a newer Python; the command it gave before could not work
  there.
- `shadow open`'s line for a repository with no project at its root now says
  how to name one in a subdirectory.

## v0.2.1 (2026-11-12)

### Fixed

- `proob shadow open` (and so `/proob:try`) no longer stops with
  `shadow: git add failed: The following paths are ignored by one of your
  .gitignore files:` in a repository whose `.gitignore` ignores `.claude/`,
  `.claude/settings.json`, or `.gitignore` itself.

## v0.2.0 (2026-11-10)

### Added

- `proob doctor`: for each adapter in `proob/config.toml`, whether this
  machine has its tools at the versions proob accepts, with the exact
  install command for the first one missing; free disk; the memory budget.
  It installs nothing, exits 1 when anything is not ready, and takes
  `--json`. A TLC jar other than the pinned release is not ready.
- `/proob:setup`: runs `proob doctor` and installs what is missing, one step
  at a time, each after the user approves it, with its source and size
  shown first; never `sudo`, never a script piped from the network, and no
  shell profile edited without asking.
- TLC is pinned: `tools/tla.toml` names TLA+ 1.7.4, its URL, and its
  SHA-256. The workflows `proob init --tla` writes download that jar, refuse
  it on a digest mismatch, and set `PROOB_TLA2TOOLS`; a kept workflow gets
  the steps printed. This repository's CI runs the real TLC tests on it.
- Models of a design with TLA+: a `tla` adapter binds a `\* proob:<ULID>`
  line above an invariant or temporal property of a `.tla` module (its
  `.cfg` beside it) as a `model`, and runs TLC on a temporary copy with only
  that name checked, one worker, and a fixed fingerprint and seed. TLC's
  output decides: no violation only after "No error has been found"; a
  violation carries the error trace's first and last states; a deadlock, a
  parse or semantic error, a timeout, out of memory, or anything
  unrecognised is errored. Strength is measured on the spec
  (`tla-spec-mutants-v1`). TLC comes only from the jar `PROOB_TLA2TOOLS`
  names, on Java from `PATH`. `proob init --tla` adds the adapter, and `proob validate` rejects a TLA+
  token on a property whose kind is not `model`.
- Skills for the model route: with a `tla` adapter, `/proob:declare` may
  propose `kind = "model"` for a protocol-shaped scope, and `/proob:falsify`
  writes a TLA+ spec under `proob/specs/` citing the code each action
  models, runs TLC, and lists the constants and what the model leaves out
  in the pull request's "Model-checked for" block.
- A memory budget for every runner proob starts: while it runs, proob reads
  the resident memory of its process group every half second and kills the
  group once it passes `PROOB_MEMORY_LIMIT_MB` (default: half the physical
  memory; `0` turns the watch off). A run stopped for memory is errored, and
  a mutant stopped for memory counts as a timeout, never killed or survived.
  CBMC under Kani took 6 to 13 GB per harness (`bench/results/kani-v1.md`).
- A score, and a Kani check, refuse to start with an errored reason when the
  volume they build on has under 2 GB free.
- Proofs on ordinary Rust with Kani 0.68.0: a `kani` adapter binds a
  `#[kani::proof]` harness as a `proof`, so a property on plain Rust code
  can reach `Holds` with no proof language. A run that verifies is reported
  within the harness's bounds; a counterexample is `Broken`; anything Kani
  cannot complete is errored with its reason, never positive, and so is a
  harness whose own assertions are unreachable or absent. Strength is
  measured by mutation under Kani, where only a failed assertion of the
  harness's own is a kill. `proob init --kani` opts a repository in (the
  adapter, a pinned and cached Kani install in both workflows, and a build
  probe); `/proob:declare` proposes proofs when the probe passed, and
  `/proob:falsify` writes the harness, lists its bounds in the pull request,
  and falls back to a property-based test, quoting Kani's reason, only when
  Kani cannot take the property. CI runs the real Kani route in its own job.
- Contributor standards: `CONTRIBUTING.md` (one `master` branch with
  releases as tags, Conventional Commits, the core and detection rules, adapters,
  releases), `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), `SECURITY.md`,
  issue forms, a pull request template, CODEOWNERS, and Dependabot.
- Labels as code (`.github/labels.yml`), synced on push to `master`, and area
  labels on pull requests from their changed paths.
- `cargo xtask fmt`, `cargo xtask clippy`, and `cargo xtask commit-lint`, run
  by `cargo xtask ci` and by CI's new `lint` job; pull requests have every
  commit subject and their title checked.

### Changed

- `proob shadow report` ranks what it found. Failures come first, most
  severe first, and every line starts with the property's severity;
  `report.json` adds `severity` and `rationale` to each property. Severity
  is set at declare, before any check runs, by the consequence of a
  violation (money or an irreversible action, a security boundary, or lost
  data is critical), and the try summary lists failures in that order.

- **Breaking:** `Holds` is reserved for a property of kind `proof` or
  `model`: approved, proved, shown able to fail. A property-based test
  reaches `Checked (tested)` at best, and every `Checked` on a test-kind
  property carries the `tested` modifier, first:
  `Checked (tested, strength pending, self-signed)`. `status --json` gains a
  boolean `tested` on a `checked` verdict. Test-backed properties now count
  under `checked`, not `holds`, in the status counts, the headline, and the
  Action's outputs (a new `checked` output). A test-backed property that
  passes is gated by the new `[policy] tested` row (default `pass`), so a
  config that left `holds` at its default gates it as before; one that set
  `holds` to `warn` should set `tested` to match. `[policy] checked` keeps covering a
  `Checked` with a defect pending (strength pending or self-signed). The
  soundness simulation now has 480 worlds, every one of them crossed for a
  test-kind and a proof-kind property, and CI's `verify` job reaches `Holds`
  through the real Verus adapter.
- README rewritten around what proob is, the evidence, and a three-command
  quick start; the full usage guide and benchmark tables moved to
  `docs/guide.md` and `docs/benchmark.md`. The README's GIF is recorded by
  `tools/demo/record.sh`.
- **Breaking for older readers:** a proof's evidence record (`verified`)
  gains an optional `checks` field for Kani, beside Verus's `rlimit`; a
  record carries exactly one of them, and a Verus record is unchanged on the
  wire. A proob older than this release cannot read a Kani `verified` record
  and fails the whole retrieval, so once a repository uses Kani the gate, the
  evidence workflow, and the local shim must run this release or newer.

### Fixed

- Release archives carry a signed GitHub build record again. v0.1.0 was
  built while the repository was private, without one, so the launcher
  refused it ("attestation verification failed") for anyone whose `gh` is
  logged in; every v0.2.0 archive has one, which `gh attestation verify`
  checks.

- The Python adapter no longer tells you to install into your own Python.
  It uses its own virtual environment, `proob/.cache/python` (gitignored),
  whenever it exists, and a missing tool's message gives the one command that
  makes it with the pinned tools and your project. The old message pointed at
  `tools/python-tools.txt`, a file only proob's own repository has.

- The Rust and Kani adapters no longer tell you to install into your own
  `~/.cargo/bin`, where a pinned cargo-nextest, cargo-mutants, or Kani would
  replace the version you use. proob looks in its own `~/.proob/tools/rust/bin`
  first and uses `~/.proob/tools/kani` as `KANI_HOME` when you set none, and
  a missing tool's message installs there.

- `/proob:try` fails loudly when it can check nothing. `proob shadow open`
  exits 2 with "proob cannot check this repository" when it finds no Python or
  Rust project at the repository root, before any property is written. When
  properties were written but none has a check result, `proob shadow report`
  leads with "nothing was checked", offers nothing to adopt, and exits 1; a
  property with no check result also blocks adopting the rest. The try skill
  reports only what a check recorded: a bug noticed while reading the code is
  not a finding. On a Go and TypeScript repository the try wrote five Go
  properties it could not check, then reported bugs found by reading, and
  its steps still offered to adopt them.

- The Kani adapter takes cargo features. `features`, `all_features`, and
  `no_default_features` in a kani `[[adapters]]` row go on every cargo call it
  makes (the build probe, `cargo kani` for the check and each mutant, and the
  mutant listing); before, a kani row refused them. On vaultwarden, whose
  `build.rs` refuses to build without a database feature, the probe failed and
  every property fell back to a test, with no way to set `features =
  ["sqlite"]` for Kani. `proob init --kani` probes with the row's features and adds the same
  commented hint as the rust block.

- The shadow report no longer says "test cannot fail" when only a sample of
  the mutants ran and none was killed. The unsampled mutants may be the ones
  the test reads (on vaultwarden, 17 of a file's 160 ran against a test of
  two small functions). Such a score now reads "not shown able to fail",
  says to raise `max_mutants` and rescore, and has the new JSON state
  `none_killed_in_sample` (`mutants`, `unsampled`); `cannot_fail` now means
  every mutant in scope ran.
- Every missing-tool message now names a pinned install command that works
  from any repository: `cargo install --locked cargo-nextest --version
  0.9.150` (and cargo-mutants 27.1.0), and the full pinned set for Python,
  where it used to point at `tools/python-tools.txt`, a file only proob's
  own repository has.
- A runner that timed out is killed with everything it started. Before,
  only the direct child was killed: a grandchild that held its output open
  (CBMC under `cargo kani`, a test binary under `cargo test`) kept running,
  so the timeout never returned and the grandchild kept its memory. On Unix
  each runner now leads its own process group, and a timeout, or SIGINT,
  SIGTERM, or SIGHUP sent to proob, kills the whole group.
- The shadow report no longer shows a property as passing when a later run
  with another seed passed after a counterexample on the same code and
  test; the counterexample is reported.

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
