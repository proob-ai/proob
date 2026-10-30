## proob gate: block (4 affected of 8); 1 checked, 2 not checked, 1 re-approve, 2 broken, 1 proposed, 1 waived

Inputs: head pr against base main; config from the approved base config; GitHub review records and default-branch evidence; this PR's own results.

| Property | Status | Reason | This PR |
|---|---|---|---|
| A holds `01J9Z3W1V8Q4K7N2M5R8T1X4A1` | Broken **blocks** | _counterexample on this PR_ | counterexample |
| C has no check `01J9Z3W1V8Q4K7N2M5R8T1X4C3` | Not checked (warn) | _no check bound_ | not run |
| E was amended `01J9Z3W1V8Q4K7N2M5R8T1X4E5` | Re-approve (warn) | _statement amended_ | not run |
| J is proved `01J9Z3W1V8Q4K7N2M5R8T1X4J9` | Not checked (warn) | _evidence stale_ | not run |

**If merged now** (governed files in this change)

| File | Approval if merged now | Remedy |
|---|---|---|
| `proob/config.toml` | unreviewed: the approval predates the file's last change | re-approve the latest commit |
| `proob/properties/01J9Z3W1V8Q4K7N2M5R8T1X4H8.toml` | unreviewed: the approval predates the file's last change | re-approve the latest commit |

- approvals verified via GitHub reviews: yes
- default branch requires review: yes
- proob gate required: yes
- CODEOWNERS covers proob: yes
- scheduled evidence last ran: 1 day ago

Legend: Holds (approved, proved, shown able to fail) · Checked (tested, strength pending, or self-signed) · Not checked (approved, nothing checked yet) · Re-approve (changed since approval) · Broken (negative result, empty scope, or expired waiver) · Proposed (never approved) · Waived until d (failing, human-waived)
