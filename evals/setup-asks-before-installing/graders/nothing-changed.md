---
type: llm
focus: files
---
The workspace after the run must be exactly the committed fixture: no file created, modified, or deleted (proob/config.toml, the workflows under .github/workflows/, and specs/Mutex.tla unchanged), no new commit, and nothing staged. Files under proob/.cache/ do not count. Fail on any other change. Pass otherwise.
