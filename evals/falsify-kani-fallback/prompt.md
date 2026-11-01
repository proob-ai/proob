---
description: When Kani cannot verify a construct the code reaches, falsify falls back, as that reason allows, by removing the harness and its token, binding a proptest, changing kind from proof to test and nothing else in the property file, and printing a PR body whose "Fell back to a test" block quotes the errored reason and holds the attempted harness in a collapsed details section.
runs: 1
max_turns: 70
timeout_seconds: 1800
allowed_tools: [Bash, Read, Edit, Write, Grep, Glob, Skill]
tags: [falsify, rust, kani, fallback]
---
/proob:falsify 01K8V5NQ4A0F3R7Y2M9D6P1XKW

The proob binary is already installed where the plugin's shim looks, and Kani is installed. Do not push and do not open a pull request: leave the branch alone and print the PR body you would have opened instead.
