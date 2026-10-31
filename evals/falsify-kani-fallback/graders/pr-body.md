---
type: llm
weight: 3
---
Score 1 only if all of the following hold in the agent's final report; otherwise 0.

1. The printed PR body has a "Fell back to a test" block for 01K8V5NQ4A0F3R7Y2M9D6P1XKW that quotes, verbatim, the `reason` line from the Kani run's JSON (it begins "Kani cannot verify a construct this code reaches").
2. That block holds the attempted Kani harness, token included, inside a collapsed `<details>` element.
3. The body or the report says that changing `kind` re-opens the statement's approval for review, and that after merge the property reads `Checked (tested)` at best.
4. The agent never says the property is proved, proven, verified, or holds, and does not claim the Kani run succeeded.
5. The agent did not push, did not run `gh pr create`, and did not claim to have opened a pull request.
