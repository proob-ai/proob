# proob benchmark results

The numbers behind the [README](../README.md)'s evidence table, release by release. The method is in [`bench/README.md`](../bench/README.md); each release's full report is in [`bench/results/`](../bench/results/).

Each release is measured the same way, with methods taken from software testing and verification practice rather than invented here ([method](../bench/README.md)):

- **Real bugs detected.** Ground truth is the developers' own fix, as in Defects4J and BugsInPy: a failure counts only if it disappears when the fix is applied.
- **Time to bug.** Many independent seeds per bug, reported as distributions, following Klees et al., "Evaluating Fuzz Testing" (CCS 2018).
- **Verdict soundness.** Verdicts scored with the SV-COMP scheme, where a wrong `Holds` costs sixteen times what a right one earns.

Results for v0.0.1 ([full report](../bench/results/v0.0.1.md)), on 11 developer-fixed bugs from 11 libraries, 3 blind trials each:

| Measure | Result |
|---|---|
| Bugs caught, at least once | 10 of 11 |
| Trials that caught their bug | 22 of 33, 67% (95% interval 50% to 80%) |
| Time for a bound check to find the bug | 0.3 s to 8.3 s median, by bug |
| Reported failures that were real bugs | 39 of 44, 89% |
| New bugs found in current releases | 5, in python-semver, rust-url, and tomlkit |
| Cost of the full agent flow | median 7 minutes and $1.85 per trial |

The libraries: more-itertools, tomlkit, humanize, cachetools, python-semver, packaging, bidict (Python); chrono, unicode-segmentation, rust-url, toml (Rust). The one miss, bidict, was missed in all three trials. All fixes date from 2025 and 2026, so the agent's model may have seen some of them; the next bug set adds bugs fixed after its training cutoff.

On well-known codebases ([target benchmark](../bench/results/targets-v1.md)): `/proob:try`, run as a new user would, on attrs and rich (Python) and indexmap and vaultwarden (Rust) at pinned commits.

| Measure | Result |
|---|---|
| Properties proposed that got a working test | 35 of 35, in 7 complete trials |
| Real bugs found, still present in the latest releases | 7 distinct: 3 in attrs, 3 in rich, 1 in vaultwarden's two-factor login path |
| Reported failures that were real bugs | 11 of 15; 3 false alarms, 1 disputed |
| Repository changed by a trial | never: byte-identical, `.git` included |
| Cost | median $1.10 per trial |

The Rust targets are incomplete in this version: mutation scoring ran every mutant one at a time, 35 to 40 minutes per indexmap property. Making it faster is the next iteration, after which they are rerun in full.

The Kani route on indexmap ([Kani results](../bench/results/kani-v1.md)) is not yet practical on a laptop. Over two trials, 1 of 10 proposed proofs verified, within a bound of 3 entries. With a bound of 4, every harness ran past 30 minutes, and one CBMC process used up to 13 GB of memory. Both trials were stopped for memory. No run that did not finish was reported as passing.

Every change to proob is measured against the previous version on the bug set's development half, and released only without a significant regression; the held-out half is reported at each release. Each catch also becomes a regression test that CI replays on every change, with no model involved ([method](../bench/README.md)).

| Iteration | Change | Development half (6 bugs) | Held-out half (5 bugs) |
|---|---|---|---|
| v0.0.1 | baseline | 10 of 18 trials | 12 of 15 trials |
| dev-1 | state the laws of standard interfaces (mappings, iterators, equality and hashing, round trips) | 14 of 18 trials; the two bugs it targeted went from 1 of 6 to 6 of 6 | measured at the next release |
| v0.1.0 ([results](../bench/results/v0.1.0.md)) | dev-1, plus sampled and parallel mutation scoring, cargo features | not rerun | 9 of 10 trials, plus 9 of 10 post-cutoff bugs |
| v0.2.0 ([results](../bench/results/v0.2.0.md)) | `Holds` reserved for proofs and models; Kani and TLA+ adapters | not rerun | not rerun |
| v0.2.1 ([results](../bench/results/v0.2.1.md)) | `proob shadow open` works when `.gitignore` ignores `.claude/` | not rerun | not rerun |
| v0.2.2 ([results](../bench/results/v0.2.2.md)) | `--project` for a project below the repository root; `proob shadow open` sets up the Python environment | not rerun | not rerun |

The [v0.1.0-rc1 results](../bench/results/v0.1.0-rc1.md) add a second, stricter test: **10 real bugs fixed after the agent model's training cutoff, so it cannot have seen them. proob caught 9, one trial each.** Across every held-out bug, 18 of 20 trials caught their bug (90%, 95% interval 70% to 97%). On 260 generated verdicts, scored the SV-COMP way, proob never said `Holds` where the code did not hold. A strength score on indexmap now takes 66 seconds instead of 700.
