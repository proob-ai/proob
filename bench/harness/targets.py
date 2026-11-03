"""Target-repository benchmark (track B): the evaluator's path, /proob:try, on a pinned public repo.

usage: python3 -I targets.py run <target> <trial-dir>
       python3 -I targets.py score <trial-dir>
run copies the repo at its pinned commit into a fresh history (<trial-dir>/repo), builds its
environment, snapshots the copy, runs /proob:try headless with web tools disabled and the
shadow inside the trial dir, snapshots again, and records time, cost, turns, the final message
and the shadow's report.json. score reads those files and writes <trial-dir>/score.json.
Set TARGETS_MIRROR to a directory of clones named by target id to skip the network clone.
"""
import hashlib, json, os, shutil, statistics, subprocess, sys, time

PROOB = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
BIN = os.environ.get("PROOB_BIN", os.path.join(PROOB, "target", "debug", "proob"))
CLAUDE = os.environ.get("CLAUDE_BIN", os.path.expanduser("~/.local/bin/claude"))
MODEL = "claude-opus-5-5"
PROMPT = "/proob:try\n"

def sh(cmd, cwd, env=None):
    return subprocess.run(cmd, cwd=cwd, env=env, shell=True, capture_output=True, text=True)

def target(tid):
    return next(t for t in json.load(open(os.path.join(PROOB, "bench", "targets.json")))["targets"] if t["id"] == tid)

def prepare(t, trial):
    repo = os.path.join(trial, "repo")
    src = os.path.join(os.environ.get("TARGETS_MIRROR", ""), t["id"]) if os.environ.get("TARGETS_MIRROR") else t["repo"]
    r = sh(f"git clone -q {src} repo && git -C repo checkout -q {t['commit']} && git -C repo submodule update -q --init --recursive", trial)
    if r.returncode: sys.exit(r.stderr)
    shutil.rmtree(os.path.join(repo, ".git"))
    sh("git init -q -b main && git add -A && git -c user.name=dev -c user.email=dev@x commit -qm 'import'", repo)
    return repo

def environment(t, repo, trial):
    env = dict(os.environ, **t["env"])
    if t["language"] == "python":
        v = os.path.join(trial, "venv")
        for c in t["install"]:
            c = c.replace("<venv>", v).replace("<proob>", PROOB)
            r = sh(c, repo, dict(env, VIRTUAL_ENV=v))
            if r.returncode: sys.exit(c + "\n" + r.stderr)
        env["PATH"] = f"{v}/bin:" + env["PATH"]
    if "shadow_pythonpath" in t:  # src/ layout: import the shadow's copy, never the user's (product finding)
        env["PYTHONPATH"] = os.path.join(shadow_work(trial, repo), t["shadow_pythonpath"])
    return env

def shadow_work(trial, repo):
    """Where proob will put the shadow's clone: <home>/<name>-<first 8 hex of sha256(canonical root)>/work."""
    root = os.path.realpath(repo)
    return os.path.join(trial, "shadow", f"{os.path.basename(root)}-{hashlib.sha256(root.encode()).hexdigest()[:8]}", "work")

def snapshot(repo):
    """Every byte under the copy, .git included, plus git's view read without taking optional locks
    (a plain `git status` refreshes and rewrites the index, which would itself be a footprint)."""
    files = {}
    for d, _, names in os.walk(repo):
        for n in names:
            p = os.path.join(d, n)
            files[os.path.relpath(p, repo)] = os.readlink(p) if os.path.islink(p) else hashlib.sha256(open(p, "rb").read()).hexdigest()
    env = dict(os.environ, GIT_OPTIONAL_LOCKS="0")
    git = {c: sh(f"git {c}", repo, env).stdout for c in ("rev-parse HEAD", "for-each-ref", "stash list", "status --porcelain=v2 --untracked-files=all --ignored")}
    return {"files": files, "git": git}

def footprint(before, after):
    b, a = before["files"], after["files"]
    return {"zero": before == after, "added": sorted(set(a) - set(b)), "removed": sorted(set(b) - set(a)),
            "changed": sorted(k for k in set(a) & set(b) if a[k] != b[k]),
            "git_changed": sorted(k for k in before["git"] if before["git"][k] != after["git"][k])}

def shadow_dir(trial):
    home = os.path.join(trial, "shadow")
    ds = [os.path.join(home, d) for d in os.listdir(home)] if os.path.isdir(home) else []
    return ds[0] if ds else None

def run(tid, trial):
    t = target(tid); trial = os.path.abspath(trial)
    if os.path.exists(trial): sys.exit(f"{trial} exists; use a fresh trial dir")
    os.makedirs(trial)
    repo = prepare(t, trial); env = environment(t, repo, trial)
    env.update(PROOB_BIN=BIN, PROOB_SHADOW_HOME=os.path.join(trial, "shadow"))
    before = snapshot(repo)
    t0 = time.time()
    try:
        r = subprocess.run([CLAUDE, "-p", "--plugin-dir", PROOB, "--dangerously-skip-permissions", "--model", MODEL,
                            "--max-budget-usd", "15", "--output-format", "json", "--disallowedTools", "WebFetch", "WebSearch"],
                           input=PROMPT, cwd=repo, env=env, capture_output=True, text=True, timeout=4 * 3600)
    except subprocess.TimeoutExpired as e:  # record the trial anyway; its shadow report is still built below
        r = subprocess.CompletedProcess(e.cmd, "timeout", (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or ""), "timed out after 4 h")
    wall = time.time() - t0
    fp = footprint(before, snapshot(repo))
    open(os.path.join(trial, "claude.json"), "w").write(r.stdout)
    open(os.path.join(trial, "claude.err"), "w").write(r.stderr)
    sd = shadow_dir(trial)
    if sd and os.path.isdir(os.path.join(sd, "work")) and not os.path.exists(os.path.join(sd, "report.json")):
        sh(f"{BIN} shadow report --json > /dev/null", os.path.join(sd, "work"), env)  # the agent stopped before its report
    if sd and os.path.exists(os.path.join(sd, "report.json")): shutil.copy(os.path.join(sd, "report.json"), trial)
    try: d = json.loads(r.stdout)
    except ValueError: d = {}
    meta = {"target": tid, "commit": t["commit"], "model": MODEL, "wall_s": round(wall), "cost_usd": d.get("total_cost_usd"),
            "turns": d.get("num_turns"), "rc": r.returncode, "final": d.get("result"), "zero_footprint": fp["zero"]}
    json.dump(fp, open(os.path.join(trial, "footprint.json"), "w"), indent=1)
    json.dump(meta, open(os.path.join(trial, "meta.json"), "w"), indent=1)
    print(json.dumps({k: v for k, v in meta.items() if k != "final"}))

def ratio(s):
    return s["killed"] / s["mutants"] if s["state"] == "can_fail" else 0.0 if s["state"] in ("cannot_fail", "none_killed_in_sample") else None

def score(trial):
    trial = os.path.abspath(trial); meta = json.load(open(os.path.join(trial, "meta.json")))
    p = os.path.join(trial, "report.json")
    props = json.load(open(p))["properties"] if os.path.exists(p) else []
    ratios = [x for x in (ratio(q["strength"]) for q in props) if x is not None]
    fails = [{"id": q["id"], "title": q["title"], "statement": q["statement"], "witness": q["result"]["input"],
              "seed": q["result"].get("seed"),
              "replay": f"proob --shadow run check {q['id']} --json" + (f" --seed {q['result']['seed']}" if q["result"].get("seed") else "")}
             for q in props if q["result"]["state"] == "fails"]
    out = {"target": meta["target"], "trial": os.path.basename(trial), "report": os.path.exists(p),
           "proposed": len(props), "bound": sum(q["result"]["state"] != "no_test" for q in props),
           "scored": len(ratios), "able_to_fail": sum(q["strength"]["state"] == "can_fail" and q["strength"]["killed"] >= 1 for q in props),
           "median_kill_ratio": round(statistics.median(ratios), 3) if ratios else None,
           "errored": sum(q["result"]["state"] == "errored" for q in props), "failures": fails,
           "wall_s": meta["wall_s"], "cost_usd": meta["cost_usd"], "turns": meta["turns"], "zero_footprint": meta["zero_footprint"]}
    json.dump(out, open(os.path.join(trial, "score.json"), "w"), indent=1)
    print(json.dumps({k: v for k, v in out.items() if k != "failures"} | {"failures": len(fails)}))

def selfcheck():
    s = {"files": {"a": "1"}, "git": {"x": ""}}
    assert footprint(s, s)["zero"]
    f = footprint(s, {"files": {"a": "2", "b": "3"}, "git": {"x": "y"}})
    assert not f["zero"] and f["changed"] == ["a"] and f["added"] == ["b"] and f["git_changed"] == ["x"]
    assert shadow_work("/t", "/t/repo").startswith("/t/shadow/repo-") and shadow_work("/t", "/t/repo").endswith("/work")
    assert ratio({"state": "can_fail", "killed": 1, "mutants": 4}) == 0.25 and ratio({"state": "not_scored"}) is None
    assert ratio({"state": "none_killed_in_sample", "mutants": 17, "unsampled": 143}) == 0.0

if __name__ == "__main__":
    selfcheck()
    {"run": run, "score": score}[sys.argv[1]](*sys.argv[2:])
