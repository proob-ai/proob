"""Measure 2: for each caught bug, its first credited property, 30 fresh seeds on the buggy tree."""
import glob, json, os, random, statistics, subprocess, sys, time
sys.path.insert(0, os.path.dirname(__file__))
from control import P, bugs, env_for, swap
N = int(sys.argv[1]) if len(sys.argv) > 1 else 30
def ci(xs, n=2000):
    m = sorted(statistics.median(random.choices(xs, k=len(xs))) for _ in range(n)); return m[int(.025*n)], m[int(.975*n)]
for bid, bug in bugs.items():
    pick = next(((w, r) for w in sorted(glob.glob(os.path.abspath(f"runs/t*/{bid}")))
                 for r in json.load(open(os.path.join(w, "judge.json"))) if r["caught_real_bug"]), None)
    if not pick: print(json.dumps({"bug": bid, "caught": False})); continue
    w, r = pick; src = os.path.join(w, "src"); env = env_for(w, bug); swap(src, bug, False)
    runs = []
    for _ in range(N):
        t = time.time()
        o = subprocess.run([P, "run", "check", r["id"], "--json", "--seed", str(random.getrandbits(63))], cwd=src, env=env, capture_output=True, text=True)
        try: hit = json.loads(o.stdout.strip().splitlines()[-1])["outcome"]["result"] == "counterexample"
        except Exception: hit = False
        runs.append((hit, time.time() - t))
    hits = [t for h, t in runs if h]; out = {"bug": bid, "property": r["id"], "seeds": N, "found": len(hits)}
    if hits:
        lo, hi = ci(hits); out |= {"median_s": round(statistics.median(hits), 1), "ci95_s": [round(lo, 1), round(hi, 1)]}
    print(json.dumps(out), flush=True)
