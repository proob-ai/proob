# Kani route results: version 1

The Kani route on a real crate: `/proob:try` run headless on indexmap at `41a8708` (the pinned target in [`bench/targets.json`](../targets.json)), with `proob init --kani` taken. Raw data: [`bench/data/kani-v1/`](../data/kani-v1/). Run on 2026-10-31.

| Setting | Value |
|---|---|
| proob | `feat/kani-proof-route`; trial 1 before `ed6f4c5`, trial 2 at `ed6f4c5` with the shadow's Kani mutant cap patched from 10 to 3 for this run only |
| Agent model | `claude-opus-5-5`, web tools disabled, budget $15 per trial |
| Kani | 0.68.0, CBMC 6.11.0, nightly-2026-08-21 |
| Machine | macOS arm64, 32 GiB RAM, 12 to 19 GB of free disk |

**Neither trial finished.** Each was stopped for memory before the agent wrote its report, so there is no cost or footprint record. What follows is the evidence each left in its shadow.

## Per trial

| Trial | Probe | Proposed | Bound as Kani proofs | Verified | Errored | Not run | Harness bounds |
|---|---|---|---|---|---|---|---|
| indexmap-1 | passed | 5 | 5 | 1 (169 checks) | 0 | 4 | maps of at most 3 entries, `unwind(9)` |
| indexmap-2 | passed | 5 | 5 | 0 | 3 | 2 | maps of at most 4 entries, `unwind(10)` |

- **No property was reported as passing without a proof.** All 10 proposed properties were `proof`, not `test`: the agent never fell back to a property-based test, so the fallback rate is 0 of 10. It was never tested either, because no harness failed in a way that calls for one.
- **The one verified property is the smallest.** indexmap-1's verified harness checks range resolution against a length, a helper with no hash table in it. The four map-shaped properties of that trial never got a verdict.
- **Every errored run is reported as errored, with its reason.** In indexmap-2, two harnesses hit the adapter's 30-minute limit. The third was cut off when the watchdog stopped the trial, and was recorded as "unrecognised Kani output: no VERIFICATION line". A run killed by a signal now says that it was killed (see below).

## Memory

**One CBMC process used 6 to 13 GB on every indexmap harness.** The watchdog log samples it every 15 s ([`watchdog.log`](../data/kani-v1/indexmap-2/watchdog.log)). Peaks per ten minutes in indexmap-2:

| From | CBMC peak | Swap peak |
|---|---|---|
| 16:00 | 9.5 GB | 4.4 GB |
| 16:30 | 11.6 GB | 4.2 GB |
| 16:40 | 11.3 GB | 8.2 GB |
| 17:30 | 13.0 GB | 7.0 GB |
| 17:40 | 12.0 GB | 8.6 GB (stopped) |

- **indexmap-1** proved its first property, then started scoring it by mutation under Kani. One mutant run held CBMC at 8.7 GB for over an hour, and the machine ran out of application memory. The trial was killed by hand, CBMC with `kill -9`.
- **That trial also found a bug in proob.** A runner that timed out was killed alone: CBMC, its grandchild, kept the pipes open, so the timeout never returned and CBMC kept its memory. Fixed in `ed6f4c5`: each runner leads its own process group, and a timeout or an interrupt kills the whole group.
- **indexmap-2** ran under a watchdog that stops the trial when swap passes 8 GB, free disk drops under 5 GB, or free memory drops under 20%. Both 30-minute timeouts returned on time and released CBMC's memory, which confirms the fix on a real run. Swap passed 8 GB at 17:46 and the watchdog stopped the trial.

## What this says

- **On a hash map, the bounds an agent picks decide whether Kani finishes.** Going from 3 entries to 4, with a symbolic hasher, took every harness past 30 minutes.
- **Kani can stop a machine that is running other work.** proob does not yet check free memory before a Kani run, cap CBMC's memory, or keep two verifier runs from overlapping. Until it does, the Kani route is not ready for `/proob:try` on a stranger's laptop.
- **The route failed closed.** Over two trials, no unfinished or killed run was ever shown as passing.

## Fixed since

- A Kani run killed by a signal with no verdict is errored as "Kani was killed by a signal before it finished; on a machine short of memory, the system may have stopped CBMC", not as unrecognised output.

## Not measured

- Mutation strength under Kani on a real crate: no score finished.
- The fallback to a property-based test: no trial reached it.
- Cost and wall time per trial: both trials were stopped before Claude Code wrote its result.
