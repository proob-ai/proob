# proob

[![ci](https://github.com/proob-ai/proob/actions/workflows/ci.yml/badge.svg)](https://github.com/proob-ai/proob/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**English** · [简体中文](README.zh-CN.md) · [日本語](README.ja.md)

Team-approved, machine-checked, continuously guarded properties for your codebase.

Your team agrees, in plain English, on what your code must never do; proob checks it on every pull request and tells you the day it stops being true. An agent reads your code and proposes the statements, your team approves them by reviewing a pull request, and every verdict comes from tests and proofs that ran on your default branch.

**On 11 real bugs that escaped the test suites of popular open-source libraries, proob's agent, working blind, caught 10, in 22 of 33 runs; on 10 more, fixed after its model's training cutoff, it caught 9.** The checks it wrote find each bug again in under ten seconds. Run on attrs, rich, and vaultwarden, it found 7 real bugs that are still in their latest releases. Details in [Benchmark](#benchmark). To see what it finds in your own code first, without changing your repository, run [`/proob:try`](#try-it-without-touching-your-repository).

```text
$ claude   # in a checkout of python-semver, one commit before its fix for issue #497
> /proob:try

shadow: 5 properties tried · 1 fails · 4 pass · nothing in your repository was changed

  - Asking for the next version always gives a strictly greater version
    fails: v=parse('0.0.0-1'), part='prerelease', token='0'
    0.0.0-1.next_version('prerelease', '0') gave 0.0.0-0.1
    replay: proob --shadow run check 01M644W0MSK9CBE2136XR777N5 --seed 1793003282928823768
  - Printing a parsed version string gives back exactly that string · passes, test can fail
  - Two versions that compare equal always have the same hash · passes, test can fail
  - Version ordering is antisymmetric and transitive · passes, test can fail
  - Bumping a prerelease or build always yields valid SemVer · passes, test can fail

Make it real:
  0. fix the code or drop the failing property before adopting
  1. proob shadow adopt   puts the properties, tests, and workflows on a new branch
  2. open a pull request; a teammate's review is the approval
  3. make the proob gate check required on main
```

A real `/proob:try` run, shortened: one command, about five minutes and $1.10. proob found the bug the python-semver maintainers later fixed (asking for the next prerelease can go backwards, as in `1.2.3-rc.1` with token `alpha` giving `1.2.3-alpha.1`), and did not change a file in the repository.

## Table of Contents

- [Background](#background)
- [Install](#install)
- [Usage](#usage)
- [Benchmark](#benchmark)
- [How we know it works](#how-we-know-it-works)
- [Supported languages](#supported-languages)
- [Team use](#team-use)
- [Limits](#limits)
- [Contributing](#contributing)
- [License](#license)

## Background

Tests check what someone thought to test. Formal methods can check what must always be true, but they ask for a specialist and drift out of date. proob keeps the part that matters, a short list of properties your team has agreed on, and makes everything else mechanical.

| Stage | Who acts | What happens |
|---|---|---|
| Sense | agent | reads the code and writes its picture of the system to a local cache |
| Declare | agent, then people | proposes plain-language properties as files; your team approves them in a pull request |
| Falsify | agent, then machine | writes one property-based test per property; proob runs it with a fixed seed and budget |
| Verify | machine | measures whether each check can fail, by mutating the code it covers |
| Guard | machine | re-derives every verdict on each pull request, every night, and at the start of each session |

The agent proposes and never approves. Approval is read from GitHub's review records every time a status renders; nothing in the repository stores it.

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
| Property files survive a round trip `01M622RQ7AM90T2YNQHJ6P58PW` | Checked (strength pending, self-signed) | _strength not yet measured; no second reviewer_ | not run |
| The fact cache cannot write a verdict `01M622RQ7BGJ75BQ4B0JARP2KC` | Checked (strength pending, self-signed) | _strength not yet measured; no second reviewer_ | not run |
| A run off the default branch is not evidence `01M622RQ7C066YFA9NDMB7EG6K` | Checked (strength pending, self-signed) | _strength not yet measured; no second reviewer_ | not run |
| Property-derived arguments follow the separator `01M622RQ7D1NN8MNXJA62ABZNB` | Checked (strength pending, self-signed) | _strength not yet measured; no second reviewer_ | not run |

The four test-backed properties read `Checked (self-signed)`: checked on the default branch, approved without a second reviewer because this repository has one maintainer, and waiting for their first nightly strength score. The nine proof-backed properties read `Re-approve` because a commit changed files their proofs depend on without going through a reviewed pull request. That is the rule working: a change nobody reviewed cannot keep an approval. `Holds` needs a second maintainer, and no row here will read it until one exists.

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
- **Python projects:** pytest 9.1.1, Hypothesis 6.169.4, and mutmut 3.8.0 in the environment your tests run in ([`tools/python-tools.txt`](tools/python-tools.txt)).
- **Rust projects:** cargo-nextest and cargo-mutants. For the proof route, the pinned Verus release, which [`tools/get-verus.sh`](tools/get-verus.sh) fetches.

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
| Holds | approved statement, check, and config; positive evidence; check proven able to fail | nothing |
| Checked | as above, but strength not yet measured, or self-signed | wait for the nightly run, or get a second reviewer |
| Not checked | approved, but no check, no adapter, no recent evidence, or GitHub unreachable | bind a check, or wait for the next run |
| Re-approve | statement, check, config, or workflow changed since approval | review the amendment |
| Broken | counterexample or failed proof on this pull request, empty scope, or expired waiver | fix the code, or amend the statement |
| Proposed | never approved | review and merge, or reject |
| Waived until d | approved and failing, with a human waiver and an open issue | track the issue |

A walk-through on a real service, with every line the binary printed: [`examples/python-service/`](examples/python-service/).

### When a requirement changes

Run `/proob:declare` on the branch where you build the feature. It works out what is needed from where you are:

- On a feature branch, it reads the branch's changes. For new code that no property covers, it proposes properties and commits them on the same branch, so your reviewers approve the feature and its properties together.
- If your change breaks an existing property because the requirement itself changed, it writes a replacement property and says which one it supersedes. The reviewer approves the new one and deletes the old. If the code is wrong instead, it says so and proposes nothing.
- Given text, an issue number, or a URL, as in `/proob:declare refunds never exceed the original charge`, it proposes properties for that requirement.

You do not have to remember to run it. When a pull request adds code that no property covers, the proob gate check lists it under "New code without a property" and suggests `/proob:declare`; it never blocks. The session line says the same while you work. To leave paths out of that list, add globs to `uncovered_ignore` in `proob/config.toml`.

### CLI

The skills drive the `proob` binary, which you can also run directly:

```sh
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

## Benchmark

Each release is measured the same way, with methods taken from software testing and verification practice rather than invented here ([method](bench/README.md)):

- **Real bugs detected.** Ground truth is the developers' own fix, as in Defects4J and BugsInPy: a failure counts only if it disappears when the fix is applied.
- **Time to bug.** Many independent seeds per bug, reported as distributions, following Klees et al., "Evaluating Fuzz Testing" (CCS 2018).
- **Verdict soundness.** Verdicts scored with the SV-COMP scheme, where a wrong `Holds` costs sixteen times what a right one earns.

Results for v0.0.1 ([full report](bench/results/v0.0.1.md)), on 11 developer-fixed bugs from 11 libraries, 3 blind trials each:

| Measure | Result |
|---|---|
| Bugs caught, at least once | 10 of 11 |
| Trials that caught their bug | 22 of 33, 67% (95% interval 50% to 80%) |
| Time for a bound check to find the bug | 0.3 s to 8.3 s median, by bug |
| Reported failures that were real bugs | 39 of 44, 89% |
| New bugs found in current releases | 5, in python-semver, rust-url, and tomlkit |
| Cost of the full agent flow | median 7 minutes and $1.85 per trial |

The libraries: more-itertools, tomlkit, humanize, cachetools, python-semver, packaging, bidict (Python); chrono, unicode-segmentation, rust-url, toml (Rust). The one miss, bidict, was missed in all three trials. All fixes date from 2025 and 2026, so the agent's model may have seen some of them; the next bug set adds bugs fixed after its training cutoff.

On well-known codebases ([target benchmark](bench/results/targets-v1.md)): `/proob:try`, run as a new user would, on attrs and rich (Python) and indexmap and vaultwarden (Rust) at pinned commits.

| Measure | Result |
|---|---|
| Properties proposed that got a working test | 35 of 35, in 7 complete trials |
| Real bugs found, still present in the latest releases | 7 distinct: 3 in attrs, 3 in rich, 1 in vaultwarden's two-factor login path |
| Reported failures that were real bugs | 11 of 15; 3 false alarms, 1 disputed |
| Repository changed by a trial | never: byte-identical, `.git` included |
| Cost | median $1.10 per trial |

The Rust targets are incomplete in this version: mutation scoring ran every mutant one at a time, 35 to 40 minutes per indexmap property. Making it faster is the next iteration, after which they are rerun in full.

Every change to proob is measured against the previous version on the bug set's development half, and released only without a significant regression; the held-out half is reported at each release. Each catch also becomes a regression test that CI replays on every change, with no model involved ([method](bench/README.md)).

| Iteration | Change | Development half (6 bugs) | Held-out half (5 bugs) |
|---|---|---|---|
| v0.0.1 | baseline | 10 of 18 trials | 12 of 15 trials |
| dev-1 | state the laws of standard interfaces (mappings, iterators, equality and hashing, round trips) | 14 of 18 trials; the two bugs it targeted went from 1 of 6 to 6 of 6 | measured at the next release |
| v0.1.0 ([results](bench/results/v0.1.0.md)) | dev-1, plus sampled and parallel mutation scoring, cargo features | not rerun | 9 of 10 trials, plus 9 of 10 post-cutoff bugs |

The [v0.1.0-rc1 results](bench/results/v0.1.0-rc1.md) add a second, stricter test: **10 real bugs fixed after the agent model's training cutoff, so it cannot have seen them. proob caught 9, one trial each.** Across every held-out bug, 18 of 20 trials caught their bug (90%, 95% interval 70% to 97%). On 260 generated verdicts, scored the SV-COMP way, proob never said `Holds` where the code did not hold. A strength score on indexmap now takes 66 seconds instead of 700.

## How we know it works

No code path in the plugin or the Action can make a property render as approved because an agent acted. Approval is derived at read time from GitHub's review records. Evidence comes only from Action runs on the default branch, in a job that holds no token. The Action holds no write permission. The hooks are feedback on top of that, and they fail open.

The function that decides is `verdict`, in the crate `proob-core`. The crate that decides is verified with Verus under `--no-cheating`; here is the trusted base: [`crates/proob-trusted/README.md`](crates/proob-trusted/README.md), two functions, `sha256` and `now_secs`. Six assertions define `Holds`, and each is an `ensures` clause checked on every pull request ([`ASSERTIONS.md`](crates/proob-core/ASSERTIONS.md)):

1. `Holds` only when the property's current content was approved by an eligible review; a self-signed merge never counts.
2. `Holds` only when the bound check, `proob/config.toml`, and the evidence workflow are each approved by an eligible review.
3. `Holds` only when the scope matches a file and the latest default-branch evidence is fresh at the current hashes and positive.
4. `Holds` only when a fresh score shows the check kills at least the floor of mutants.
5. `Waived` only with an approved waiver, an open issue, and an unexpired window.
6. Eligibility is sound, a direct push is never approved, and no verdict is ever an input.

For Rust under Verus, strength is measured by mutation-under-proof: proob changes the code under the proof and runs the verifier again. A change the proof still accepts is a specification that cannot tell right from wrong.

`cargo xtask sim` runs the whole lifecycle against local stand-ins for GitHub, the release server, and Claude Code's hooks, and checks that a set of adversarial cases never renders `Holds`.

## Supported languages

| Language | Check | Strength |
|---|---|---|
| Rust | proptest through cargo-nextest | cargo-mutants |
| Rust under Verus | the verifier, on an item marked with the property's id | mutation-under-proof |
| Python | Hypothesis through pytest | mutmut |
| TypeScript | after launch: fast-check through vitest | StrykerJS |
| Go, Java | planned | |

In any other language, sense and declare still work, and every property reads `Not checked (no adapter)`.

## Team use

`proob init` commits a `.claude/settings.json` that recommends the plugin to everyone who opens the repository in Claude Code. A teammate trusts the folder, installs the plugin once (`/plugin install proob@proob`), and from then on sees a one-line status at the start of each session. Reviewing and approving needs no plugin at all. More: [`skills/guard/reference/team-install.md`](skills/guard/reference/team-install.md).

On your default branch, we recommend you:

- require the `proob gate` check, and require branches to be up to date or use a merge queue;
- require one approving review before merging, so nothing reaches the branch unreviewed;
- add a `CODEOWNERS` entry for `proob/` and the two proob workflow files.

`/proob:guard` reports which of these are in place.

## Limits

- This repository has one maintainer, so its own properties can read `Checked (self-signed)` at best.
- The evals that test the skills need an API key, and their scores vary between runs, so they are not a required check.
- GitHub disables scheduled workflows on a public repository after 60 days without activity. When the nightly run stops, scores go stale and `Holds` drops to `Checked`.
- mutmut needs `os.fork`, so Python checks are scored on Linux and macOS only.
- TLA+ is not supported yet.

## Contributing

Questions and ideas go in [Discussions](https://github.com/proob-ai/proob/discussions), bug reports in [GitHub issues](https://github.com/proob-ai/proob/issues), and vulnerabilities to a private advisory ([`SECURITY.md`](SECURITY.md)). Pull requests are welcome, into the `develop` branch. [`CONTRIBUTING.md`](CONTRIBUTING.md) covers the branches, commit messages, the verified core, and the rules for changes that touch detection. Before opening one, run the same checks CI runs:

```sh
cargo xtask ci
```

[`xtask/README.md`](xtask/README.md) lists every command and its prerequisites. Changes are recorded in [`CHANGELOG.md`](CHANGELOG.md). Translations of this README follow the English version: after editing `README.md`, update each `README.<language>.md` and its source hash, and `cargo xtask readme-sync` confirms they match. Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

[Apache-2.0](LICENSE) © proob-ai
