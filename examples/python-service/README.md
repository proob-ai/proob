# idem: the proob Python demo

A small idempotency-key store with a time to live. A client that retries a
request sends the same key; the service must hand the retry the first
response, never do the work twice, and refuse a different request that reuses
a key. Two modules (`idem/store.py`, `idem/fingerprint.py`), six
example-based tests, pytest configured, Hypothesis available, and nothing that
mentions proob. This is the tree the walk-through starts from.

## Before you start

- Claude Code with the proob plugin installed (`/plugin install proob --marketplace proob-ai/proob`).
- Python 3.10 or later, with proob's pinned tools in the environment pytest
  runs in: `pytest==9.1.1 hypothesis==6.169.4 mutmut==3.8.0` (the pins in
  proob's `tools/python-tools.txt`; any other mutmut is refused).
- A copy of this directory as its own repository on GitHub, so the pull
  requests below have somewhere to go: copy the directory, `git init`,
  commit, push.
- `gh auth login`, or `GITHUB_TOKEN` in the environment, for a verified view
  locally. Without it every status carries `unverified` and nothing reads
  `Holds`; the pull-request check is verified regardless.

```sh
python3 -m venv .venv && . .venv/bin/activate
python3 -m pip install -e . "pytest==9.1.1" "hypothesis==6.169.4" "mutmut==3.8.0"
python3 -m pytest        # 6 passed
```

## The walk-through

Open the copy in Claude Code and run the five skills in order. What each
prints below is what the skills say and what the binary printed when this
walk-through was run by hand (versions at the end); the ids will differ.

### 1. `/proob:sense`

Reads the tracked files and writes one file, `proob/.cache/sense/picture.md`:
the entry points (`begin`, `complete`, `fingerprint`), the state that must stay
consistent (the entry per key), and the invariants the names, docstrings,
and tests imply, each with its citation and a scope. It returns a summary of
at most twelve lines and ends with the sentence "This is my reading of the
code, not a fact about the system; nothing here is approved or checked."
Nothing else is written.

### 2. `/proob:declare`

There is no `proob/config.toml`, so declare runs `proob init` first:

```text
detected adapters: python
       written  proob/config.toml
       written  .github/workflows/proob-gate.yml
       written  .github/workflows/proob-evidence.yml
       written  .claude/settings.json
        merged  .gitignore
action pin: proob-ai/proob@v0.0.1 # pin to a commit SHA before relying on this gate
note: this proob was built without a release commit; the workflows pin a version tag. Replace it with the release's commit SHA before relying on the gate.
next: review proob/config.toml, commit, and open the pull request; the gate takes effect after it merges.
```

(The last two lines are what a binary built from source prints; a release
build pins the Action to the release's commit SHA and prints no note.)

Then it turns the picture into proposals, one TOML file each under
`proob/properties/`. For this repository it proposed three:

```toml
id = "01M62DFKKMFYX0VES2SAXKSG7X"
title = "A retried request is answered once"
statement = "A request retried under the same key gets the first response back and the work is never done twice."
scope = ["idem/store.py"]
severity = "high"
rationale = "The module doc of idem/store.py promises it and tests/test_store.py::test_a_new_key_proceeds_and_then_replays covers one case; a double charge or a duplicate order is the cost of getting it wrong."
```

and, in the same shape, "A key already in use is never accepted for a
request with a different fingerprint" (`idem/store.py`, high) and "Two
requests whose bodies hold the same fields and values have the same
fingerprint whatever order the fields are listed in"
(`idem/fingerprint.py`, medium). It validates them and derives status:

```text
proob validate: ok (3 property files)
proob: 3 proposed; Re-approve (config amended; workflow amended)
```

(`Re-approve (config amended; workflow amended)` says the config and
workflows are not yet approved on the default branch, so the built-in policy
applies; it disappears once the pull request merges.) Each proposal renders
`Proposed`. Declare commits to a branch `proob/declare-<date>` and opens the
pull request. The pull request carries:

- `proob/config.toml`: the Python adapter block and the gate policy, with
  `allow_self_signed = true` explained;
- `.github/workflows/proob-gate.yml`: the pull-request check, two jobs,
  `execute` with no token and `judge` with a read-only one;
- `.github/workflows/proob-evidence.yml`: the push and nightly runs whose
  results are the only evidence;
- `.claude/settings.json`: the plugin recommendation for teammates;
- `.gitignore`: `proob/.cache/`;
- the three proposals.

### 3. Approve by merging

Review the pull request on GitHub. Its body carries the three reviewer
instructions (merging approves every file as its current content; suggest
rather than edit; `reject <ULID>` to drop one proposal). An approving review
by someone with write access who did not write the pull request, followed by
a merge, is the approval. With one maintainer the merge is self-signed and
proob says so. After the merge each property reads `Not checked (no check
bound)`: approved, nothing checked yet.

### 4. `/proob:falsify`

Falsify writes one Hypothesis test per property into `tests/`, next to the
team's tests, reusing the strategies in `tests/conftest.py`, with the
property's id on the line above the test:

```python
# proob:01M62DFKKMFYX0VES2SAXKSG7X
@given(key=keys, fp=st.text(min_size=1, max_size=6), response=responses, retries=st.lists(st.integers(0, TTL - 1), max_size=8))
def test_a_retried_request_is_answered_once(key, fp, response, retries):
    store = IdempotencyStore(ttl_seconds=TTL)
    first = store.begin(key, fp, now=0)
    assert first == Proceed(), f"first request under {key!r} got {first}"
    store.complete(key, response)
    for now in sorted(retries):
        decision = store.begin(key, fp, now=now)
        assert decision == Replay(response), f"retry under {key!r} at {now} got {decision}, expected the first response {response}"
```

No seed, no example count: proob sets both. Then it runs
`proob validate` and `proob run check <id> --json` for each:

```text
proob validate: ok (3 property files)
```

```json
{"schema_version":1,"id":"01M62DFKKMFYX0VES2SAXKSG7X","kind":"check","algo":"sha256","content_hash":"b8d646001f11d52ab074511c242641dc2b4470254cb4afdc99653348ef3e71ec","binding_hash":"98b4909eee6509d2c88d58bda3bc22ac774e92f58e52ce85e67f454fb54a1ed7","scope_hash":"8e2d07e2884e984392c111e0ee2a4a32b3b1a2d4e7b2e6626fa354e74eb29b16","outcome":{"result":"no_counterexample","budget":256},"replay":{"seed":"1793528298208264728","cases":256},"tool_versions":{"hypothesis":"6.169.4","pytest":"9.1.1","python":"3.15.0"},"head_sha":"23a57cb2cbd6d56c9ddc54f409c0669c8d71324e"}
```

The other two lines have the same shape with their own `id`,
`content_hash`, and `replay.seed`; all three read
`"outcome":{"result":"no_counterexample","budget":256}`. Falsify reports each
as "passed locally (seed …, 256 cases): unverified until the default-branch
run", not as holding. The text form of the same run:

```text
proob run check: local run, unverified (not evidence; only an Action run on the default branch produces evidence)
property  proob/properties/01M62DFKKMFYX0VES2SAXKSG7X.toml (A retried request is answered once)
check     tests/test_props.py::test_a_retried_request_is_answered_once (token at tests/test_props.py:14)
outcome   no counterexample in 256 cases; replay with seed 1793528800745423064
tools     hypothesis 6.169.4, pytest 9.1.1, python 3.15.0
hashes    content b8d646001f11… binding 98b4909eee65… scope 8e2d07e2884e… over 1 file
```

Had a test found a counterexample, the JSON would carry
`"outcome":{"result":"counterexample","witness":…}` and falsify would stop
with two exits for you: fix the code, or amend the statement in its own
pull request. It picks neither.

The binding set of each test is `pyproject.toml` (the pytest table),
`tests/conftest.py`, and `tests/test_props.py`; after merge, an edit to any
of them renders the property `Re-approve (check amended)` until a reviewer
approves the amendment. Falsify opens a pull request `proob/falsify-<date>`
with the three rows and that note. Merge it with a review; the push run on
the default branch then produces the first evidence and each property reads
`Checked (strength pending)`.

### 5. `/proob:verify`

Verify asks one question per property: can this check fail? It runs
`proob run score <id> --json`, which mutates the code in the property's
scope and runs only the bound test against each mutant:

```json
{"schema_version":1,"id":"01M62DFKKMFYX0VES2SAXKSG7X","kind":"score","algo":"sha256","content_hash":"b8d646001f11d52ab074511c242641dc2b4470254cb4afdc99653348ef3e71ec","binding_hash":"98b4909eee6509d2c88d58bda3bc22ac774e92f58e52ce85e67f454fb54a1ed7","scope_hash":"8e2d07e2884e984392c111e0ee2a4a32b3b1a2d4e7b2e6626fa354e74eb29b16","outcome":{"result":"scored","killed":28,"survived":9,"timeout":0,"unviable":0},"tool_versions":{"hypothesis":"6.169.4","mutmut":"3.8.0","pytest":"9.1.1","python":"3.15.0"},"head_sha":"23a57cb2cbd6d56c9ddc54f409c0669c8d71324e"}
{"schema_version":1,"id":"01M62DFKMPB7HX6EW14ZY7V3RT","kind":"score","algo":"sha256","content_hash":"bebfd3dc67384b473b09cfe7935bba93ff68565c23d142130f1224d24223076c","binding_hash":"98b4909eee6509d2c88d58bda3bc22ac774e92f58e52ce85e67f454fb54a1ed7","scope_hash":"8e2d07e2884e984392c111e0ee2a4a32b3b1a2d4e7b2e6626fa354e74eb29b16","outcome":{"result":"scored","killed":23,"survived":14,"timeout":0,"unviable":0},"tool_versions":{"hypothesis":"6.169.4","mutmut":"3.8.0","pytest":"9.1.1","python":"3.15.0"},"head_sha":"23a57cb2cbd6d56c9ddc54f409c0669c8d71324e"}
{"schema_version":1,"id":"01M62DFKNQ7K25TVFVE1GJ6E8A","kind":"score","algo":"sha256","content_hash":"6dd0e2557e29ea97cfdeeca6ed4017947cd74944bc13e334c2151ed4701d48a5","binding_hash":"98b4909eee6509d2c88d58bda3bc22ac774e92f58e52ce85e67f454fb54a1ed7","scope_hash":"4a06c52215022932a5f2c70019b3b3d89ab95ebfd5bbdd4c20dfb4135cc763cf","outcome":{"result":"scored","killed":8,"survived":13,"timeout":0,"unviable":0},"tool_versions":{"hypothesis":"6.169.4","mutmut":"3.8.0","pytest":"9.1.1","python":"3.15.0"},"head_sha":"23a57cb2cbd6d56c9ddc54f409c0669c8d71324e"}
```

Verify reads these as "the test killed 28 of 37 mutants in the scope", "23
of 37", and "8 of 21". Each is at or above the floor (`kill_floor = 1`), so
each check can fail. The survivors are the interesting part. For the
fingerprint property, mutmut's list of the 13 survivors (verify reads it
and says so; the score JSON carries counts only) is every mutant the bound
test cannot see because it compares two fingerprints computed the same way:
`method.upper()` turned into `method.lower()`, the labels of the canonical
form (`"method"`, `"path"`, `"body"`) renamed, the separators changed or
dropped, the encoding name re-cased. The eight it killed are the ones that
break the order-independence the statement is about (`sort_keys=True`
turned off, the canonical object replaced). The statement says nothing about
what the fingerprint is, only that field order does not change it, so a
test that pins a known fingerprint for a fixed request would be a different,
stronger check. Only the bound check runs against a mutant; the six
example-based tests play no part. Verify offers the stronger test as a pull
request, since a change to a bound file reopens the check's approval.

A local score moves nothing. After the falsify pull request merges and the
nightly evidence run records a score at or above the floor, a property reads
`Holds` if a second person approved the statement, the check, the config,
and the workflow, and `Checked (self-signed)` if one maintainer merged them.

### 6. `/proob:guard`

Prints the one-line summary, whether the view is verified, the counts in
the vocabulary words, and the governance footer: approvals verified via
GitHub reviews, default branch requires review, `proob gate` required,
CODEOWNERS covers `proob/`, kill floor in force, scheduled evidence last ran.
On a fresh repository it warns once that the default branch does not require
review and that the `proob gate` check is not required to merge, with the
GitHub settings path for each. The same line opens every Claude Code session
in the repository from then on.

## What this walk-through ran, by hand

The outputs above were produced on 2026-10-29 by following the declare,
falsify, and verify skills step by step on a copy of this directory with no
remote (so the local status read `unverified: no GitHub remote` and nothing
was pushed), with:

- proob 0.0.1 built from this repository at commit `0ab6b80`
  (`cargo build --locked -p proob`, `PROOB_BIN` set to the build);
- Python 3.15.0, pytest 9.1.1, hypothesis 6.169.4, mutmut 3.8.0;
- Claude Code 2.1.312, on macOS arm64.

Each `proob run score` took under ten seconds on this scope; a score on a
large module takes minutes to hours, and the one that counts comes from the
nightly evidence run on the default branch.
