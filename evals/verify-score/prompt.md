---
description: Verify runs proob run score --json on a bound property whose test is vacuous, reports "killed K of M" from the JSON, names the survivors with the mutated line and what a stronger test needs, distinguishes unviable from survived, and lowers nothing.
runs: 1
max_turns: 40
timeout_seconds: 900
allowed_tools: [Bash, Read, Grep, Glob, Skill]
tags: [verify, rust, score]
---
/proob:verify 01K8V5NQ4A0F3R7Y2M9D6P1XHD

Tell me whether this check can fail and what a stronger test would need. The proob binary is already installed where the plugin's shim looks. Do not change any tracked file.
