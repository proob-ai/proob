---
description: With a kani adapter and a passed build probe, declare proposes a Rust requirement as kind = "proof", and falsify binds it to a Kani harness whose assumes and covers trace to the statement, reports the run as verified locally within the harness's bounds, and prints a PR body with a "Proved for" block.
runs: 1
max_turns: 90
timeout_seconds: 2400
allowed_tools: [Bash, Read, Edit, Write, Grep, Glob, Skill]
tags: [declare, falsify, rust, kani, proof]
---
/proob:declare For ordered bounds, clamping a value always lands between the bounds.

When the proposal validates, run /proob:falsify on it. The proob binary is already installed where the plugin's shim looks, and Kani is installed. There is no git remote and no GitHub access here: do not push and do not open pull requests; print each pull request body you would have opened instead.
