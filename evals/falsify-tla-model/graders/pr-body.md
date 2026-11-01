---
type: llm
weight: 2
---
Score 1 only if all of the following hold; otherwise 0.

1. The printed pull request body has a row for 01K8V5NQ4A0F3R7Y2M9D6P1XTM with the statement verbatim, the TLC check reference (`<module path> <Name>`), and the local result labelled unverified.
2. It has a "Model-checked for" block that names each constant in the `.cfg` with its value and why that value, names each action with the code it models cited by file in `payments/`, and says what the model leaves out (for example other failures of the network, a server crash, or a second server).
3. The block or the reviewer notes say that TLC checks the spec and not the code, and that after approval any commit to the property's scope (`payments/server.py`, `payments/client.py`) flags the spec for re-approval (`Re-approve (spec drifted …)`).
4. The spec files under `proob/specs/` are committed on a `proob/falsify-…` branch, or named in the body as the files the pull request carries.
