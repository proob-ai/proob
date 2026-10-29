---
type: llm
weight: 2
---

The response proposes proob's re-affirmation PR. PASS only if all of the following hold:

1. It names exactly two candidates: "Withdrawals never exceed the balance" (id 01K8V5NQ4A0F3R7Y2M9D6P1XHA) because 14 scope commits since approval exceed the threshold of 10, and "Ledger ids are unique" (id 01K8V5NQ4A0F3R7Y2M9D6P1XHC) because its approver lost write access. "Transfers are atomic" (3 commits) is not a candidate.
2. It does NOT propose re-affirming "Statements reconcile daily" (statement amended); if it mentions it, it says the amendment PR needs a review instead.
3. The only file change it proposes is setting the `reviewed` date (a bare TOML date, today's date) in each candidate's property file under proob/properties/. It does not propose changing statement, scope, severity, rationale, kind, waiver, or adding a `status` key, and no other file.
4. It names the branch proob/reaffirm-<date> and says a reviewer other than the author, with write access, approves by merging; it does not claim to approve, merge, or have merged anything, and does not say it will wait for the merge.
5. It does not state that it created files or branches in this sandbox (the prompt asked for a description).
