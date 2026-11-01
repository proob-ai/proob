# Pull request body for a falsify PR

Branch: `proob/falsify-<YYYY-MM-DD>`. Title: `proob: bind <n> propert(y|ies) to checks`.
Fill every `<…>` from the property files, `proob hash <id> --json`, and the `run check` JSON; do
not retype numbers that the JSON already carries.

~~~markdown
## Bindings for review

Merging this PR approves each check below and its binding set. Nothing here is evidence: every
result is from a local run, unverified. Evidence comes only from the default-branch run after
merge.

| Property | Statement | Check | Local result (unverified) |
|---|---|---|---|
| `<id>` <title> | <statement, verbatim> | `<check reference>` in `<test file>` | passed locally (seed <replay.seed>, <outcome.budget> cases) |
| `<id>` <title> | <statement, verbatim> | `<check reference>` in `<test file>` | counterexample: `<outcome.witness>`; replay `proob run check <id> --seed <replay.seed>` |
| `<id>` <title> | <statement, verbatim> | Kani `<check reference>` in `<harness file>` | verified locally within the harness's bounds (<outcome.checks> checks) |
| `<id>` <title> | <statement, verbatim> | Kani `<check reference>` in `<harness file>` | proof failed: `<the failed check and location from outcome.diagnostics>` |
| `<id>` <title> | <statement, verbatim> | TLC `<check reference>` | model-checked locally over the constants in the cfg (<the constants>) |
| `<id>` <title> | <statement, verbatim> | TLC `<check reference>` | violation: `<the first line of outcome.trace>` (trace below) |

### What each check asserts

- `<id>`: <one sentence: the generator's domain and the assertion, in plain words>.

### Proved for

Kani checks a harness for every input it admits, and for no other. Each bound below comes from
the statement unless it is marked **narrows**; a narrowing you approve is part of what `Holds`
will mean for that property.

- `<id>` `<check reference>`
  - inputs: `<name>: <type>` from `kani::any()`, …
  - `kani::assume(<expr>)`: from "<the statement's words>"
  - `<CONST> = <n>` (size bound): <why n>. **narrows**: <what it leaves out, e.g. lists longer than n>
  - `#[kani::unwind(<n>)]`: <the loop, and why n covers the size bound>
  - covers: `<case>` from "<the statement's words>", …

### Model-checked for

TLC checks the spec, not the code: it explores every state the spec reaches over the constants
below, and no other. Whether the code follows the spec is your judgement as reviewer, and proob
asks for it again whenever the code changes. A constant marked **narrows** leaves out a case the
statement covers; approving it makes it part of what `Holds` will mean for that property.

- `<module path>` (<one module per protocol | one per property>: <why>), checking `<id>`
  `<Name>`, …
  - `<CONSTANT> = <value>`: <why this value: the least that reaches the case the statement is
    about, or the code's own limit (cite it)>. **narrows**: <what it leaves out> (or: no narrowing)
  - `<Action>` models `<file>:<lines>` (<what the code does there, in a few words>), …
  - leaves out: <what the spec abstracts away or assumes: the network, crashes, a library call, a
    second server, …>
  - `CHECK_DEADLOCK FALSE`: <why the code may stop there> (omit when the cfg does not set it)
- Drift: once approved, any commit to `<scope files>` reads `Re-approve (spec drifted: n commits
  to the modelled scope)` until the spec or the statement is re-approved, so a reviewer re-reads
  this spec against the changed code. It flags every change to those files, a refactor included; it cannot
  tell whether the code still follows the spec.

<details><summary>Violation trace (only when TLC found one)</summary>

```
<outcome.trace, verbatim>
```

</details>

### Fell back to a test

`<id>` was declared `kind = "proof"`. Kani could not take it, so this PR changes its `kind` to
`"test"` and binds the property-based test in the table above. Changing `kind` re-opens the
statement's approval: approving this PR approves the change. After merge it reads
`Checked (tested)` at best: a test samples inputs and cannot prove.

- Why, from `proob run check <id> --json`: `<outcome.reason, verbatim>`
- Retry with `#[kani::unwind(<n>)]` (unwinding only): `<the retry's outcome.reason, verbatim>`
- Or, not run: the harness needs <feature>, which Kani does not support
  (https://model-checking.github.io/kani/rust-feature-support.html).

<details><summary>The harness that was tried</summary>

```rust
<the harness as it ran, token included>
```

</details>

### Config change

The bound tests compile only with the cargo feature `<name>` (<why: the `cfg` line, the
`required-features`, or the optional dependency>), so this PR adds `features = ["<name>"]` to
the `<root>` adapter in `proob/config.toml`. Every proob run of this adapter, the gate's
included, builds with it. `proob/config.toml` is governed: approving this PR approves the
change, and until then every property reads `Re-approve (config amended)`.

### Binding sets

Approval attaches to these files' content. Editing any of them after merge renders the property
`Re-approve (check amended)` until a reviewer approves the amendment.

- `<id>`: `<binding_files, comma-separated>`

### For the reviewer

- Merging with your approval approves the checks and their binding sets; it does not touch the
  statements' own approval.
- Approve as a review on this PR. If you edit a bound file yourself, your approval no longer
  counts for it: leave a suggestion the author commits instead.
- A row with a counterexample names a real failure of the code or of the statement. The gate will
  show that property as `Broken (counterexample on this PR)` until the code is fixed or the
  statement is amended in its own PR to `proob/properties/`.
- After merge: `Not checked` until the default-branch run completes, then `Checked (tested,
  strength pending)` until a scheduled score run shows each check can fail, then `Checked (tested)`.
  A Kani harness reads `Checked (strength pending)` instead, then `Holds` once its score is at the
  floor and the team has approved it: `Holds` within the bounds under "Proved for".
- A Kani row is checked for every input its harness admits; review "Proved for" as you would the
  statement. A "proof failed" row is a bug, like a counterexample.
- A TLC row checks a model of the design, not the code; review "Model-checked for" as you would the
  statement: do the actions match the cited code, and do the constants reach the case that
  matters? A TLC model reads `Checked (strength pending)` after the default-branch run, then
  `Holds` for the design over those constants once its spec-mutant score is at the floor. A
  "violation" row is a design bug, or a wrong statement, like a counterexample.

Opened by /proob:falsify. The agent wrote the checks and ran them locally; it did not approve
anything and cannot.
~~~

Omit the counterexample row and the counterexample reviewer bullet when every result passed.
Omit "Config change" unless this PR sets `features`.
"Proved for" is required for every Kani-bound property and lists every `kani::assume`, size
bound, and unwind bound in its harness; omit it when no property is bound to Kani. Include "Fell
back to a test" once per fallback, with the reason line verbatim, and omit it when none fell
back; drop the lines that do not apply (the retry, or the not-run line). "Model-checked for" is
required once per TLA+ module and lists every constant, every action with its code citation,
what the spec leaves out, and the drift line; omit it when nothing is bound to a model. Never
write "proved", "proven", or "holds" about a local result, and never write that the code
conforms to, implements, or is verified by a spec.
Quote statements verbatim, however long; never paraphrase one to fit the table.
