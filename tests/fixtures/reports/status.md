## proob: 1 holds, 1 checked, 1 not checked, 1 re-approve, 1 broken, 1 proposed, 1 waived

Inputs: working tree, main at c9e9a5503052, GitHub review records and default-branch evidence.

| Property | Status | Reason | This PR |
|---|---|---|---|
| A holds `01J9Z3W1V8Q4K7N2M5R8T1X4A1` | Holds | _approved, checked, proven able to fail_ | not run |
| B is checked `01J9Z3W1V8Q4K7N2M5R8T1X4B2` | Checked (strength pending) | _strength not yet measured_ | not run |
| C has no check `01J9Z3W1V8Q4K7N2M5R8T1X4C3` | Not checked | _no check bound_ | not run |
| D is proposed `01J9Z3W1V8Q4K7N2M5R8T1X4D4` | Proposed | _never reviewed_ | not run |
| E was amended `01J9Z3W1V8Q4K7N2M5R8T1X4E5` | Re-approve | _statement amended_ | not run |
| F matches nothing `01J9Z3W1V8Q4K7N2M5R8T1X4F6` | Broken | _scope matches nothing_ | not run |
| G is waived `01J9Z3W1V8Q4K7N2M5R8T1X4G7` | Waived until 2026-12-31 | _failing, human-waived with an open issue_ | not run |

- approvals verified via GitHub reviews: yes
- default branch requires review: yes
- proob gate required: yes
- CODEOWNERS covers proob: yes
- scheduled evidence last ran: 1 day ago

Legend: Holds (approved, checked, proven able to fail) · Checked (strength pending or self-signed) · Not checked (approved, nothing checked yet) · Re-approve (changed since approval) · Broken (negative result, empty scope, or expired waiver) · Proposed (never approved) · Waived until d (failing, human-waived)
