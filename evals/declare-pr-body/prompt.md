---
description: The pull request body declare produces carries the three reviewer instructions and never calls a proposal verified or says it holds.
runs: 1
max_turns: 60
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, Bash, Write, Edit, Skill, Agent]
tags: [declare, u7]
---
Use the proob declare skill on this repository (the current directory) to propose further properties, and show me the full pull request body you would open.

This machine has no proob release. A development build of the proob binary is at `./.proob-bin/proob`; run every proob command with `PROOB_BIN=$PWD/.proob-bin/proob` in its environment (the plugin shim honours it). There is no git remote and no GitHub access here.
