# proob benchmark

How well and how fast proob catches real bugs in real repositories, measured the same way every release. Every measure here is borrowed from established practice in software testing and verification; none is ours.

## The bug set

Real, historical, developer-fixed bugs from public open-source libraries, in the style of Defects4J (Just, Jalali, Ernst, ISSTA 2014) and BugsInPy (Widyasari et al., ESEC/FSE 2020). Each entry in `bugs.json` records the repository, the buggy commit, the fix commit, the issue, and the regression test the developers added. An entry is admitted only when:

- the project's own test suite passes at the buggy commit (the bug escaped the existing tests);
- the developers' regression test fails at the buggy commit and passes at the fix.

The set is frozen per benchmark version. Bugs are added, never swapped, and a result always names the set version it ran on.

## What counts as caught

A failure counts as catching the bug only if it is a counterexample at the buggy commit **and** it disappears when the fix's non-test changes are applied, replayed with the same seed. A failure that survives the fix is reported separately as "other failure". It is not credited.

## Measure 1: real bugs detected (the headline)

The number of bugs in the set for which proob's full flow catches the bug: `/proob:sense`, `/proob:declare`, then `/proob:falsify`, run by the agent with no knowledge of the bug. The repository is copied at the buggy commit into a fresh history, so the fix is unreachable, and web tools are disabled.

The agent stage is non-deterministic and expensive, so each bug gets `k` independent trials (k = 3 at minimum). Report the per-bug detection rate and the overall rate with a 95% Wilson interval. Report the model id, the cost, and the wall time per trial.

## Measure 2: time to bug (the machine stage)

For every bug that some trial caught, the bound check is replayed with 30 independent seeds. Report the median wall time and the median number of generated cases to the first failure, with 95% confidence intervals, plus the fraction of seeds that found it within the adapter's default budget.

Method: Klees et al., "Evaluating Fuzz Testing" (ACM CCS 2018): real bugs rather than crash counts, many trials, and distributions rather than single runs. Releases are compared per bug with the Mann-Whitney U test and the Vargha-Delaney A₁₂ effect size. The cases-to-failure form follows Etna (Shi et al., ICFP 2023), the evaluation platform for property-based testing.

## Measure 3: verdict soundness

The verdicts proob renders on each benchmark property, at the buggy commit and at the fix, scored with the SV-COMP scheme (the International Competition on Software Verification):

| Verdict | Score |
|---|---|
| Correct `Holds` | +2 |
| Correct `Broken` | +1 |
| Any other verdict | 0 |
| `Broken` at the fix (false alarm) | −16 |
| `Holds` at the buggy commit (unsound) | −32 |

`Holds` is proob's "true" claim and `Broken` its "false" claim. A single unsound `Holds` blocks the release, whatever the total.

### Measured on simulated worlds, every PR

The simulation (`sim/`, xtask/README.md) scores the same way on worlds it builds, so the number exists on every PR and not only after a benchmark run. Each verdict is a line of `target/sim/verdicts.jsonl`, `{"scenario","step","expected","rendered","truth"}`, and the scorer compares `rendered` with `truth`, never with `expected`.

**Ground truth.** `truth` is what the world is, from how it was built, never from proob:

| `truth` | When |
|---|---|
| `violated` | The code under the verdict carries a real counterexample to the property: a planted bug (`sorted` drops its largest element, or keeps one copy of each value) that the bound test catches. |
| `holds` | The code is correct, and the world holds what the claim rests on: an eligible second reviewer approved what is checked, a default-branch check of this very code passed and was a proof or a model (a test samples its inputs, so a test-kind world is at most `unknown`), and a score at this binding inside `freshness_days` killed at least the floor. |
| `unknown` | The code is correct but the world lacks one of those (a test where a proof is needed, no approval, a self-signed or revoked one, evidence from a PR, another branch, an earlier commit or outside the window, a score that killed nothing, GitHub unreadable). |

`Holds` in an `unknown` world is scored as a wrong `Holds` (−32), since the claim says more than the world can back; `Broken` in a `holds` or `unknown` world is a false alarm (−16). In the `soundness` scenario a test's check record is never written by hand: it is what the real evidence job recorded at that commit, and plain `cargo test` confirms each head is the code it says (passes on the correct head, fails on the planted one), so no world is a CI that lies. A proof record is written by the simulation, since CI's `test` job has no Verus: it is derived from that real run on the same code, never chosen (`verified` where the bound test found no counterexample, `proof_failed` where it found one, and the build stops on any other pairing). Every simulated `Holds` therefore rests on a derived proof record, not on a verifier run. The truth of the scripted steps (`lifecycle`, `honesty`, `requirement`) is stated beside each step, from the same table.

**The worlds.** `cargo xtask sim lifecycle honesty requirement shadow soundness` records 269 verdicts: 21 scripted steps and 248 worlds of `soundness`. 240 are the full cross product (not a pairwise subset), for a test-kind property, of

- approval of the declaring PR (5): a second reviewer; the author alone (self-signed is allowed in the fixture); none; a reviewer who also authored a commit of the PR touching governed files; a reviewer who then lost write access;
- the check's run (6): a push run on main at the head a day ago; a push run at the earlier merge commit, before the head edited the scope; the fresh world read 15 days later (outside `freshness_days`); a pull-request run; a run on another branch; none;
- the score (4): the real cargo-mutants record (kills ≥ the floor of 1); every mutant surviving; no score; every mutant unsampled;
- the code (2): correct (a behaviour-preserving edit) or planted.

The other 8 are a proof-kind property with the same statement, where the kind changes the verdict: a second reviewer's approval and a fresh push run, under each score (4) and each code (2). Only one world of the 248 is `holds`, the proof-kind one on correct code with the real score; its test-kind twin renders `Checked (tested)` and scores as other.

The score rides the check's run (with no check run, a scheduled run on main brings it alone). One repository is built and the real jobs run four times; every world then varies only the fixture GitHub and the clock. About a minute.

```sh
cargo xtask sim lifecycle honesty requirement shadow soundness
cargo xtask bench soundness   # the table; target/sim/soundness.json; exit 1 on any wrong Holds
```

CI runs both in the `test` job, so a wrong `Holds` fails the PR. Results per release: [`results/soundness-v1.md`](results/soundness-v1.md).

## Supporting number: mutation score

The kill rate of each bound check under the adapter's mutation tool, reported per bug. Mutation score correlates with real-fault detection (Just et al., "Are Mutants a Valid Substitute for Real Faults in Software Testing?", ESEC/FSE 2014), but it is not detection, so it is never the headline.

A score runs at most `max_mutants` mutants per property (adapter config; default 30 for rust and python, 25 for verus). The rust and python adapters list every mutant in scope first and run a sample chosen from the property id, so a rerun on the same tree runs the same sample; verus runs a prefix by operator kind then position. The mutants left out are reported as `unsampled`, and the kill rate and the kill floor are the sample's, an estimate over the scope rather than an exhaustive count: a result reports it as "killed K of M sampled (S in scope)". Mutants run in parallel, by default half the CPUs and at most one job per 1.5 GB free in the temporary directory, since cargo-mutants copies and builds the tree once per job.

## Regression corpus

Every property that caught a bug in a trial, frozen so a later proob can be held to it with no LLM. `bench/corpus/<entry>/` holds `entry.toml` (the bug, the trial, the property id, the seed, the expected outcomes), `proob/` (the trial's config and that one property file), `agent.patch` (the agent's test changes as a diff against the buggy commit; agents edit existing test files, and in some Rust repositories test modules inside source files, so whole files would carry library source), and, for a Rust repository that commits no `Cargo.lock`, the trial's lock file, so dependency versions stay fixed.

An entry is a **catch** (a credited catch under "What counts as caught": `counterexample` at the buggy commit, `no_counterexample` once the fix's non-test files are applied, same seed) or **live** (a counterexample that survived the fix and was confirmed a real bug; only the buggy side is checked).

```sh
cargo xtask bench replay               # build proob, replay every entry
cargo xtask bench replay --list        # the entries
cargo xtask bench replay <entry>...    # some of them
```

The replayer, `bench/harness/replay.py`, clones each repository once into `~/.cache/proob-bench` (or `$PROOB_BENCH_CACHE`; a few GB, all regenerable), resets the tree to the buggy commit before every entry, overlays the entry, and runs `proob run check <property> --json --seed <seed>`. It prints one line per entry and a summary, and exits 1 on any mismatch. CI runs it in the `bench-replay` job.

To add an entry from a judged blind trial:

```sh
python3 -I bench/harness/replay.py add --from-trial <trial dir> --property <id>          # a credited catch
python3 -I bench/harness/replay.py add --from-trial <trial dir> --property <id> --live   # a confirmed real bug that survives the fix
```

`add` refuses a property that is not a credited catch (or, with `--live`, has no counterexample at the buggy commit), and refuses when the agent edited a file the fix replaces. An entry is marked `flaky = true`, with `flaky_evidence`, only when it disagreed with itself across three back-to-back replays; flaky entries are listed and skipped, never gated on.

## Reporting

`bench/results/<proob-version>.md` holds one table per measure, the bug-set version, the model ids, the tool pins, and the commands that produced it. Each release appends a new file; old files are never edited.
