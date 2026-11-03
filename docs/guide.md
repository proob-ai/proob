# proob guide

Everything the [README](../README.md) leaves out: installing, the skills, the status words, team setup, and why a verdict can be trusted. Benchmark details are in [benchmark.md](benchmark.md).

## Install

In Claude Code:

```text
/plugin marketplace add proob-ai/proob
/plugin install proob@proob
```

Or in one step, on Claude Code 2.1.275 or later:

```text
/plugin install proob --marketplace proob-ai/proob
```

The first time a skill runs, the plugin downloads the `proob` binary for your platform from this repository's GitHub Releases. It checks the SHA-256 against the published `.sha256` file, checks the Sigstore attestation when the release carries one and `gh` is signed in (releases made while this repository is private do not), and caches the binary under `~/.proob/bin/`. A file whose checksum differs is never run. Hooks never download anything. To use a local build instead, set `PROOB_BIN`.

Requirements:

- **GitHub read access**, for a verified status: `gh auth login`, or `GITHUB_TOKEN` in the environment. Without it every status is marked `unverified` and nothing reads `Holds`.
- **Python projects:** pytest 9.1.1, Hypothesis 6.169.4, and mutmut 3.8.0 in the environment your tests run in ([`tools/python-tools.txt`](../tools/python-tools.txt)).
- **Rust projects:** cargo-nextest 0.9.150 and cargo-mutants 27.1.0. For the proof route, the pinned Verus release, which [`tools/get-verus.sh`](../tools/get-verus.sh) fetches, or Kani at the version in [`tools/kani.toml`](../tools/kani.toml).
- **TLA+ models:** Java 11 or later, and `tla2tools.jar` of the TLA+ release in [`tools/tla.toml`](../tools/tla.toml), named by `PROOB_TLA2TOOLS`.

`proob doctor` reports, for each adapter `proob/config.toml` lists, whether this machine has its tools at the versions proob accepts, and the exact command for anything missing; it installs nothing. `/proob:setup` runs it and installs what is missing, one step at a time, each only after you say yes. The workflows `proob init` writes install their own pinned tools in CI.

## Usage

### Try it without touching your repository

```text
/proob:try
```

One command in Claude Code reads your code, proposes properties, writes a test for each, runs them, measures whether each test can fail, and prints a report. All of it happens in a private copy under `~/.proob/shadow/`: your files, your `.git`, and `git status` stay exactly as they were. The copy uses your last commit, so commit anything you want included.

The report ends with the steps to make it real, written out for your repository:

1. `proob shadow adopt` puts the properties, their tests, the config, and the two workflows on a new branch. It never switches your checkout.
2. Push the branch and open a pull request. A teammate's review is the approval.
3. Make the `proob gate` check required on your default branch.

Until then every property reads `Proposed`, because nobody has approved anything. `proob shadow clean` removes the copy.

### Adopt it

Open your repository in Claude Code and run the skills in order:

```text
/proob:sense      read the code; write proob's picture of it to a local cache
/proob:declare    propose properties and open a pull request for your team
/proob:falsify    write one test per approved property and run it
/proob:verify     measure whether each test can fail; explain the survivors
/proob:guard      show where the team stands and what is missing
```

The first `/proob:declare` also runs `proob init`, so its pull request carries `proob/config.toml` and the two workflows that run the checks. Reviewers approve with GitHub alone:

1. Merging approves every file in this pull request as its current content. Only a review by someone other than the author counts as a team approval; a merge without one is self-signed, and proob says so.
2. To change wording, use GitHub's "suggest" on the line and let the author commit the suggestion. You may push your own edit instead, knowing that a file you edited then needs someone else's approval.
3. To reject one proposal without rejecting the rest, reply `reject <ULID>` on this pull request with that proposal's id, or delete the file in the pull request.

Every status uses one of seven words:

| Word | Meaning | What to do |
|---|---|---|
| Holds | approved statement, check, and config; a proof or model verified; check proven able to fail | nothing |
| Checked | as above, but tested rather than proved, strength not yet measured, or self-signed | for a test, nothing; otherwise wait for the nightly run, or get a second reviewer |
| Not checked | approved, but no check, no adapter, no recent evidence, or GitHub unreachable | bind a check, or wait for the next run |
| Re-approve | statement, check, config, or workflow changed since approval | review the amendment |
| Broken | counterexample or failed proof on this pull request, empty scope, or expired waiver | fix the code, or amend the statement |
| Proposed | never approved | review and merge, or reject |
| Waived until d | approved and failing, with a human waiver and an open issue | track the issue |

A walk-through on a real service, with every line the binary printed: [`examples/python-service/`](../examples/python-service/).

### When a requirement changes

Run `/proob:declare` on the branch where you build the feature. It works out what is needed from where you are:

- On a feature branch, it reads the branch's changes. For new code that no property covers, it proposes properties and commits them on the same branch, so your reviewers approve the feature and its properties together.
- If your change breaks an existing property because the requirement itself changed, it writes a replacement property and says which one it supersedes. The reviewer approves the new one and deletes the old. If the code is wrong instead, it says so and proposes nothing.
- Given text, an issue number, or a URL, as in `/proob:declare refunds never exceed the original charge`, it proposes properties for that requirement.

You do not have to remember to run it. When a pull request adds code that no property covers, the proob gate check lists it under "New code without a property" and suggests `/proob:declare`; it never blocks. The session line says the same while you work. To leave paths out of that list, add globs to `uncovered_ignore` in `proob/config.toml`.

### Proofs on ordinary Rust (Kani)

For a Rust crate, proob can prove a property instead of testing it, with no proof language to learn. Opt in once:

```sh
proob init --kani
```

It adds the Kani adapter to `proob/config.toml` and a pinned, cached Kani install to both workflows, then checks that Kani can build the crate at all. In a repository already set up for proob it changes nothing and prints the block and steps to add. When that check passes, `/proob:declare` proposes Rust properties as proofs, and `/proob:falsify` writes a Kani harness for each: a function that gives the code symbolic inputs and asserts the statement for every input it admits.

A crate that builds only with a cargo feature (a `build.rs` that stops with `compile_error!` unless one is on) fails that check. Set the feature on the kani adapter as on the rust one, `features = ["sqlite"]` (or `all_features = true`, `no_default_features = true`), and run `proob init --kani` again to recheck. Every `cargo kani` and `cargo mutants` call of the adapter then carries it. `proob init` leaves a commented hint in both blocks when it sees such a crate. The config is governed, so a change to it reads `Re-approve (config amended)` until it is approved.

A Kani proof holds only within the harness's bounds: the input ranges it assumes, the collection sizes it fixes, and the loop bounds it unwinds. The falsify pull request lists every bound in a "Proved for" block, with the words of the statement it comes from, and flags any bound that narrows the statement. Approving the pull request approves those bounds. The property reaches `Holds` only after that approval, a default-branch run that verifies, and a mutation score at the floor: proob changes the code under proof and reruns the harness, and only a failed assertion of the harness's own counts as a catch.

When Kani cannot take a property (code it does not support, a loop it cannot bound, a timeout, running out of memory), falsify binds a property-based test instead, changes the property's `kind` from `proof` to `test`, and quotes Kani's reason in the pull request; that property reads `Checked (tested)` at best. A counterexample is never a reason to fall back: it is a bug, and the property reads `Broken`.

What a Kani proof does not cover:

- inputs outside the harness's bounds;
- concurrency: Kani compiles threads sequentially;
- aliasing rules (Stacked Borrows) and other undefined behaviour Kani does not check;
- the build itself: Kani compiles with its own nightly toolchain, not yours.

Kani runs on Linux and macOS, not Windows. It downloads CBMC, whose BSD-style licence has an advertising clause.

Kani is heavy. On indexmap, one CBMC process used 6 to 13 GB of memory per harness, and a harness over maps of 4 entries ran past the adapter's 30-minute limit ([Kani results](../bench/results/kani-v1.md)). proob does not yet check free memory before a run or cap CBMC's memory, so run one Kani job at a time on a machine with memory to spare, and keep harness bounds small.

### Models of a design (TLA+)

For a protocol, a retry loop, or a state machine, a property can be a model: an invariant or temporal property of a TLA+ spec, checked by TLC. Opt in once:

```sh
proob init --tla
```

It adds the TLA+ adapter to `proob/config.toml` (`language = "tla"`, `runner = "tlc"`, `mutation_tool = "spec-mutants"`). TLC runs on Java 11 or later, from the `tla2tools.jar` that `PROOB_TLA2TOOLS` names. proob never looks for the jar anywhere else: with the variable unset, or naming no file, or with no `java` on `PATH`, a check is errored and says how to set it. The workflows `proob init` writes download the jar of the release pinned in [`tools/tla.toml`](../tools/tla.toml) (TLA+ 1.7.4), refuse it unless its SHA-256 matches, and set `PROOB_TLA2TOOLS`; a workflow that already exists is kept, and `proob init --tla` prints the steps to add. On your machine, `/proob:setup` does the same with your approval, and `proob doctor` reports a jar other than the pinned release as not ready: TLC's output is what the adapter parses, and it is tested against that release.

A model property declares `kind = "model"` and binds one definition. Write `\* proob:<ULID>` on the line directly above the invariant or property it checks, a definition without parameters at the start of a line (`MutualExclusion == …`), in a `.tla` module whose `.cfg` sits beside it with the same base name. The binding set is the module, its `.cfg`, and every module beside it that it reaches through `EXTENDS` or `INSTANCE`; standard modules such as `Naturals` live in the jar, whose TLC release each result records. `proob validate` rejects a TLA+ token on a property of another kind.

`proob run check` copies those modules into a temporary directory and runs TLC there with one worker, a fixed fingerprint and seed, and a cfg that keeps the original's specification, constants, symmetry, constraints, and `CHECK_DEADLOCK`, but checks only the bound name: as a `PROPERTY` when the cfg lists it under `PROPERTY`, else as an `INVARIANT`. Nothing is written into the repository. TLC's output decides, not its exit code: no violation needs TLC's "No error has been found" after exploring every reachable state; a violation reports the error trace's first and last states; a deadlock (unless the cfg says `CHECK_DEADLOCK FALSE`), a parse or semantic error, a timeout (30 minutes), running out of memory, and anything proob does not recognise are errored, never a pass.

`Holds` on a model means: the approved spec, model-checked by TLC over the approved constants, satisfies the approved property, and mutating the spec shows the property can fail. It is a claim about the design, not about the code. The constants in the `.cfg` are the bounds of that claim, so approving the pull request approves them, as approving a Kani harness approves its bounds.

Strength is measured on the spec, not the code (`tla-spec-mutants-v1`). proob changes the definitions the cfg's `NEXT` or `SPECIFICATION` reaches, never the bound property, the initial predicate, or the cfg: `=` and `#` swapped where nothing is assigned, `<` and `<=`, `>` and `>=`, `+ 1` and `- 1`, one conjunct of a `/\` list dropped, `TRUE` and `FALSE`. A mutant that violates the bound property is killed; one TLC passes survived; one that runs out of time or memory is a timeout; one that does not parse, or deadlocks, says nothing about the property and is excluded. The unmutated spec must pass first, or nothing is scored.

Code that moves away from its spec flags the spec for re-approval: an approved model property whose scope has commits since approval reads `Re-approve (spec drifted: n commits to the modelled scope)`. Drift is file-granular and over-approximate. It cannot miss a change to the scope, and it cannot tell a refactor from a change of behaviour. No method checks that arbitrary code conforms to a TLA+ spec, so proob does not claim one.

What a model does not cover:

- the code: TLC checks the spec, and whether the code follows it is the reviewer's judgement, renewed on every drift;
- states beyond the constants in the `.cfg`;
- definitions written other than at the start of a line, such as infix operators: they are never mutated.

### CLI

The skills drive the `proob` binary, which you can also run directly:

```sh
proob doctor                # what this machine lacks to run each adapter (installs nothing)
proob validate              # check proob's files and binding tokens
proob status                # derive every property's status
proob check --base main --head HEAD   # the gate: status plus policy for this change
proob run check <id>        # run one property's bound check locally
proob run score <id>        # measure whether that check can fail
proob shadow open           # make or refresh the private copy /proob:try uses
proob shadow report         # what was tried, what failed, how to make it real
proob shadow adopt          # put the shadow's work on a new branch of your repository
proob --shadow <command>    # run any command above against the private copy
```

## Team use

`proob init` commits a `.claude/settings.json` that recommends the plugin to everyone who opens the repository in Claude Code. A teammate trusts the folder, installs the plugin once (`/plugin install proob@proob`), and from then on sees a one-line status at the start of each session. Reviewing and approving needs no plugin at all. More: [`skills/guard/reference/team-install.md`](../skills/guard/reference/team-install.md).

On your default branch, we recommend you:

- require the `proob gate` check, and require branches to be up to date or use a merge queue;
- require one approving review before merging, so nothing reaches the branch unreviewed;
- add a `CODEOWNERS` entry for `proob/` and the two proob workflow files.

`/proob:guard` reports which of these are in place.

## proob on itself

proob on itself, rendered live from this repository's review records and default-branch runs:

| Property | Status | Reason | This PR |
|---|---|---|---|
| A statement nobody else approved never holds `01M622RQ71P6ZXCKVVTRCA4DSE` | Re-approve | _check amended_ | not run |
| An unapproved check, config, or workflow never holds `01M622RQ72HFMBG9RTQBMB7YBE` | Re-approve | _check amended_ | not run |
| Stale evidence never holds `01M622RQ73XGJ3PRRVCY53FECA` | Re-approve | _check amended_ | not run |
| A self-signed merge never holds `01M622RQ74SFCQ8Y800YE3XNS9` | Re-approve | _check amended_ | not run |
| A waiver lapses with its issue or its window `01M622RQ75T3CFHXPNAAHR3PPD` | Re-approve | _check amended_ | not run |
| Approval comes only from an eligible review `01M622RQ76XZBSVRNHW3PC3Z5S` | Re-approve | _check amended_ | not run |
| The scope hash ignores file order `01M622RQ778BXGXXT6ZQ54EJFR` | Re-approve | _check amended_ | not run |
| The gate blocks only a broken property this change affects `01M622RQ781KT06FB0MKKCZNPF` | Re-approve | _check amended_ | not run |
| Evidence comes only from the approved workflow on the default branch `01M622RQ791MXW80SMN13TWVSY` | Re-approve | _check amended_ | not run |
| Property files survive a round trip `01M622RQ7AM90T2YNQHJ6P58PW` | Checked (tested, strength pending, self-signed) | _tested on sampled inputs, not proved; strength not yet measured; no second reviewer_ | not run |
| The fact cache cannot write a verdict `01M622RQ7BGJ75BQ4B0JARP2KC` | Checked (tested, strength pending, self-signed) | _tested on sampled inputs, not proved; strength not yet measured; no second reviewer_ | not run |
| A run off the default branch is not evidence `01M622RQ7C066YFA9NDMB7EG6K` | Checked (tested, strength pending, self-signed) | _tested on sampled inputs, not proved; strength not yet measured; no second reviewer_ | not run |
| Property-derived arguments follow the separator `01M622RQ7D1NN8MNXJA62ABZNB` | Checked (tested, strength pending, self-signed) | _tested on sampled inputs, not proved; strength not yet measured; no second reviewer_ | not run |

The four test-backed properties read `Checked (tested, strength pending, self-signed)`: tested on the default branch, which is as far as a test goes, approved without a second reviewer because this repository has one maintainer, and waiting for their first nightly strength score. The nine proof-backed properties read `Re-approve` because a commit changed files their proofs depend on without going through a reviewed pull request. That is the rule working: a change nobody reviewed cannot keep an approval. `Holds` needs a second maintainer, and no row here will read it until one exists.

## Why a verdict can be trusted

No code path in the plugin or the Action can make a property render as approved because an agent acted. Approval is derived at read time from GitHub's review records. Evidence comes only from Action runs on the default branch, in a job that holds no token. The Action holds no write permission. The hooks are feedback on top of that, and they fail open.

The function that decides is `verdict`, in the crate `proob-core`. The crate that decides is verified with Verus under `--no-cheating`; here is the trusted base: [`crates/proob-trusted/README.md`](../crates/proob-trusted/README.md), two functions, `sha256` and `now_secs`. Six assertions define `Holds`, and each is an `ensures` clause checked on every pull request ([`ASSERTIONS.md`](../crates/proob-core/ASSERTIONS.md)):

1. `Holds` only when the property's current content was approved by an eligible review; a self-signed merge never counts.
2. `Holds` only when the bound check, `proob/config.toml`, and the evidence workflow are each approved by an eligible review.
3. `Holds` only when the property's kind is `proof` or `model`, the scope matches a file, and the latest default-branch evidence is fresh at the current hashes and positive; a test's passing run earns `Checked (tested)`, never `Holds`.
4. `Holds` only when a fresh score shows the check kills at least the floor of mutants.
5. `Waived` only with an approved waiver, an open issue, and an unexpired window.
6. Eligibility is sound, a direct push is never approved, and no verdict is ever an input.

For a proof, strength is measured by mutation under proof: proob changes the code under the proof and runs the verifier (Verus) or the harness (Kani) again. A change the proof still accepts is a specification that cannot tell right from wrong.

`cargo xtask sim` runs the whole lifecycle against local stand-ins for GitHub, the release server, and Claude Code's hooks, and checks that a set of adversarial cases never renders `Holds`.

## Limits

- This repository has one maintainer, so its own properties can read `Checked (self-signed)` at best; its test-backed ones, `Checked (tested, self-signed)`.
- The evals that test the skills need an API key, and their scores vary between runs, so they are not a required check.
- GitHub disables scheduled workflows on a public repository after 60 days without activity. When the nightly run stops, scores go stale: `Holds` drops to `Checked (strength pending)`, and `Checked (tested)` to `Checked (tested, strength pending)`.
- mutmut needs `os.fork`, so Python checks are scored on Linux and macOS only.
- A Kani proof holds only within its harness's bounds, and Kani cannot run on Windows (see [Proofs on ordinary Rust](#proofs-on-ordinary-rust-kani)).
- A TLA+ model holds for its spec over the constants in its `.cfg`, not for the code; a change to the modelled code only flags the spec for re-approval (see [Models of a design](#models-of-a-design-tla)). Alloy is not supported yet.
- Every runner proob starts (a test runner, Verus, Kani and CBMC, TLC) is held to a memory budget: once its process group's resident memory passes `PROOB_MEMORY_LIMIT_MB` megabytes, proob kills the group. The default is half the machine's physical memory; `0` turns the budget off. A run stopped for memory is reported as errored with the limit it passed, and a mutant stopped for memory counts as a timeout, never killed or survived. A score, or a Kani check, does not start when the volume it builds on has under 2 GB free.
