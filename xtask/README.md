# xtask

`cargo xtask <command>` is the one entry point for every exec command proob's
CI runs, so a developer's shell and `.github/workflows/ci.yml` run the same
argv. It is a std-only crate in the workspace (`publish = false`, skipped by
dist), reached through the alias in `.cargo/config.toml`. Every command prints
the exact argv before running it, spawns it directly (argv only, no shell),
runs it from the workspace root, and exits non-zero on failure.

```sh
cargo xtask --help
cargo xtask test -- -p proob-core      # arguments after -- go to nextest
cargo xtask ci --dry-run               # the plan, nothing runs
cargo xtask ci                         # everything ci.yml runs, in its order
cargo xtask sim lifecycle honesty      # proob end to end against a fixture GitHub
```

## Commands

| Command | Runs | ci.yml job |
|---|---|---|
| `fmt` | `cargo fmt --check` (style in `rustfmt.toml`; `verus!` bodies are left as written) | lint |
| `clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` | lint |
| `commit-lint [<base>..<head>]` | `git log` over the range (default `origin/master..HEAD`); every subject must follow CONTRIBUTING.md, "Commits": `type(scope)!: description` with an allowed type, at most 72 characters, imperative, no trailing period; no `fixup!`/`squash!`/`amend!` commits; no merge commits | lint (pull requests: `base.sha..head.sha`) |
| `commit-lint --title <title>` | the same rules on one subject line | lint (the pull request title) |
| `build` | `cargo build --locked --workspace` | test |
| `test [-- args]` | `cargo nextest run --locked --workspace --no-fail-fast [args]` | test; kani (the Kani-gated tests, below) |
| `verify` | `cargo verus verify --locked -p proob-core --fwd-verus-args-to roots -- --no-cheating --rlimit 10` | verify (the required check) |
| `trusted-base` | the line count of `crates/proob-trusted/src/*.rs`, as `wc -l` prints it | verify |
| `verify-negative` | `tools/ci/verify-negative.sh`: proves the verify job can fail (several minutes) | verify |
| `crate-graph` | `tools/crate-graph-check.sh`, then `tools/ci/crate-graph-negative.sh` | crate-graph |
| `shim-tests` | `tests/shim/test_shim.sh` | shim |
| `hook-tests` | `tests/hooks/test_hooks.sh` | shim |
| `declaration` | `cargo build --locked -p proob`; `./target/debug/proob validate`; refuses if `GITHUB_TOKEN` or `GH_TOKEN` is set; then, with no GitHub token reachable (see below), `./target/debug/proob status` and `./target/debug/proob status --json`, whose output must contain `"state": "unverified"` and must not contain `"word": "holds"` | declaration |
| `bench replay [entry...]` | `cargo build --locked -p proob`, then `python3 -I bench/harness/replay.py run [entry...]`: every entry of the regression corpus `bench/corpus/` replayed with no LLM; `--list` names the entries, `-j N` sets how many repositories replay at once (bench/README.md, "Regression corpus") | bench-replay |
| `bench soundness` | `cargo build --locked -p sim`, then `./target/debug/sim --score`: `target/sim/verdicts.jsonl` (the last `sim` run's) scored with SV-COMP, `rendered` against `truth`; prints the table, writes `target/sim/soundness.json`, exits 1 on any wrong `Holds`, or when no world reached a correct `Holds` (bench/README.md, "Measure 3") | test |
| `plugin-validate` | `claude plugin validate --strict .`, with `claude` from PATH, else `$HOME/.local/bin/claude` | plugin-validate |
| `sim [scenario...]` | `cargo build --locked -p proob -p sim`, then `./target/debug/sim [scenario...]`; `sim --list` names the scenarios (see below) | test (`sim lifecycle honesty requirement shadow soundness`) |
| `ci` | all of the above in that order; keeps going past failures, prints a summary table (command, ok or failed), exits 1 if any failed | |
| `ci --dry-run` | prints the plan without running anything | |

`verify` and `verify-negative` use `cargo-verus` from PATH when it is there
(CI puts it there after `tools/get-verus.sh`); otherwise they prepend the
directory the script unpacks the pinned release into,
`$HOME/.proob-dev/verus/<version>/verus-<platform>/` (or `$PROOB_VERUS_DIR`),
to the child's PATH. xtask never downloads anything: when neither is present
it fails with a one-line hint to run `tools/get-verus.sh`. The pinned version
is read from that script at build time, so the pin lives in one place.

The `kani` job (not required) installs the Kani release pinned in
`tools/kani.toml`, cached on that pin, with the steps `proob init --kani`
writes for users, then runs `cargo xtask test` three times with
`PROOB_REQUIRE_KANI=1`: the real-Kani twin of the real-Verus test in
`crates/proob/tests/run.rs` (real `verified` and score records render `Holds`,
a real counterexample `Broken`), `-p proob-adapters --test kani` (the
adapter's real checks and scores), and `init --kani`'s build probe in
`crates/proob/tests/init.rs`. Without Kani those tests skip themselves (`test`
runs them, and so `ci`; there is no Kani stage); the variable turns a skip into
a failure.

`declaration` exercises the no-token path: in CI nothing can reach GitHub, so
`status` renders Unverified and never Holds. proob also accepts a token from
`gh auth token`, which a developer signed in to gh would supply, turning the
check live (and spending API calls). So the two `status` steps run with
`target/xtask/no-gh/` first on PATH, holding one program, `gh`, a link to
`false`: proob's fallback finds no login, and the check means the same thing
everywhere. The printed argv shows it (`PATH=target/xtask/no-gh:$PATH …`).

## `sim`: the outside world, simulated

`cargo xtask sim` runs, on this machine, everything about proob that normally
depends on the outside world, against local stand-ins for it. The scenarios
live in `sim/` (a dev-only crate like this one: unpublished, skipped by dist,
no proob crate among its dependencies) and drive the built
`./target/debug/proob`, so they exercise what users run. Each verdict scenario
builds its own temporary git repository with fixed identities and dates,
`PROOB_NOW` fixed, and a fixture GitHub (`PROOB_FIXTURE_DIR`).

| Scenario | What it runs | Needs |
|---|---|---|
| `lifecycle` | One property from `tests/fixtures/rust-repo` through the team's path: declared on a branch (`Proposed`), merged with a second reviewer (`Not checked`), a push evidence run (`Checked (tested, strength pending)`), a scheduled score run (`Checked (tested)`: a test never renders `Holds`), a PR that breaks the code (the gate judge renders `Broken` and exits 1), a self-signed merge (`Checked (tested, self-signed)`), an amended bound test (`Re-approve (check amended)`). Every run is the real `proob action` execute job (cargo-nextest, cargo-mutants, no token) and judge job (git alone on PATH), each in its own checkout, the results artifact copied between them and the evidence artifact served by the fixture GitHub afterwards. | the `test` prerequisites below |
| `honesty` | Worlds that must never render `Holds`, and a control that must: a second property `P`, A's statement with `kind = "proof"`, bound through the fixture's `bindings.json` and served a `verified` proof record and a score (no Verus runs); its twin gives test-kind `A` the same facts and renders `Checked (tested)`. Then an agent's commit pushed directly to main, evidence from a pull-request run, evidence from a non-default branch, a score older than `freshness_days`, a reviewer who lost write access, a killed-0 score, GitHub unreachable (a refused port) and rate-limited (a local endpoint answering 403 with no calls remaining), the last two through proob's real REST client. | nothing beyond git |
| `shadow` | Shadow mode on a copy of `tests/fixtures/py-repo` with no `proob/` and no tokens: `proob shadow open`, a property and its tokened test written into the shadow as `/proob:try` would, a failing `proob --shadow run check` (Hypothesis), `proob shadow report` (headline, replay command, the Make it real steps), a new commit carried over by a second `open`, `proob shadow adopt` (the new branch holds exactly the declaration, the test, the config, both workflows, `.gitignore`, and `.claude/settings.json`; the checkout and branch do not move), `proob shadow clean`. After every step the repository (every path with size and mtime, `.git` included, and `git status --porcelain`) is unchanged; adopt may add only refs, reflogs, and objects. | python3 with the pins in `tools/python-tools.txt` |
| `soundness` | 480 worlds: the full cross product of the kind (test-kind `A`, proof-kind `P` with its statement), approval of the declaring PR (team, self-signed, unreviewed, approved only by a reviewer who also authored a governed commit, approver's access revoked), the check's run (fresh push on main, at an earlier commit, the fresh world read 15 days later, a pull-request run, another branch, none), the score (killed at the floor, killed 0, not scored, every mutant unsampled), and the code (correct, or a planted bug the bound test catches; confirmed with plain `cargo test`); `P`'s proof record is derived from the real check at the same commit (`verified` where it found no counterexample, `proof_failed` where it found one), never chosen. One repository, the real jobs four times (check at the merge and at each head, cargo-mutants once); each world then varies only the fixture GitHub and the clock. No world expects a word: each records `rendered` and `truth`, for `bench soundness`. About two minutes. | the `test` prerequisites below |
| `requirement` | A feature branch on an adopted repository adds a module no property covers: `proob check` lists it under "New code without a property" and exits 0; a property for it on the same branch empties the list; a second branch changes behavior an existing property covers and the gate renders `Broken`; a superseding property with the old file deleted renders `Proposed` and the gate passes. | the Rust toolchain in `rust-toolchain.toml`, cargo-nextest |
| `shim`, `hooks` | `tests/shim/test_shim.sh`, `tests/hooks/test_hooks.sh` | as `shim-tests`, `hook-tests` |
| `workflows` | `act push` runs ci.yml's `crate-graph` and `action-self-test` jobs in Docker (`catthehacker/ubuntu:act-latest`); the image is pulled once if absent, then `--pull=false`; logs under `target/sim/`. Skipped, never failed, when the stand-in is missing: `docker not running` when `docker info` fails, or the reason when the daemon cannot answer an image query in 30 s or cannot pull the image. A job that runs and fails, or exceeds 30 minutes, fails. | Docker, act, network for the image and the toolchain |
| `release` | `dist plan` releases `proob` alone, as archives for the five targets of dist-workspace.toml; skipped without `dist` | dist |

With no names every scenario runs; a summary table follows (scenario, time,
ok/failed/skipped with the reason), and the exit is 1 if any failed. Every
verdict of `lifecycle`, `honesty`, `requirement`, and `soundness` is also a
line of `target/sim/verdicts.jsonl`,
`{"scenario","step","expected","rendered","truth"}` (`expected` is null in
`soundness`), the input `cargo xtask bench soundness` scores with
bench/README.md's SV-COMP scheme. A wrong
verdict is never weakened into a pass: it fails the scenario. `SIM_KEEP=1`
keeps each scenario's temporary world and prints where.

Without Docker the full simulation takes about two minutes once the fixture is
built (`target/sim/rust-repo` keeps its cargo build), so `ci` runs it.

## Local prerequisites

| Needed by | Tool | How |
|---|---|---|
| everything | the toolchain in `rust-toolchain.toml` | rustup installs it on the first `cargo` call |
| `test` | cargo-nextest 0.9.150 | `cargo install cargo-nextest --locked --version 0.9.150` |
| `test` (the Rust adapter's score tests) | cargo-mutants 27.1.0 | `cargo install cargo-mutants --locked --version 27.1.0` |
| `test` (the Python adapter's tests) | pytest, hypothesis, mutmut at the pins in `tools/python-tools.txt`, on PATH | `python -m venv .venv && .venv/bin/pip install -r tools/python-tools.txt`, then put `.venv/bin` on PATH |
| `test` (the Kani-gated tests; skipped without it) | Kani and its nightly, at the pin in `tools/kani.toml` | `cargo install --locked kani-verifier --version 0.68.0 && cargo kani setup` |
| `verify`, `verify-negative` | the pinned Verus release | `tools/get-verus.sh` (fetches, checks the sha256, unpacks; prints the directory) |
| `plugin-validate` | Claude Code | `curl -fsSL https://claude.ai/install.sh \| bash` (installs to `$HOME/.local/bin/claude`) |
| `declaration` | `origin/master` reachable, no `GITHUB_TOKEN`/`GH_TOKEN` in the environment (a gh login is neutralised, see above) | `git fetch origin master`; `env -u GH_TOKEN cargo xtask declaration` if your shell exports one |
| `shim-tests`, `hook-tests` | sh, curl, tar with xz, a sha256 tool | present on macOS and most Linux |
| `sim` (`lifecycle`, `soundness`) | cargo-nextest and cargo-mutants, as for `test` | as above |
| `sim` (`workflows`, `release`) | Docker and act; dist 0.33.1 | each is skipped when absent |
| `bench replay` | git and network for the first clone of each repository; uv and Python 3.13 for the Python entries; cargo-nextest for the Rust entries; a few GB under `~/.cache/proob-bench` (or `$PROOB_BENCH_CACHE`), all of it regenerable | `brew install uv` or `pip install uv` |

## Layout

One concept per module; `plan` is the part worth reading.

- `src/cli.rs`: the argv grammar, `enum Task` and `enum Command`, `parse`.
- `src/commit.rs`: Conventional Commit subjects, `lint_subject` and
  `lint_commit`, pure.
- `src/plan.rs`: `plan(task, tools) -> Vec<Step>`, a pure function. A `Step`
  is a program with its argv and environment additions, a captured command
  with expectations on its stdout, an environment check, or a line count. The
  tests pin every argv, the `verify` environment, and the `ci` order.
- `src/tools.rs`: where `cargo-verus` and `claude` are; the only probing.
- `src/run.rs`: the one module that spawns a process.
- `src/error.rs`: every failure, each one line.

To add a scenario: a function in `sim/src/` returning its `Outcome`, a row
of `SCENARIOS` in `sim/src/main.rs`, and a row in the scenario table above;
verdict scenarios list their steps as `(Expect, step)` pairs.

To add a command: a `Task` variant, its arm in `plan`, a test that pins its
argv, its place in `CI_ORDER` if `ci` should run it, a row in the table above,
and the `cargo xtask <cmd>` line in ci.yml.
