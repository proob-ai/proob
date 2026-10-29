"""Flakiness control: every credited catch must reproduce on the buggy tree with its own seed,
and pass on the fixed tree with that seed, twice each."""
import glob, json, os, subprocess, sys
P = os.environ.get("PROOB_BIN", os.path.join(os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")), "target", "debug", "proob"))
bugs = {b["id"]: b for b in json.load(open("bench/bugs.json"))}
def env_for(work, bug):
    found = glob.glob(os.path.join(work, "src", "runs", "*", bug["id"], "venv", "bin")) + [os.path.join(work, "venv", "bin")]
    v = next((c for c in found if os.path.isdir(c)), None)
    e = dict(os.environ, PATH=(v + ":" if v else "") + os.environ["PATH"])
    if bug["language"] == "rust": e["RUSTUP_TOOLCHAIN"] = "1.99.1"
    return e
def res(pid, seed, src, env):
    r = subprocess.run([P, "run", "check", pid, "--json", "--seed", seed], cwd=src, env=env, capture_output=True, text=True)
    try: return json.loads(r.stdout.strip().splitlines()[-1])["outcome"]["result"]
    except Exception: return "unparsed"
def swap(src, bug, fixed):
    if fixed:
        fix = os.path.join(os.path.dirname(src), "fix")
        for p in bug["module_paths"]: open(os.path.join(src, p), "wb").write(open(os.path.join(fix, p), "rb").read())
    else:
        subprocess.run(["git", "checkout", "HEAD", "--", *bug["module_paths"]], cwd=src, capture_output=True)
def main():
    ok = bad = 0
    for w in sorted(glob.glob(os.path.abspath("runs/t*/*"))):
        bug = bugs[os.path.basename(w)]; src = os.path.join(w, "src"); env = env_for(w, bug)
        for r in json.load(open(os.path.join(w, "judge.json"))):
            if not r["caught_real_bug"]: continue
            swap(src, bug, False); b = [res(r["id"], r["seed"], src, env) for _ in range(2)]
            swap(src, bug, True); f = [res(r["id"], r["seed"], src, env) for _ in range(2)]
            good = b == ["counterexample"] * 2 and f == ["no_counterexample"] * 2
            ok += good; bad += not good
            if not good: print("UNSTABLE", w.split("/")[-2], bug["id"], r["id"], b, f)
    print(f"stable credited catches: {ok}, unstable: {bad}")

if __name__ == "__main__":
    main()
