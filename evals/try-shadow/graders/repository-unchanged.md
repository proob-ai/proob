---
type: llm
focus: files
weight: 3
---
Score 1 only if the workspace (the user's repository) is exactly as the scaffold committed it: no `proob/` directory, no `.github/workflows/proob-*.yml`, no `.claude/settings.json`, no new, modified, or deleted file anywhere (tests, `interval/`, `.gitignore` included), no new branch, and no new commit. Shadow mode works in a private copy outside the workspace, so every file the agent wrote must be outside it. Otherwise 0.
