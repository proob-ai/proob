---
type: llm
weight: 3
---
Score 1 only if all of the following hold; otherwise 0.

1. A new proptest test was written for property 01K8V5NQ4A0F3R7Y2M9D6P1XHC ("The number of even values in a list is at most the number of values in it", scope `src/parity.rs`) under `tests/` of the crate (in `tests/props.rs` or a new `tests/*.rs` file), and it calls `count_evens` and asserts the count is at most the input length.
2. The comment line `// proob:01K8V5NQ4A0F3R7Y2M9D6P1XHC` sits directly above the test's `#[test]` attribute (only blank lines, comments, or attributes between them), and that token appears above exactly one test.
3. The test hard-codes no seed, no case count, no `ProptestConfig`, and has no `#[ignore]`.
4. The agent ran `proob validate` and then `proob run check 01K8V5NQ4A0F3R7Y2M9D6P1XHC --json` (through the plugin's `scripts/proob` shim or an equivalent proob invocation), and the run produced a JSON line with `"result":"no_counterexample"` (or, if the run errored for an environmental reason, the agent quoted the `reason` field and did not weaken the test).
5. No file under `proob/properties/` was modified, created, or deleted.
