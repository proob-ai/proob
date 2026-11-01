---
type: llm
weight: 2
---
Score 1 only if all of the following hold; otherwise 0.

1. A new property file under `proob/properties/` states the requirement (a clamped value lies between ordered bounds) with a scope that matches `src/lib.rs`, and declares `kind = "proof"`.
2. Declare's report or its PR body says why the kind is `proof`, quoting the line `kani_probe = "passed"` from `proob/.cache/kani-probe.toml`.
3. No existing file outside `proob/properties/` was modified by declare, and the property file carries no `reviewed`, `waiver`, or `status` key.
