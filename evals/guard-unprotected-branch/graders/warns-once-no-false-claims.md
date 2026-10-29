---
type: llm
weight: 2
---

The response reports status and governance gaps. PASS only if all of the following hold:

1. It reports the counts 1 Holds and 1 Proposed (2 total) and says the view is verified.
2. It warns that the default branch does not require review (footer value "no") and that whether the proob gate is a required check is unknown (footer value "unknown"), each stated once, not repeated.
3. For each of those it gives the GitHub repository Settings path (Rules/Rulesets or Branches/Branch protection) and the setting to turn on (require a pull request before merging with approvals; require status checks to pass with `proob gate`), and it says that "unknown" means proob could not read the branch rules with its token.
4. It suggests a CODEOWNERS entry covering /proob/ and the two workflow files (.github/workflows/proob-gate.yml and proob-evidence.yml), as a file to add in a PR, and mentions that the "Require review from Code Owners" branch setting must be enabled by an admin for it to bind.
5. It does not claim to have changed, enabled, or verified any GitHub setting itself, and it does not stop to wait for the user to perform an administrative step before giving the full report.
6. It does not invent numbers that are not in the JSON.
