# Verdict soundness: version 1

Measure 3 of [`bench/README.md`](../README.md) on the simulated worlds: `rendered` against the world's `truth`, SV-COMP scoring. Run on 2026-10-29 from branch `feat/sound` (base `ea27b77`), proob 0.0.1, Rust 1.99.1, cargo-nextest 0.9.150, cargo-mutants 27.1.0, macOS arm64.

```sh
cargo xtask sim lifecycle honesty requirement shadow soundness
cargo xtask bench soundness
```

| Cell | Points | Count | Score |
|---|---|---|---|
| correct `Holds` | +2 | 3 | +6 |
| correct `Broken` | +1 | 14 | +14 |
| other | 0 | 243 | 0 |
| wrong `Broken` | −16 | 0 | 0 |
| wrong `Holds` | −32 | 0 | 0 |
| **total** | | **260** | **+20** |
| maximum possible | | | 128 |

**Sound: no wrong `Holds`.** 260 verdicts: 20 scripted steps (`lifecycle` 7, `honesty` 9, `requirement` 4) and the 240 worlds of `soundness`. `soundness` took 76 s.

The three correct `Holds` are lifecycle's scheduled score run, honesty's control, and the one `soundness` world whose truth is `holds` (team approval, a fresh push check, the real score, correct code). No other world rendered `Holds`: in particular none of the 120 planted-code worlds, including those given positive evidence from the earlier commit before the bug and the real score at the current binding.

The gap to the maximum is completeness, not soundness. Of the 120 planted worlds, 12 render `Broken` (a fresh negative default-branch check, with a team, self-signed, or author-only approval). 100 have no check of the planted code that proob may use: it is at an earlier commit, outside the window, from a PR or another branch, or absent. Abstaining there is right, and they score 0 against a maximum of 1. The other 8 have a fresh negative check but no usable approval. With none, proob renders `Not checked` (4 worlds); when the approver's access was revoked, it renders `Re-approve (reviewer access revoked)` (4). A `Broken` in those 8 worlds would score +1 each.
