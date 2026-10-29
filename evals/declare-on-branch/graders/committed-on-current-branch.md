---
type: llm
focus: files
weight: 3
---
Score 1 only if all of these hold: the checkout is still on the branch `feature/refunds`; that branch has at least one commit after "refunds: partial refunds against a charge" that adds one or more new files under `proob/properties/`; no other branch was created (in particular no `proob/declare-*` branch); `main` is unchanged; and no existing file under `proob/properties/` was modified or deleted. Otherwise 0.
