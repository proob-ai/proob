# Pull request body for a falsify PR

Branch: `proob/falsify-<YYYY-MM-DD>`. Title: `proob: bind <n> propert(y|ies) to checks`.
Fill every `<…>` from the property files, `proob hash <id> --json`, and the `run check` JSON; do
not retype numbers that the JSON already carries.

```markdown
## Bindings for review

Merging this PR approves each check below and its binding set. Nothing here is evidence: every
result is from a local run, unverified. Evidence comes only from the default-branch run after
merge.

| Property | Statement | Check | Local result (unverified) |
|---|---|---|---|
| `<id>` <title> | <statement, verbatim> | `<check reference>` in `<test file>` | passed locally (seed <replay.seed>, <outcome.budget> cases) |
| `<id>` <title> | <statement, verbatim> | `<check reference>` in `<test file>` | counterexample: `<outcome.witness>`; replay `proob run check <id> --seed <replay.seed>` |

### What each check asserts

- `<id>`: <one sentence: the generator's domain and the assertion, in plain words>.

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
- After merge: `Not checked` until the default-branch run completes, then `Checked (strength
  pending)` until a scheduled score run shows each check can fail.

Opened by /proob:falsify. The agent wrote the tests and ran them locally; it did not approve
anything and cannot.
```

Omit the counterexample row and the counterexample reviewer bullet when every result passed.
Quote statements verbatim, however long; never paraphrase one to fit the table.
