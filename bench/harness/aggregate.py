"""Aggregate blind trials into the benchmark's measure 1 table.

usage: python3 -I aggregate.py <bugs.json> <runs dir>
Detection per bug over its trials, overall rate with a 95% Wilson interval,
median cost and active time per trial. Active time is the claude session's own
duration minus the idle wait some runs spent on a stray background task: the
sum of API time is not exposed, so duration_api_ms is used when present.
"""
import glob, json, math, os, statistics, sys

def wilson(k, n, z=1.96):
    if n == 0: return (0.0, 0.0)
    p = k / n; d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d; h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0, c - h), min(1, c + h))

def trial(work):
    j = json.load(open(os.path.join(work, "judge.json")))
    m = json.load(open(os.path.join(work, "meta.json")))
    try: c = json.load(open(os.path.join(work, "claude.json")))
    except ValueError: c = {}
    return {"caught": any(r["caught_real_bug"] for r in j),
            "other": sum((r["buggy"] or {}).get("result") == "counterexample" and not r["caught_real_bug"] for r in j),
            "bound": len(j), "cost": m.get("cost_usd") or 0, "wall": m["wall_s"],
            "api_s": (c.get("duration_api_ms") or 0) / 1000}

def main(bugs, runs):
    rows, k, n = [], 0, 0
    for b in json.load(open(bugs)):
        ts = [trial(w) for w in sorted(glob.glob(os.path.join(runs, "t*", b["id"]))) if os.path.exists(os.path.join(w, "judge.json"))]
        hits = sum(t["caught"] for t in ts); k += hits; n += len(ts)
        rows.append((b["id"], b["language"], b["difficulty_guess"]["level"] if isinstance(b["difficulty_guess"], dict) else b["difficulty_guess"], hits, len(ts), sum(t["other"] for t in ts),
                     statistics.median([t["wall"] for t in ts]) if ts else None, round(sum(t["cost"] for t in ts) / max(len(ts), 1), 2)))
    print("| bug | lang | difficulty | caught | other failures | median wall s | mean cost $ |")
    print("|---|---|---|---|---|---|---|")
    for r in rows: print(f"| {r[0]} | {r[1]} | {r[2]} | {r[3]}/{r[4]} | {r[5]} | {r[6]} | {r[7]} |")
    lo, hi = wilson(k, n)
    bugs_any = sum(r[3] > 0 for r in rows)
    print(f"\ntrials caught: {k}/{n} = {k/n:.0%} (95% Wilson {lo:.0%}-{hi:.0%}); bugs caught in at least one trial: {bugs_any}/{len(rows)}")

if __name__ == "__main__":
    main(*sys.argv[1:])
