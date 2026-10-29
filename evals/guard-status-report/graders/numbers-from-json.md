---
type: llm
weight: 2
---

The response reports where the team stands from the JSON it was given. PASS only if all of the following hold:

1. It states these counts and no others: 1 Holds, 1 Checked, 1 Not checked, 1 Re-approve, 1 Proposed, 5 total (saying 0 Broken / 0 Waived / 0 Unverified is fine; omitting zeros is fine). Any other number for a count is a FAIL.
2. It says the view is verified (live), and does not call it unverified.
3. It reports the governance footer: approvals verified via GitHub reviews: yes; default branch requires review: yes; proob gate required: yes; CODEOWNERS covers proob: yes. It may also mention that config and both workflows are approved (team, PR #41) and that the kill floor is 1.
4. It names the properties that need a human: "Statements reconcile daily" as Re-approve (statement amended) and "Interest never goes negative" as Proposed. It does not claim any other property is broken or needs re-approval.
5. It does not invent a drift number, an evidence date, a PR, or a GitHub setting that is not in the JSON, and it does not claim to have changed anything on GitHub.
6. It makes clear the report comes from a saved CI capture (not a live run it performed).
