"""Judge a blind run: which counterexamples are the real bug.

usage: python3 -I judge.py <bugs.json> <id> <outdir>
For every property the agent bound, replays its check on the buggy tree with the recorded
seed, then applies the fix's non-test changes and replays again. A counterexample on the
buggy tree that disappears with the fix is the real bug. Also times each replay.
"""
import glob, json, os, subprocess, sys, time

PROOB = os.environ.get("PROOB_BIN", os.path.join(os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")), "target", "debug", "proob"))

def run(cmd, cwd, env):
    t = time.time(); r = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True)
    return r, time.time() - t

def check(pid, seed, cwd, env):
    r, dt = run([PROOB, "run", "check", pid, "--json"] + (["--seed", seed] if seed else []), cwd, env)
    try: o = json.loads(r.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError): o = {"outcome": {"result": "unparsed", "stderr": r.stderr[-300:]}}
    return o, round(dt, 1)

def apply_fix(bug, src, work):
    fixdir = os.path.join(work, "fix")
    if not os.path.isdir(fixdir):
        subprocess.run(f"git clone -q {bug['repo']} fix && git -C fix checkout -q {bug['fix_sha']}", cwd=work, shell=True, check=True)
    for p in bug["module_paths"]:
        os.makedirs(os.path.dirname(os.path.join(src, p)) or src, exist_ok=True)
        open(os.path.join(src, p), "wb").write(open(os.path.join(fixdir, p), "rb").read())

def main(bugs_path, bug_id, out):
    bug = next(b for b in json.load(open(bugs_path)) if b["id"] == bug_id)
    work = os.path.abspath(os.path.join(out, bug_id)); src = os.path.join(work, "src")
    # blind.py made the venv relative to the repo when given a relative outdir
    found = glob.glob(os.path.join(src, "runs", "*", bug_id, "venv", "bin")) + [os.path.join(work, "venv", "bin")]
    v = next((c for c in found if os.path.isdir(c)), found[-1])
    # an earlier judge pass may have left the fix applied: start from the buggy tree
    subprocess.run(["git", "checkout", "HEAD", "--", *bug["module_paths"]], cwd=src, capture_output=True)
    env = dict(os.environ, PATH=(v + ":" if os.path.isdir(v) else "") + os.environ["PATH"])
    if bug["language"] == "rust": env["RUSTUP_TOOLCHAIN"] = "1.99.1"
    ids = [f[:-5] for f in sorted(os.listdir(os.path.join(src, "proob", "properties"))) if f.endswith(".toml")] \
        if os.path.isdir(os.path.join(src, "proob", "properties")) else []
    rows = []
    for pid in ids:
        o, dt = check(pid, None, src, env)
        rows.append({"id": pid, "buggy": o.get("outcome"), "seed": (o.get("replay") or {}).get("seed"), "t_buggy": dt})
    apply_fix(bug, src, work)
    for row in rows:
        if (row["buggy"] or {}).get("result") in ("counterexample", "no_counterexample"):
            o, dt = check(row["id"], row["seed"], src, env)
            row["fixed"], row["t_fixed"] = o.get("outcome"), dt
        row["caught_real_bug"] = (row["buggy"] or {}).get("result") == "counterexample" and \
                                 (row.get("fixed") or {}).get("result") == "no_counterexample"
    json.dump(rows, open(os.path.join(work, "judge.json"), "w"), indent=1)
    print(json.dumps({"id": bug_id, "bound": len(rows), "counterexamples": sum((r["buggy"] or {}).get("result") == "counterexample" for r in rows),
                      "caught_real_bug": any(r["caught_real_bug"] for r in rows)}))

if __name__ == "__main__":
    main(*sys.argv[1:])
