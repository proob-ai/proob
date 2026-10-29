# Target benchmark results: version 1

The evaluator's path: `/proob:try` run headless on a pinned public repository, as a new user would run it. Targets: [`bench/targets.json`](../targets.json), version 1. Harness: [`bench/harness/targets.py`](../harness/targets.py). Raw data: [`bench/data/targets-v1/`](../data/targets-v1/). Run on 2026-10-29.

| Setting | Value |
|---|---|
| proob | binary built from master `455c609` |
| Agent model | `claude-opus-5-5`, Claude Code 2.1.312, web tools disabled, budget $15 per trial |
| Trials | 3 each for attrs and rich; 1 for indexmap; 0 complete for vaultwarden (see below) |
| Python tools | pytest 9.1.1, Hypothesis 6.169.4, mutmut 3.8.0, Python 3.13 |
| Rust tools | Rust 1.99.1, cargo-nextest 0.9.150, cargo-mutants 27.1.0 |
| Machine | macOS arm64, 12 cores, shared with another benchmark job; at most one Rust trial and one Python trial at a time |

**Rust has fewer than 3 trials.** indexmap has 1 complete trial and vaultwarden has none. Mutation scoring runs every mutant one after another: proob calls `cargo mutants` with no `--jobs` and no cap on the number of mutants. Every mutant rebuilds the crate and runs its test. That takes 35 to 40 minutes per indexmap property, so one indexmap trial took 2 h 3 min. At that rate the remaining Rust trials would have taken more than 15 hours. They were stopped and will be rerun in full after the speed work. The measured times below are the baseline for that work.

## Per trial

A property is *bound* when it has a check. *Able to fail* counts the scored properties whose test killed at least one mutant. *Kill ratio* is killed divided by viable mutants, and the median is taken over each trial's scored properties. *Footprint* is a byte-for-byte hash of the user's copy, `.git` included, taken before and after the trial.

| Trial | Proposed | Bound | Failures | Scored | Able to fail | Median kill ratio | Wall | Cost | Footprint |
|---|---|---|---|---|---|---|---|---|---|
| attrs-1 | 5 | 5 | 1 | 4 | 4 | 0.278 | 10.3 min | $1.21 | zero |
| attrs-2 | 5 | 5 | 1 | 4 | 4 | 0.246 | 13.2 min | $1.20 | zero |
| attrs-3 | 5 | 5 | 2 | 3 | 3 | 0.272 | 13.4 min | $1.08 | zero |
| rich-1 | 5 | 5 | 3 | 2 | 0 | invalid | 2.2 min | $0.98 | zero |
| rich-2 | 5 | 5 | 2 | 3 | 0 | invalid | 2.5 min | $1.10 | zero |
| rich-3 | 5 | 5 | 4 | 1 | 0 | invalid | 2.8 min | $1.09 | zero |
| indexmap-1 | 5 | 5 | 0 | 2 | 2 | 0.163 | 123 min | $1.86 | zero |
| vaultwarden-1 (killed at 27 min) | 5 | 5 | 1 | 1 | 1 | 0.172 | — | not recorded | git clean |
| attrs-2, first run (killed at 14 min) | 5 | 5 | 1 | 3 | 3 | 0.279 | — | not recorded | git clean |

**Complete trials:**
- 35 of 35 proposed properties were bound to a check, and none of the checks errored.
- The 7 complete trials cost $8.51 in total, about $1.10 for a median trial.
- No complete trial wrote anything to the user's copy.

**Killed trials:** the disk guard killed two trials at 15:00, when free space fell under 2 GB. These were vaultwarden-1 and the first run of attrs-2. attrs-2 was rerun in full, and its first run is kept only for its findings. A killed trial loses its Claude Code JSON (cost and turns) and its "before" snapshot. For these two, "git clean" means `git status --ignored` shows nothing and the refs and stash are unchanged. That is a weaker check than the byte hash.

**rich's mutation scores are invalid: the tool could not trace them.** mutmut depends on textual, which depends on rich, so mutmut imports `rich` while it starts up. That happens before any mutant is loaded. The stats run therefore never reaches a mutated function, mutmut records every mutant as "no tests", and proob reported "test cannot fail: killed 0". That verdict appeared even for a test that plainly calls `set_cell_size`. A rerun by hand gave the same 0 of 268, with and without the editable install. master `32a93fc` now reports such a run as "not scored".

**vaultwarden needs a cargo feature.** proob's Rust adapter cannot pass cargo features, and vaultwarden's `build.rs` refuses to compile without a database feature. The agent found this itself, about 2 minutes in, by reading proob's adapter source. It then turned on `sqlite` in the shadow's `Cargo.toml` (`default = ["sqlite", ...]`), with the comment "proob's runner passes no --features". It did not touch the user's copy.

## Failures

Every counterexample was classified against the library's documentation and its latest release (attrs 26.1.0, rich 15.0.0, vaultwarden main `64ae55c`). Each witness was reproduced against that release. The full list, with witnesses and replay commands, is in [`failures.json`](../data/targets-v1/failures.json).

| Class | Count |
|---|---|
| Real bug, present in the latest release | 11 |
| False alarm: the agent's test was wrong | 3 |
| Disputed: behaviour by design that the docs do not state | 1 |

**3 false alarms among 15 counterexamples: 80% of reported failures were real bugs.** In complete trials only, it is 9 real bugs among 13 counterexamples. The 11 real-bug counterexamples come from 7 distinct bugs:

| Library | Bug | Trials | Input |
|---|---|---|---|
| attrs | `converters.to_bool(0.0)` returns `False`. The docstring says any value not listed raises `ValueError`, but `0.0 == 0` passes the membership test. | 2 of 4 runs | realistic |
| attrs | `filters.exclude(...)` crashes `asdict` with `TypeError: unhashable type: 'dict'` on any class that has an unhashable default, even `exclude("name")`. | 1 of 3 | realistic |
| attrs | `VersionInfo(26, 1, 0, "final") == (26,)` is true, but the two hash differently. | 1 of 3 | realistic, low severity |
| rich | `ratio_distribute(80, [1, 1, 1], [10, 10, 50])` returns `[27, 27, 50]`, which sums to 104, not 80. The docstring says the parts are "guaranteed to sum to total". `table.py` sizes columns with this function. | 2 of 3 | realistic |
| rich | `chop_cells("\x1f", 1)` returns `[]`, so text made only of zero-width characters is lost. | 3 of 3 | realistic |
| rich | `set_cell_size("‍", 1)` returns text whose `cell_len` is 0, not 1. | 1 of 3 | extreme (a lone zero-width joiner) |
| vaultwarden | `obscure_email` uses `String::truncate(2)`, which counts bytes. A local part whose first two characters are not both ASCII panics, e.g. `já…@example.com`. This happens in the two-factor login path. | 1 partial | realistic |

The false alarms are one statement, written by all three rich trials: escaped markup renders back exactly as the original text. Rich's `Text` strips control codes (`\r`, `\x08`) on purpose, and `escape()` only concerns markup. The disputed case is attrs' `evolve(inst)` with no changes. It reruns every converter through `__init__`, so a converter that isn't idempotent changes the value. The docstring calls the result "a copy".

Two Hypothesis groups each held a second, uncredited case whose input broke the function's own precondition. In rich-2 it was a 2-cell character at width 1; in rich-3, all-zero ratios. Both groups also held a real case, and are counted under the real bug.

## Measured times (Rust)

Taken from the agents' session transcripts and from cargo-mutants' `outcomes.json` for each scoring run ([`stages.json`](../data/targets-v1/stages.json)).

| Stage | indexmap-1 | vaultwarden-1 |
|---|---|---|
| Sense, declare, bind, build the tests | 2.5 min | 5.7 min, including the feature workaround |
| 5 bound checks | about 12 s in total | — |
| Baseline build + test, per score | 10 s + 1 s | 63 s + 7 s |
| Per mutant, median: build + test | 5.2 s + 1.0 s = 6.3 s | 7.7 s + 7.3 s = 15.3 s |
| Mutants per property | 396 (`src/map.rs` and `src/inner.rs`), 228 | 30, 47 |
| Scoring one property | 35–40 min (396 mutants), 23 min (228) | about 8 min (30 mutants) |
| Unviable mutants | 99 of 396 (25%) | 1 of 30 |
| Scores finished | 2 of 5 | 1 of 5, then killed |

indexmap's scoring stopped at Claude Code's 2-hour limit on one background command. The agent had put all 5 scores in a single loop and did not restart it. The other 3 properties are reported as "not scored yet". vaultwarden-1 used 2.0 GB for the shadow's `target/` and 2.3 GB for cargo-mutants' copy of the tree, on top of everything else on the disk.

## Product fixes made during this run

Both fixes are on master. Every trial here ran with the binary from `455c609`, which predates both.

- **`a9ef57f` fix(python): checks import the copy under test in src-layout projects.** attrs keeps its code under `src/`. With the recipe's editable install, the shadow's checks imported the user's files, not the shadow's. To work around this, the harness installs attrs non-editable (for its version metadata) and puts the shadow's `src/` first on `PYTHONPATH` (`shadow_pythonpath` in `targets.json`).
- **`32a93fc` fix(python): a run where mutmut traced no mutant to the test is not a score.** This is the rich case above.

## Threats to validity

- **Training-data exposure.** All four projects are popular and older than the model, and the model may have read their code and issues.
- **One trial per Rust target at most.** Nothing about indexmap generalises from a single run, and vaultwarden has only a partial one.
- **Shared machine.** Another benchmark job ran builds at the same time, so wall times are upper bounds.
- **Triage** was done by the benchmark operator, who read the docs and reproduced each case against the latest release. There was no maintainer confirmation.

## Commands

```
python3 -I bench/harness/targets.py run <target> <trial-dir>
python3 -I bench/harness/targets.py score <trial-dir>
```
