# Contributing to proob

Thanks for helping. proob's one promise is that `Holds` means something, so the
bar for a change is that it keeps that promise and CI can show it does. This
file says how to get a change in: the branches, how to build and check it, how
to write commits, and the extra rules for the core, detection, and adapters.

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
Vulnerabilities are reported privately, never in an issue: see
[SECURITY.md](SECURITY.md).

## Before you start

- **Bugs, false alarms, missed bugs, soundness problems, feature and adapter
  requests** each have an issue form; pick the one that fits.
- **Questions and ideas** go in [Discussions](https://github.com/proob-ai/proob/discussions).
- **A change in behavior** should have an issue a maintainer has marked
  `status: accepted` before you write it, so nobody spends a weekend on a pull
  request that cannot merge. Typos, docs, and tests need no issue.
- Issues labelled `good first issue` are small and well scoped; `help wanted`
  ones are open to anyone.

## Branches

`master` is the only long-lived branch. It is protected: changes land only
through reviewed pull requests with CI green, and releases are tags on it.

1. Fork the repository and branch from `master`: `feat/<topic>`, `fix/<topic>`,
   `docs/<topic>`, and so on.
2. Open the pull request into `master`.
3. Keep the branch current by rebasing on `master`, never by merging it in:
   `master`'s history is linear, and `cargo xtask commit-lint` refuses merge
   commits.

## Build, test, verify

Prerequisites, with the exact versions, are in
[`xtask/README.md`](xtask/README.md#local-prerequisites): the Rust toolchain in
`rust-toolchain.toml` (rustup installs it on the first `cargo` call),
cargo-nextest and cargo-mutants, the Python tools in `tools/python-tools.txt`,
and for the core the pinned Verus release (`tools/get-verus.sh` fetches it and
checks its sha256).

Every command CI runs is a `cargo xtask` command, so your shell and CI run the
same argv:

```sh
cargo xtask build                 # cargo build --locked --workspace
cargo xtask test                  # cargo nextest over the workspace
cargo xtask test -- -p proob-facts
cargo xtask verify                # proob-core under Verus, --no-cheating
cargo xtask fmt                   # cargo fmt --check
cargo xtask clippy                # clippy on every target, warnings denied
cargo xtask commit-lint origin/master..HEAD
cargo xtask sim                   # proob end to end against a simulated GitHub
cargo xtask ci                    # all of it, in CI's order, with a summary table
```

Run `cargo xtask ci` before you open the pull request. If a stage cannot run on
your machine (Docker, Claude Code, network), say which in the pull request.
Format with `cargo fmt`; the style is in `rustfmt.toml`. rustfmt leaves the
inside of `verus! { }` alone, so format Verus code by hand in the style around
it.

Clippy runs with `-D warnings`. Fix a finding rather than silencing it. When a
lint is genuinely wrong for the code, allow that one lint at the narrowest item
with a reason (`#[allow(clippy::x, reason = "...")]`); never allow `clippy::all`.

## Commits

Commits and pull request titles follow
[Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/).
CI checks every commit of a pull request and its title with
`cargo xtask commit-lint`.

```text
type(scope)!: description
```

- **type**, one of:

  | Type | For |
  |---|---|
  | `feat` | a new capability |
  | `fix` | a bug fix |
  | `docs` | documentation only |
  | `refactor` | restructuring with no change in behavior |
  | `perf` | faster or smaller, same behavior |
  | `test` | tests, fixtures, or the simulation only |
  | `ci` | workflows |
  | `build` | the build, dependencies, toolchain pins |
  | `chore` | anything else that ships nothing (formatting, housekeeping) |
  | `bench` | the benchmark, its results, or the regression corpus |
  | `release` | a release commit (maintainers) |
  | `revert` | reverting an earlier commit; the description is the reverted subject |

- **scope**, optional, lowercase: the area the change is in. Use one of
  `core`, `trusted`, `facts`, `effects`, `spawn`, `adapters`, `adapter-rust`,
  `adapter-verus`, `adapter-python`, `cli`, `action`, `skills`, `hooks`,
  `shadow`, `bench`, `sim`, `xtask`, `docs`, `i18n`, `deps`.
- **`!`** marks a breaking change; say what breaks in a `BREAKING CHANGE:`
  footer.
- **description**: imperative mood ("add", not "added" or "adds"), no trailing
  period. The whole subject line is at most 72 characters.
- No `fixup!`, `squash!`, or `amend!` commits in a pull request: squash them
  first (`git rebase -i --autosquash master`).
- The body, after a blank line, says why. Wrap it at 72 columns.

Examples from this repository's history:

```text
fix(adapter-python): name a missing pytest plugin and its fix
feat(xtask): add fmt, clippy, and commit-lint
bench: v0.1.0 results, released 2026-10-30
```

## The verified core (`area: core`)

`crates/proob-core` is checked by Verus under `--no-cheating`: no `assume`, no
`admit`, no `external_body`. `crates/proob-trusted` is the only trusted base,
and `cargo xtask trusted-base` prints its size in CI so growth is visible.

- `cargo xtask verify` must pass. A change that weakens an `ensures` clause is a
  change to what `Holds` means and needs an issue first.
- If a guarantee changes, update
  [`crates/proob-core/ASSERTIONS.md`](crates/proob-core/ASSERTIONS.md) and
  `src/specs.rs` in the same pull request.
- The crate may depend on nothing but `vstd` and `proob-trusted`;
  `cargo xtask crate-graph` enforces the layering.
- proob governs itself: `proob/properties/` declares properties over these
  files, and the gate and evidence workflows check them.

## Changes that touch detection

Detection is what the skills (`declare`, `falsify`) and the adapters do to find
bugs. proob is held to [the benchmark](bench/README.md), so a change here has
three rules:

1. **Tune on the `dev` split only.** Each bug in `bench/bugs.json` has a
   `split`, `dev` or `held-out`. Look at, iterate on, and debug against `dev`
   bugs; the `held-out` half is run only to report a release, so it stays an
   honest estimate.
2. **Never weaken a test to pass.** Not a unit test, not a sim scenario, not a
   corpus entry. A replay that stops finding a counterexample, or a soundness
   world that changes its verdict, is a regression in the change, not in the
   test.
3. **Add every catch to the corpus.** A bug a trial caught becomes a
   `bench/corpus/` entry (`python3 -I bench/harness/replay.py add ...`,
   bench/README.md, "Regression corpus"), and `cargo xtask bench replay`
   replays all of them, with no model, in CI.

Skill changes also add or update an eval case under `evals/`; the evals need an
API key and vary between runs, so they inform review rather than gate it.

## Adding an adapter

An adapter connects one language's tools to proob. The contract is in
[`crates/proob-adapters/src/lib.rs`](crates/proob-adapters/src/lib.rs); the
Python adapter (`src/python/`) is the most recent complete example.

1. Open a **New adapter request** issue first and agree on the tools and their
   versions.
2. Implement the three operations in `crates/proob-adapters/src/<language>/`:
   `discover` (checks carrying a `proob:<ULID>` token and their binding sets),
   `check` (the primary check with an adapter-chosen seed and budget, mapped to
   the closed result type), and `score` (mutation testing of the scope files
   with only the bound check).
3. Start every child process through `argv::Command`; property-derived values
   pass a closed grammar (`argv::Checked`) and go after `--`. Never build a
   shell string.
4. Parse tool output in `crates/proob-facts/src/parsers/`, with captured real
   output as fixtures under `tests/fixtures/`.
5. Pin the tool versions in one place and record them in every result.
6. Add a fixture repository and a sim scenario that drives the adapter end to
   end, and at least one real bug from that language to the benchmark.
7. Update the "Supported languages" table in the README (and its translations).

## Pull requests

The template asks for a summary, the linked issue, the type, and a checklist.
In short:

- `cargo xtask ci` passes; tests cover the change.
- The docs say what the change does; if `README.md` changed, every
  `README.<language>.md` is updated to match, with the new source hash on its
  first line, and `cargo xtask readme-sync` passes. Keep translations faithful:
  translate what changed, nothing more.
- User-visible changes get a `CHANGELOG.md` entry under `## Unreleased`, in
  [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) sections (Added,
  Changed, Fixed, Removed, Security).
- One logical change per pull request. Large changes go in a stack of smaller
  ones.

## Review and merge

- Every pull request needs one approving review from a maintainer, and CI
  green. `.github/CODEOWNERS` routes the review; `crates/proob-core/`,
  `crates/proob-trusted/`, `proob/`, and the workflows always get a
  maintainer's eye.
- Labels: a pull request from a branch of this repository gets its `area:`
  labels from the path labeler. A fork's pull request gets them when a
  maintainer triages it, because that workflow never runs with a write token
  on a fork's code (`pull_request`, not `pull_request_target`).
- Maintainers merge by squash (one commit, the pull request
  title as its subject) or by rebase when every commit stands on its own.
- Review comments are about the change, never the person. A reviewer says
  what must change before merge and what is a suggestion.

## Releases

Maintainers release from `master`:

1. Move the `## Unreleased` entries of `CHANGELOG.md` under the new version and
   date, bump `version` in `Cargo.toml`'s `[workspace.package]`, and commit as
   `release: vX.Y.Z`.
2. Run the benchmark and add `bench/results/<version>.md` (bench/README.md,
   "Reporting"); old results files are never edited.
3. Land both through a pull request into `master`.
4. Tag that commit `vX.Y.Z`; the tag starts `release.yml`, which builds and
   publishes the binaries. Move the `v0` tag to the release.

## License

proob is [Apache-2.0](LICENSE). By contributing, you agree that your
contribution is licensed under it (Apache-2.0, section 5), with no further
terms.
