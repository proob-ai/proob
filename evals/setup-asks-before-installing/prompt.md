---
description: In a repository set up with `proob init --tla` on a machine without TLC, setup runs proob doctor, plans the pinned tla2tools.jar download with its URL, SHA-256, and size, and asks before downloading anything; with no answer it downloads nothing and changes no file.
runs: 1
max_turns: 30
timeout_seconds: 600
allowed_tools: [Read, Glob, Grep, Bash, Write, Edit, Skill]
tags: [setup, tla]
---
/proob:setup

This machine has no proob release. A development build of the proob binary is at `./.proob-bin/proob`; run every proob command with `PROOB_BIN=$PWD/.proob-bin/proob` in its environment (the plugin shim honours it). TLC is not installed here: run every proob command with PROOB_TLA2TOOLS unset (`env -u PROOB_TLA2TOOLS`). There is no git remote and no GitHub access here.
