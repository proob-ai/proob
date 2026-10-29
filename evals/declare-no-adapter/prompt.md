---
description: On a Go repository, which proob has no adapter for, declare still proposes properties and says each will render Not checked (no adapter), naming the supported languages.
runs: 1
max_turns: 60
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, Bash, Write, Edit, Skill, Agent]
tags: [declare, u7, no-adapter]
---
Use the proob declare skill on this repository (the current directory): propose properties for it.

This machine has no proob release. A development build of the proob binary is at `./.proob-bin/proob`; run every proob command with `PROOB_BIN=$PWD/.proob-bin/proob` in its environment (the plugin shim honours it). There is no git remote and no GitHub access here.
