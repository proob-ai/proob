# The pull request body

Fill the angle-bracket fields and keep everything else, especially the three
numbered reviewer instructions, word for word. Drop the "First run" section
when `proob init` did not run in this invocation, the "No adapter" section
when `proob/config.toml` lists an adapter for the repository's language, the
"Proof or test" section when it has no `kani` adapter, the "Model" section
when no proposal declares `kind = "model"`, and the "Supersedes" section when
no proposal replaces an existing property.
Never write "holds", "verified", "guaranteed", "proven", or "approved" about
a proposal anywhere in the body.

On a feature branch (declare's branch mode, or a requirement on a feature
branch), the proposals join the feature's own pull request: print the body
without its `##` heading and without "First run", under a `### proob
proposals` heading, for the user to paste into that pull request's
description.

~~~markdown
## proob: <N> property proposals

These are proposals. Each renders `Proposed` until this pull request merges,
and none is checked until a test is bound to it with `/proob:falsify`. The
statements come from the agent's reading of the code; the evidence each
rationale cites is where to look.

| property | statement | severity | scope |
|---|---|---|---|
| `<ULID>` | <statement> | <severity> | `<glob>`, `<glob>` |

### How to review

1. Merging approves every file in this pull request as its current content. Only a review by someone other than the author counts as a team approval; a merge without one is self-signed, and proob says so.
2. To change wording, use GitHub's "suggest" on the line and let the author commit the suggestion. You may push your own edit instead, knowing that a file you edited then needs someone else's approval.
3. To reject one proposal without rejecting the rest, reply `reject <ULID>` on this pull request with that proposal's id, or delete the file in the pull request.

### Status now

- `<headline from proob status --json, verbatim>` (<GitHub records consulted | local, unverified: reason>)
- `<the proob validate line>`

### First run

`proob init` wrote these files. They are governed like the proposals and take effect only once this pull request merges with a review:

- `proob/config.toml`: the detected adapters and the gate policy (what warns, what blocks). `allow_self_signed = true` until a second maintainer with write access reviews.
- `.github/workflows/proob-gate.yml`: the pull-request check. Two jobs: `execute` runs the bound checks and carries no token; `judge` reads GitHub's review records and the default-branch evidence, renders the summary, and blocks only for a `Broken` property the pull request affects.
- `.github/workflows/proob-evidence.yml`: the runs on every push to `<default branch>` and nightly whose results are the only evidence a verdict reads. A pull request's own run never becomes evidence.
- `.claude/settings.json`: recommends the proob plugin to teammates who open this repository in Claude Code.
- `.gitignore`: ignores `proob/.cache/`, the local cache of fetched facts and the picture.

The gate takes effect after this pull request merges.

### No adapter

proob has no adapter for this repository's language, so each property above will render `Not checked (no adapter)` until an adapter exists. Adapters today: Rust with proptest, Rust under Verus, Rust with Kani, Python with Hypothesis, and TLA+ with TLC for a model of a design in any language (`proob init --tla`). The statements still stand as the team's agreement, and the gate still lists them.

### Proof or test

`<the kani_probe line from proob/.cache/kani-probe.toml, verbatim | proob/.cache/kani-probe.toml is missing>`: so the Rust proposals declare `kind = "<proof | test>"`. A `proof` proposal gets a Kani harness from `/proob:falsify`, whose pull request lists the inputs the harness covers; if Kani cannot take one, that pull request changes it to `test` and says why.

### Model

`<ULID>` declares `kind = "model"` because <what makes its scope protocol-shaped: the steps that interleave, repeat, or fail>. `/proob:falsify` will write a TLA+ spec of that design under `proob/specs/` and bind the statement to its invariant; its pull request lists the constants the spec is checked over and the code each step models. A model checks the design over those constants, not the code. Once approved, any commit to the scope (`<glob>`, …) flags the spec for re-approval.

### Not proposed

- <statement left out> — same as `<ULID>` | deleted from `<default branch>` | rejected on #<PR>
- rejected statements could not be checked: `gh` <missing | unauthenticated | offline> (drop this line when gh worked)

### Supersedes

- `<new ULID>` replaces `<old ULID>`: <why>. Delete the old file in this pull request if you agree.
~~~
