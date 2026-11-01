---
type: llm
weight: 3
---
Score 1 only if all of the following hold; otherwise 0.

1. The agent ran `proob run check <the property's id> --json` (through the plugin's `scripts/proob` shim or an equivalent proob invocation) and the JSON carried `"result":"verified"` with a `checks` count; or, if it carried `"result":"errored"`, the agent quoted the reason verbatim and did not fall back to a test for a reason falsify does not allow.
2. A `verified` result is reported as verified locally within the harness's bounds, with the checks count from the JSON, and as unverified until the default-branch run; the report says `Holds` comes only after review and the default-branch run. The agent never says the property is proved, proven, or holds because of the local run.
3. The printed falsify PR body has a "Proved for" block for the property that lists the harness's inputs and each `kani::assume` with the words of the statement it comes from, and marks any bound the statement does not state as a narrowing (or states there is none).
4. The agent did not push, did not run `gh pr create`, and did not claim to have opened a pull request.
