---
description: Try runs proob in a shadow of a Python repository that has never seen proob, shows the shadow report verbatim with its Make it real steps, says the next step in one sentence, claims nothing holds or is approved, and leaves the repository exactly as committed.
runs: 1
max_turns: 80
timeout_seconds: 1200
allowed_tools: [Bash, Read, Edit, Write, Grep, Glob, Skill]
tags: [try, shadow, python]
---
/proob:try

I want to see what proob finds in this repository before we change anything. This machine has no proob release: a development build is at `./.proob-bin/proob`; run every proob command with `PROOB_BIN=$PWD/.proob-bin/proob` in its environment (the plugin shim honours it). There is no git remote and no GitHub access here.
