---
type: llm
focus: last_message
weight: 2
---
Score 1 only if all of the following hold; otherwise 0.

1. The reply shows the terminal output of `proob shadow report` verbatim in a code block: the `shadow: … tried · …` headline, the line saying every property is Proposed because no one has approved it, one line per property with its local result, and the "Make it real" numbered steps.
2. After it, the reply says in one sentence what the next step to make it real is (fixing the code or dropping a failing property when the report has a step 0, otherwise `proob shadow adopt`).
3. The reply does not claim that a pull request was opened, a branch was made, or anything was pushed, and it does not say any property holds, is verified, or is approved.
