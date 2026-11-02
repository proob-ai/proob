---
description: With a tla adapter, falsify binds a kind = "model" property to an invariant of a TLA+ spec under proob/specs/ that models the retry protocol the scoped code implements, cites the code each action models, runs it through TLC with proob run check, reports the result as a claim about the design over the cfg's constants, and prints a PR body with a "Model-checked for" block.
runs: 1
max_turns: 90
timeout_seconds: 2400
allowed_tools: [Bash, Read, Edit, Write, Grep, Glob, Skill]
tags: [falsify, tla, model]
---
/proob:falsify 01K8V5NQ4A0F3R7Y2M9D6P1XTM

The proob binary is already installed where the plugin's shim looks, Java is on PATH, and PROOB_TLA2TOOLS names a tla2tools.jar. There is no git remote and no GitHub access here: do not push and do not open a pull request; print the pull request body you would have opened instead.
