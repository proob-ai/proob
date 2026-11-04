"""Regression corpus: every bug proob has caught, replayed with no LLM.

usage:
  python3 -I replay.py run [--list] [-j N] [entry...]
  python3 -I replay.py add --from-trial <trial dir> --property <id> [--live]

`run` replays each entry of bench/corpus/ (all, or the named ones): clone the
repository at the buggy commit (cached under $PROOB_BENCH_CACHE, default
~/.cache/proob-bench), overlay the entry's proob/ declaration and the agent's
test changes (agent.patch), run `proob run check <id> --json --seed <seed>`
with $PROOB_BIN (default target/debug/proob) and require the expected outcome;
for a catch, apply the fix's non-test files and require no_counterexample with
the same seed. One line per entry, then a summary; exit 1 on any mismatch.
Entries marked `flaky = true` are reported and skipped.

`add` turns one property of a blind trial (bench/harness/blind.py's output dir,
judged by judge.py) into an entry: a credited catch, or with --live a
counterexample that survived the fix and was confirmed a real bug.
"""
import argparse, concurrent.futures, glob, hashlib, json, os, shutil, subprocess, sys, time, tomllib
sys.dont_write_bytecode = True  # no __pycache__ in bench/harness/
sys.path.insert(0, os.path.dirname(__file__))
from blind import extras

# rust_decimal's test-only diesel dependency checks for a MySQL client at build
# time though no test links it (bug set v2); any existing directory satisfies it.
os.environ.setdefault("MYSQLCLIENT_LIB_DIR", os.path.dirname(os.path.abspath(__file__)))
os.environ.setdefault("MYSQLCLIENT_VERSION", "8.0.0")

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
CORPUS = os.path.join(ROOT, "bench", "corpus")
TOOLS = os.path.join(ROOT, "tools", "python-tools.txt")
PROOB = os.environ.get("PROOB_BIN", os.path.join(ROOT, "target", "debug", "proob"))
CACHE = os.path.abspath(os.path.expanduser(os.environ.get("PROOB_BENCH_CACHE", "~/.cache/proob-bench")))
RUST_TOOLCHAIN = "1.99.1"


def sh(argv, cwd=None, env=None, check=True, **kw):
    r = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True, **kw)
    if check and r.returncode:
        raise RuntimeError(f"{' '.join(argv)}: exit {r.returncode}: {(r.stderr or r.stdout)[-400:]}")
    return r


# ---- run ----------------------------------------------------------------

def load(entry_id):
    path = os.path.join(CORPUS, entry_id, "entry.toml")
    with open(path, "rb") as f:
        e = tomllib.load(f)
    return dict(e, id=entry_id, dir=os.path.dirname(path))


def checkout(e):
    """The bug's working tree, cloned once, reset to the buggy commit."""
    work = os.path.join(CACHE, "work", e["bug"])
    if not os.path.isdir(os.path.join(work, ".git")):
        shutil.rmtree(work, ignore_errors=True)
        sh(["git", "clone", "-q", e["repo"], work])
    if sh(["git", "cat-file", "-e", e["buggy_commit"] + "^{commit}"], cwd=work, check=False).returncode:
        sh(["git", "fetch", "-q", "origin"], cwd=work)
    sh(["git", "checkout", "-qf", "--detach", e["buggy_commit"]], cwd=work)
    sh(["git", "submodule", "update", "-q", "--init", "--recursive"], cwd=work)
    reset(work, e)
    return work


def reset(work, e):
    """Back to the buggy commit exactly; ignored files (target/, build metadata) stay."""
    sh(["git", "checkout", "-qf", e["buggy_commit"], "--", "."], cwd=work)
    sh(["git", "clean", "-qfd"], cwd=work)
    shutil.rmtree(os.path.join(work, "proob"), ignore_errors=True)
    if sh(["git", "ls-files", "--error-unmatch", "Cargo.lock"], cwd=work, check=False).returncode:
        try: os.remove(os.path.join(work, "Cargo.lock"))  # an ignored lock from the previous entry
        except FileNotFoundError: pass


def environment(e, work):
    env = dict(os.environ)
    if e["language"] == "rust":
        env["RUSTUP_TOOLCHAIN"] = e.get("toolchain", RUST_TOOLCHAIN)
        return env
    # The venv is keyed by the tool pins, so bumping python-tools.txt rebuilds it.
    key = hashlib.sha256((open(TOOLS).read() + e["python"] + " ".join(e["install"])).encode()).hexdigest()[:12]
    venv = os.path.join(CACHE, "venv", f"{e['bug']}-{key}")
    # A venv links to its interpreter by path; a runner image that updates
    # Python leaves a restored venv dangling, so it is rebuilt.
    if not (os.path.isfile(os.path.join(venv, "ready")) and os.path.exists(os.path.join(venv, "bin", "python"))):
        shutil.rmtree(venv, ignore_errors=True)
        venv_env = dict(env, VIRTUAL_ENV=venv)
        sh(["uv", "venv", "-q", "--python", e["python"], venv], cwd=work)
        sh(["uv", "pip", "install", "-q", "-r", TOOLS], cwd=work, env=venv_env)
        sh(["uv", "pip", "install", "-q", "-e", ".", *extras({"install": e["install"]}).split()], cwd=work, env=venv_env)
        open(os.path.join(venv, "ready"), "w").close()
    env["PATH"] = os.path.join(venv, "bin") + os.pathsep + env["PATH"]
    return env


def overlay(e, work):
    shutil.copytree(os.path.join(e["dir"], "proob"), os.path.join(work, "proob"))
    patch = os.path.join(e["dir"], "agent.patch")
    if os.path.getsize(patch):
        sh(["git", "apply", "--whitespace=nowarn", patch], cwd=work)
    lock = os.path.join(e["dir"], "Cargo.lock")
    if os.path.isfile(lock):
        shutil.copyfile(lock, os.path.join(work, "Cargo.lock"))


def apply_fix(e, work):
    for p in e["fix_paths"]:
        blob = sh(["git", "show", f"{e['fix_commit']}:{p}"], cwd=work).stdout
        with open(os.path.join(work, p), "w") as f:
            f.write(blob)


def check(e, work, env):
    r = subprocess.run([PROOB, "run", "check", e["property"], "--json", "--seed", e["seed"]],
                       cwd=work, env=env, capture_output=True, text=True)
    try:
        return json.loads(r.stdout.strip().splitlines()[-1])["outcome"]["result"]
    except (ValueError, IndexError, KeyError, TypeError):
        return "unparsed: " + (r.stderr.strip().splitlines() or ["no output"])[-1][:200]


def replay(e, work, env):
    """(ok, detail) for one entry; the tree is left reset."""
    try:
        overlay(e, work)
        got = [("buggy", e["expect_buggy"], check(e, work, env))]
        if e["kind"] == "catch":
            apply_fix(e, work)
            got.append(("fixed", e["expect_fixed"], check(e, work, env)))
    finally:
        reset(work, e)
    ok = all(want == have for _, want, have in got)
    detail = "  ".join(f"{side}={have}" + ("" if want == have else f" (expected {want})") for side, want, have in got)
    return ok, detail


def replay_bug(entries):
    """Every entry of one bug, in one working tree, in order."""
    rows = []
    try:
        work = checkout(entries[0]); env = environment(entries[0], work)
    except (RuntimeError, OSError) as err:
        return [(e, False, f"setup failed: {err}", 0.0) for e in entries]
    for e in entries:
        t = time.time()
        try:
            ok, detail = replay(e, work, env)
        except (RuntimeError, OSError) as err:
            ok, detail = False, f"error: {err}"
        rows.append((e, ok, detail, time.time() - t))
    return rows


def run(args):
    names = args.entries or sorted(d for d in os.listdir(CORPUS) if os.path.isfile(os.path.join(CORPUS, d, "entry.toml")))
    entries = [load(n) for n in names]
    if args.list:
        for e in entries:
            print(f"{e['id']}  {e['kind']}  {e['language']}" + ("  flaky" if e.get("flaky") else ""))
        return 0
    if not os.access(PROOB, os.X_OK):
        sys.exit(f"replay: no proob binary at {PROOB} (cargo build -p proob, or set PROOB_BIN)")
    gated = [e for e in entries if not e.get("flaky")]
    by_bug = {}
    for e in gated:
        by_bug.setdefault(e["bug"], []).append(e)
    t0, failed = time.time(), []
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        for rows in pool.map(replay_bug, by_bug.values()):
            for e, ok, detail, dt in rows:
                print(f"{'ok  ' if ok else 'FAIL'}  {e['id']:<62} {e['kind']:<5}  {detail}  {dt:.1f}s", flush=True)
                if not ok: failed.append(e["id"])
    for e in entries:
        if e.get("flaky"):
            print(f"skip  {e['id']:<62} {e['kind']:<5}  flaky: {e.get('flaky_evidence', '')}")
    skipped = len(entries) - len(gated)
    print(f"\n{len(gated)} replayed, {len(gated) - len(failed)} ok, {len(failed)} failed, {skipped} flaky skipped, {time.time() - t0:.0f}s")
    for name in failed:
        print(f"failed: {name}")
    return 1 if failed else 0


# ---- add ----------------------------------------------------------------

def toml_value(v):
    if isinstance(v, bool): return "true" if v else "false"
    if isinstance(v, list): return "[" + ", ".join(json.dumps(x) for x in v) + "]"
    return json.dumps(v)  # a JSON string is a valid TOML basic string


def agent_patch(src, bug):
    """The agent's committed changes outside proob/ and its CI wiring, against the imported buggy tree."""
    root = sh(["git", "rev-list", "--max-parents=0", "HEAD"], cwd=src).stdout.split()[0]
    spec = [".", ":!proob", ":!.github", ":!.claude", ":!.gitignore"]  # proob init wiring, not the test
    touched = sh(["git", "diff", "--name-only", root, "HEAD", "--", *spec], cwd=src).stdout.split()
    clash = sorted(set(touched) & set(bug["module_paths"]))
    if clash:
        sys.exit(f"add: the agent changed the fixed files {clash}; the fix overlay would erase its changes")
    return sh(["git", "diff", "--binary", root, "HEAD", "--", *spec], cwd=src).stdout


def add(args):
    trial = os.path.abspath(args.from_trial); src = os.path.join(trial, "src")
    bug_id = os.path.basename(trial); name = os.path.basename(os.path.dirname(trial))
    sets = sorted(glob.glob(os.path.join(ROOT, "bench", "bugs*.json")))
    bug = next((b for f in sets for b in json.load(open(f)) if b["id"] == bug_id), None)
    if bug is None: sys.exit(f"add: {bug_id} is in no bench/bugs*.json")
    row = next((r for r in json.load(open(os.path.join(trial, "judge.json"))) if r["id"] == args.property), None)
    if row is None: sys.exit(f"add: {args.property} is not in {trial}/judge.json")
    if (row["buggy"] or {}).get("result") != "counterexample":
        sys.exit(f"add: {args.property} found no counterexample on the buggy tree")
    if not args.live and not row["caught_real_bug"]:
        sys.exit(f"add: {args.property} is not a credited catch (it survived the fix); use --live for a confirmed finding")
    kind = "live" if args.live else "catch"
    entry_id = f"{bug_id}-{name}-{args.property.lower()}"
    out = os.path.join(CORPUS, entry_id)
    if os.path.exists(out): sys.exit(f"add: {out} exists")
    fields = {"kind": kind, "bug": bug_id, "trial": name, "language": bug["language"], "repo": bug["repo"],
              "buggy_commit": bug["buggy_sha"]}
    if kind == "catch":
        fields |= {"fix_commit": bug["fix_sha"], "fix_paths": bug["module_paths"]}
    fields |= {"install": bug["install"] if isinstance(bug["install"], list) else [bug["install"]]}
    fields |= {"python": bug.get("python_version", "3.12")} if bug["language"] == "python" else {"toolchain": RUST_TOOLCHAIN}
    fields |= {"property": args.property, "seed": row["seed"], "expect_buggy": "counterexample"}
    if kind == "catch":
        fields |= {"expect_fixed": "no_counterexample"}
    fields |= {"flaky": False}
    patch = agent_patch(src, bug)
    os.makedirs(os.path.join(out, "proob", "properties"))
    shutil.copyfile(os.path.join(src, "proob", "config.toml"), os.path.join(out, "proob", "config.toml"))
    shutil.copyfile(os.path.join(src, "proob", "properties", args.property + ".toml"),
                    os.path.join(out, "proob", "properties", args.property + ".toml"))
    open(os.path.join(out, "agent.patch"), "w").write(patch)
    tracked = sh(["git", "ls-files", "Cargo.lock"], cwd=src).stdout.strip()
    if bug["language"] == "rust" and not tracked and os.path.isfile(os.path.join(src, "Cargo.lock")):
        shutil.copyfile(os.path.join(src, "Cargo.lock"), os.path.join(out, "Cargo.lock"))  # freezes the dependency versions the trial resolved
    with open(os.path.join(out, "entry.toml"), "w") as f:
        f.write("".join(f"{k} = {toml_value(v)}\n" for k, v in fields.items()))
    print(entry_id)
    return 0


def main(argv):
    p = argparse.ArgumentParser(prog="replay.py")
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run"); r.add_argument("--list", action="store_true")
    r.add_argument("-j", "--jobs", type=int, default=4); r.add_argument("entries", nargs="*")
    a = sub.add_parser("add"); a.add_argument("--from-trial", required=True)
    a.add_argument("--property", required=True); a.add_argument("--live", action="store_true")
    args = p.parse_args(argv)
    return run(args) if args.cmd == "run" else add(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
