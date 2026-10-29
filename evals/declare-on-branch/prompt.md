---
description: On a feature branch of a Python repository that has adopted proob, declare with no argument infers branch mode, states it in its first line, proposes properties for the module the branch adds that no property covers, and commits them on the feature branch rather than a new one.
runs: 1
max_turns: 60
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, Bash, Write, Edit, Skill]
tags: [declare, branch, python]
---
/proob:declare

This machine has no proob release. A development build of the proob binary is at `./.proob-bin/proob`; run every proob command with `PROOB_BIN=$PWD/.proob-bin/proob` in its environment (the plugin shim honours it). There is no git remote and no GitHub access here.
