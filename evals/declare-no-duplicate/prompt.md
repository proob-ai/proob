---
description: On the Rust fixture that already declares four properties, declare proposes only statements that are not equivalent to an existing one.
runs: 1
max_turns: 60
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, Bash, Write, Edit, Skill, Agent]
tags: [declare, u7]
---
Use the proob declare skill on this repository (the current directory) to propose further properties.

This machine has no proob release. A development build of the proob binary is at `./.proob-bin/proob`; run every proob command with `PROOB_BIN=$PWD/.proob-bin/proob` in its environment (the plugin shim honours it). There is no git remote and no GitHub access here.
