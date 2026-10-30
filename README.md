# proob

[![ci](https://github.com/proob-ai/proob/actions/workflows/ci.yml/badge.svg)](https://github.com/proob-ai/proob/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**English** · [简体中文](README.zh-CN.md) · [日本語](README.ja.md)

**Your tests check the cases someone thought of. proob finds the bugs nobody thought to test for.**

proob is a Claude Code plugin. It reads your code, writes down in plain English what must always be true ("a refund never exceeds the charge"), and attacks each rule with thousands of generated inputs. Your team approves the rules in an ordinary pull request review, and proob re-checks them on every pull request after that. The agent proposes; only your team approves.

![/proob:try on python-semver in Claude Code: five properties, one fails on the real bug](docs/try.gif)

A real `/proob:try` in Claude Code on python-semver, one commit before its maintainers fixed issue #497, with the five-minute wait cut. proob proposed five properties and one failed: asking for the next prerelease can go backwards (`1.2.3-rc.1` with the token `alpha` gives `1.2.3-alpha.1`, which sorts before it). That is the bug the maintainers fixed. Not one file in the repository changed.

## Does it work?

We measure every release against real bugs that escaped real test suites, with methods borrowed from testing research (Defects4J, Klees et al. CCS 2018, SV-COMP), not invented here.

| Test | Result |
|---|---|
| Real bugs fixed **after** the model's training cutoff, so it cannot have memorized them | **9 of 10 caught** |
| Held-out historical bugs, blind trials | **18 of 20 trials** (90%) |
| Unknown bugs found in the then-latest releases of attrs, rich, vaultwarden, python-semver, rust-url, and tomlkit | **12** |
| Times proob said a rule `Holds` when the code broke it, out of 260 scored verdicts | **0** |
| Failures it reported that were real bugs | 50 of 59 (85%) |

Each catch becomes a regression test that CI replays with no model in the loop, and a bound check finds its bug again in under ten seconds. Full numbers: [docs/benchmark.md](docs/benchmark.md).

## Quick start

In Claude Code, inside your repository:

```text
/plugin marketplace add proob-ai/proob
/plugin install proob@proob
/proob:try
```

`/proob:try` works on a private copy under `~/.proob/shadow/`, so your files, your `.git`, and `git status` stay exactly as they were. You need Python with pytest, Hypothesis, and mutmut, or Rust with cargo-nextest and cargo-mutants ([details](docs/guide.md#install)).

If you like what it found, the report ends with three steps to keep it:

1. `proob shadow adopt` puts the rules, their tests, and two workflows on a new branch.
2. Open a pull request. A teammate's review is the approval.
3. Make the `proob gate` check required on your default branch.

## How it works

| Skill | What happens |
|---|---|
| `/proob:sense` | the agent reads your code |
| `/proob:declare` | it proposes rules as plain-English files, in a pull request your team reviews |
| `/proob:falsify` | it writes a property-based test for each approved rule, run with a fixed seed |
| `/proob:verify` | proob mutates the code to prove each test can actually fail |
| `/proob:guard` | every pull request, every night, every session: which rules still hold |

Every rule gets one status: `Holds`, `Checked`, `Not checked`, `Re-approve`, `Broken`, `Proposed`, or `Waived`. `Holds` means approved by a reviewer other than the author, backed by fresh evidence from your default branch, and checked by a test that is proven able to fail. A teammate who only reviews needs nothing but GitHub. The [guide](docs/guide.md) covers each skill, the statuses, team setup, and the CLI.

## Why you can trust a verdict

- **The agent can't approve anything.** Approval is read from GitHub's review records each time a status is shown; nothing in the repository stores it, and a self-merged change never counts.
- **Evidence only comes from your default branch,** from a job that holds no token.
- **The code that decides is formally verified.** The verdict function is proved with [Verus](https://github.com/verus-lang/verus) against [six assertions](crates/proob-core/ASSERTIONS.md) that define `Holds`; the trusted base is [two functions](crates/proob-trusted/README.md).

## Languages

| Language | Check | Strength |
|---|---|---|
| Python | Hypothesis through pytest | mutmut |
| Rust | proptest through cargo-nextest | cargo-mutants |
| Rust under Verus | the verifier | mutation under proof |
| TypeScript | planned: fast-check through vitest | StrykerJS |

In other languages, sense and declare still work and every rule reads `Not checked (no adapter)`. Known limits are in the [guide](docs/guide.md#limits).

## Contributing

Questions go in [Discussions](https://github.com/proob-ai/proob/discussions), bugs in [issues](https://github.com/proob-ai/proob/issues), vulnerabilities to a private advisory ([`SECURITY.md`](SECURITY.md)). Pull requests into `master` are welcome; [`CONTRIBUTING.md`](CONTRIBUTING.md) explains the checks, and `cargo xtask ci` runs them all. After editing this README, update each `README.<language>.md` and its source hash; `cargo xtask readme-sync` confirms they match.

## License

[Apache-2.0](LICENSE) © proob-ai
