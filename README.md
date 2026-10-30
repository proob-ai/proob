# proob

[![ci](https://github.com/proob-ai/proob/actions/workflows/ci.yml/badge.svg)](https://github.com/proob-ai/proob/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**English** · [简体中文](README.zh-CN.md) · [日本語](README.ja.md)

**Formal specifications for your codebase: an AI agent writes them, your team approves them, and machines check them on every pull request.**

Tests check the examples someone thought of. Formal methods check what must *always* be true, but they need a specialist, and the specs drift out of date. proob is a Claude Code plugin that brings the second to every team at the cost of a code review.

- **Specs, not more tests.** Each property is a plain-English statement of what must always hold ("a refund never exceeds the charge"), bound to a machine check and approved in a pull request like any other change.
- **Machines decide, never the agent.** Properties are checked against thousands of generated inputs, or proved with [Verus](https://github.com/verus-lang/verus) for Rust, and mutation testing proves each check can actually fail. The agent proposes. It can never approve a property or mark one as holding.
- **proob is formally verified itself.** The code that turns evidence into a verdict is proved with Verus against the [six assertions](crates/proob-core/ASSERTIONS.md) that define `Holds`. It trusts only [two unverified functions](crates/proob-trusted/README.md).

![/proob:try on python-semver in Claude Code: five properties, two fail, one of them the real bug](docs/try.gif)

A real `/proob:try` in Claude Code on python-semver, one commit before its maintainers fixed issue #497, with the five-minute wait cut. proob proposed five properties and two failed. One is the bug the maintainers fixed: asking for the next prerelease can go backwards (`1.2.3-zeta` with the token `rc` gives `1.2.3-rc.1`, which sorts before it). The other, two equal versions with different hashes, shows up only for versions built through the constructor. Not one file in the repository changed.

## Where proob fits

| | Unit tests | Formal methods | proob |
|---|---|---|---|
| What is checked | examples someone wrote | properties that must always hold | properties that must always hold |
| Who writes the spec | nobody; it lives in the test | a specialist | an agent proposes, your team approves in a pull request |
| How it is checked | the examples | a prover | generated inputs, or a proof with Verus; mutation testing shows the check can fail |
| Stays current | while someone maintains it | drifts as the code changes | re-checked on every pull request and every night |

## Does it work?

We measure every release against real bugs that escaped real test suites, with methods borrowed from testing research (Defects4J, Klees et al. CCS 2018, SV-COMP), not invented here.

| Test | Result |
|---|---|
| Real bugs fixed **after** the model's training cutoff, so it cannot have memorized them | **9 of 10 caught** |
| Held-out historical bugs, blind trials | **18 of 20 trials** (90%) |
| Unknown bugs found in the then-latest releases of attrs, rich, vaultwarden, python-semver, rust-url, and tomlkit | **12** |
| Times proob said a property `Holds` when the code broke it, out of 260 scored verdicts | **0** |
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

1. `proob shadow adopt` puts the properties, their tests, and two workflows on a new branch.
2. Open a pull request. A teammate's review is the approval.
3. Make the `proob gate` check required on your default branch.

## How it works

| Skill | What happens |
|---|---|
| `/proob:sense` | the agent reads your code |
| `/proob:declare` | it proposes properties as plain-English files, in a pull request your team reviews |
| `/proob:falsify` | it binds each approved property to a property-based test, or to a Verus proof in Rust |
| `/proob:verify` | proob mutates the code to show each check can actually fail |
| `/proob:guard` | every pull request, every night, every session: which properties still hold |

Every property gets one status: `Holds`, `Checked`, `Not checked`, `Re-approve`, `Broken`, `Proposed`, or `Waived`. `Holds` means approved by a reviewer other than the author, backed by fresh evidence from your default branch, and checked by a test that is proven able to fail. A teammate who only reviews needs nothing but GitHub. The [guide](docs/guide.md) covers each skill, the statuses, team setup, and the CLI.

## Why you can trust a verdict

- **Approval lives in GitHub, not in the repository.** It is read from GitHub's review records every time a status is shown, and a self-merged change never counts.
- **Evidence only comes from your default branch,** produced by a job that holds no token.
- **The verdict is proved, not just tested.** Each of the [six assertions](crates/proob-core/ASSERTIONS.md) is a Verus `ensures` clause, checked under `--no-cheating` on every pull request, and 260 simulated verdicts scored the SV-COMP way never once said `Holds` wrongly.

## Languages

| Language | Check | Strength |
|---|---|---|
| Python | Hypothesis through pytest | mutmut |
| Rust | proptest through cargo-nextest | cargo-mutants |
| Rust under Verus | the verifier | mutation under proof |
| TypeScript | planned: fast-check through vitest | StrykerJS |

In other languages, sense and declare still work and every property reads `Not checked (no adapter)`. Known limits are in the [guide](docs/guide.md#limits).

## Contributing

Questions go in [Discussions](https://github.com/proob-ai/proob/discussions), bugs in [issues](https://github.com/proob-ai/proob/issues), vulnerabilities to a private advisory ([`SECURITY.md`](SECURITY.md)). Pull requests into `master` are welcome; [`CONTRIBUTING.md`](CONTRIBUTING.md) explains the checks, and `cargo xtask ci` runs them all. After editing this README, update each `README.<language>.md` and its source hash; `cargo xtask readme-sync` confirms they match.

## License

[Apache-2.0](LICENSE) © proob-ai
