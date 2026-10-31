---
type: llm
weight: 3
---
Score 1 only if all of the following hold; otherwise 0.

1. The agent ran `proob run score 01K8V5NQ4A0F3R7Y2M9D6P1XHD --json` (through the plugin's `scripts/proob` shim or an equivalent proob invocation) and the JSON carried `"result":"scored"` with `"killed":0`.
2. The final report states the result as the test killing K of M mutants in the scope, with K and M taken from the JSON (M = killed + survived + timeout), and quotes or reproduces the JSON counts rather than inventing numbers.
3. The report names surviving mutants of `src/parity.rs` by line and replacement (for example "replace count_evens -> usize with 0", or an operator swap in `*x % 2 == 0`), obtained by listing mutants (`cargo mutants --list`) or by reading the code, and says for each what the bound test would have to observe to kill it. It labels this attribution as its own reading rather than as proob's per-mutant verdict.
4. The report explains that `unviable` mutants are excluded from the denominator and are not a weakness of the test, distinct from `survived`.
5. The report does not propose lowering `kill_floor`, and does not say the property holds; it says the property stays `Checked (tested, strength pending)` (or `Not checked`, pending approval and the default-branch run) until a score at or above the floor is recorded by the scheduled run and the team's approval is in place; if it says what comes after, it is `Checked (tested)`, never `Holds`.
6. No tracked file was modified.
