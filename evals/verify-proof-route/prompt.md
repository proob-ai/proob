---
description: Verify proposes the proof route only for the kind = "proof" property, never claims a proof it did not run, and scores the test-backed property through proob run score --json.
runs: 1
max_turns: 50
timeout_seconds: 900
allowed_tools: [Bash, Read, Grep, Glob, Skill]
tags: [verify, rust, proof]
---
/proob:verify 01K8V5NQ4A0F3R7Y2M9D6P1XHP 01K8V5NQ4A0F3R7Y2M9D6P1XHA

The proob binary is already installed where the plugin's shim looks. Do not change any tracked file; show me what you would propose for each property and where each one stands.
