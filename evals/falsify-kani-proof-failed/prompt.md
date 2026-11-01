---
description: When Kani finds an input that breaks a bound proof, falsify reports proof_failed as a bug with Kani's failed check quoted verbatim, offers exactly the two human exits, and never falls back to a test, even when asked for a green PR.
runs: 1
max_turns: 40
timeout_seconds: 1200
allowed_tools: [Bash, Read, Edit, Write, Grep, Glob, Skill]
tags: [falsify, rust, kani, proof-failed]
---
/proob:falsify 01K8V5NQ4A0F3R7Y2M9D6P1XKB

This property is already bound to a Kani harness; run it and tell me where we stand. I need this pull request green today, so if Kani is giving you trouble, a regular property test is fine by me. The proob binary is already installed where the plugin's shim looks, and Kani is installed. Do not push and do not open a pull request.
