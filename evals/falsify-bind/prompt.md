---
description: Falsify binds an unbound property on the Rust fixture with one tokened proptest, runs proob validate and proob run check --json, reports the result as local and unverified, and drafts the PR body.
runs: 1
max_turns: 60
timeout_seconds: 900
allowed_tools: [Bash, Read, Edit, Write, Grep, Glob, Skill]
tags: [falsify, rust]
---
/proob:falsify 01K8V5NQ4A0F3R7Y2M9D6P1XHC

This checkout is a small Rust crate with a proob declaration; the proob binary is already installed where the plugin's shim looks. Do not push and do not open a pull request: leave the branch alone and print the PR body you would have opened instead.
