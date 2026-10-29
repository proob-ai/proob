"""Blind run: proob's full agent flow on a real repo at a buggy commit, no bug knowledge.

usage: python3 -I blind.py <bugs.json> <id> <outdir> [model]
Copies the repo at the buggy SHA into a fresh git history (the fix is not reachable),
builds a venv with the library plus proob's pinned Python tools, runs
sense -> declare -> falsify headless with web tools disabled, and records time and cost.
"""
import json, os, shutil, subprocess, sys, time

PROOB = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
PROMPT = """You are a developer on this repository adopting proob. Do the following in order and do not stop between steps:
1. Run /proob:sense on the whole repository.
2. Run /proob:declare. Do not push and do not open a pull request; keep the branch and print the PR body.
3. Treat the proposals as approved and run /proob:falsify all. Do not push and do not open a pull request.
Finish with every JSON line `proob run check` printed, verbatim."""

def sh(cmd, cwd, env=None):
    return subprocess.run(cmd, cwd=cwd, env=env, shell=True, capture_output=True, text=True)

def prepare(bug, work):
    src = os.path.join(work, "src")
    shutil.rmtree(work, ignore_errors=True); os.makedirs(work)
    r = sh(f"git clone -q {bug['repo']} src && git -C src checkout -q {bug['buggy_sha']} && git -C src submodule update -q --init --recursive", work)
    if r.returncode: sys.exit(r.stderr)
    shutil.rmtree(os.path.join(src, ".git"))
    sh("git init -q -b main && git add -A && git -c user.name=dev -c user.email=dev@x commit -qm 'import'", src)
    return src

def extras(bug):
    inst = bug["install"] if isinstance(bug["install"], str) else " && ".join(bug["install"])
    tail = inst.split("-e .", 1)[1].split("&&")[0] if "-e ." in inst else ""
    return " ".join(w for w in tail.split() if w not in ("pytest", "hypothesis"))  # proob pins those

def toolchain_env(bug):
    return {"RUSTUP_TOOLCHAIN": "1.99.1"} if bug["language"] == "rust" else {}

def venv(bug, src, work):
    if bug["language"] != "python":
        return os.environ["PATH"]
    py = bug.get("python_version", "3.12")
    v = os.path.join(work, "venv")
    for c in (f"uv venv -q --python {py} {v}",
              f"VIRTUAL_ENV={v} uv pip install -q -r {PROOB}/tools/python-tools.txt",
              f"VIRTUAL_ENV={v} uv pip install -q -e . {extras(bug)}"):
        r = sh(c, src)
        if r.returncode: sys.exit(c + "\n" + r.stderr)
    return f"{v}/bin:" + os.environ["PATH"]

def main(bugs_path, bug_id, out, model="claude-opus-5-5"):
    bug = next(b for b in json.load(open(bugs_path)) if b["id"] == bug_id)
    work = os.path.abspath(os.path.join(out, bug_id)); src = prepare(bug, work)
    env = dict(os.environ, PATH=venv(bug, src, work), PROOB_BIN=f"{PROOB}/target/debug/proob", **toolchain_env(bug))
    t0 = time.time()
    r = subprocess.run(["claude", "-p", "--plugin-dir", PROOB, "--dangerously-skip-permissions",
                        "--model", model, "--max-budget-usd", "12", "--output-format", "json",
                        "--disallowedTools", "WebFetch", "WebSearch"],
                       input=PROMPT, cwd=src, env=env, capture_output=True, text=True, timeout=5400)
    wall = time.time() - t0
    open(os.path.join(work, "claude.json"), "w").write(r.stdout)
    open(os.path.join(work, "claude.err"), "w").write(r.stderr)
    try: d = json.loads(r.stdout)
    except ValueError: d = {}
    meta = {"id": bug_id, "model": model, "wall_s": round(wall), "cost_usd": d.get("total_cost_usd"),
            "turns": d.get("num_turns"), "rc": r.returncode}
    json.dump(meta, open(os.path.join(work, "meta.json"), "w"), indent=1); print(json.dumps(meta))

if __name__ == "__main__":
    main(*sys.argv[1:])
