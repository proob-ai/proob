---
description: On the Rust fixture, sense writes the picture to proob/.cache/sense/picture.md and nothing else, and calls it a reading rather than a fact.
runs: 1
max_turns: 40
timeout_seconds: 600
allowed_tools: [Read, Glob, Grep, Bash, Write, Edit, Skill, Agent]
tags: [sense, u7]
---
Use the proob sense skill on this repository (the current directory) and tell me what it found.

This machine has no proob release. A development build of the proob binary is at `./.proob-bin/proob`; run every proob command with `PROOB_BIN=$PWD/.proob-bin/proob` in its environment (the plugin shim honours it). There is no git remote and no GitHub access here.
