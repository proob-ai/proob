# Verus `--output-json` fixtures

Captured in U1 from Verus `0.2026.10.26.d17fbd0` (commit `d17fbd0c58abf0d5e86825ea64b96aba72b1b31f`) on `proob-core`, for U15's proof adapter. Local paths are replaced by `<workspace>` and `<cargo-registry>`.

Command, run from the workspace root:

    cargo verus verify -p proob-core --fwd-verus-args-to roots -- --no-cheating --rlimit 10 --output-json

| File | Run |
|---|---|
| `pass.cargo-verus.stdout.txt` | Raw stdout of a passing run, as `cargo verus` prints it |
| `pass.json` | The JSON object cut from that stdout |
| `fail-assert.json`, `fail-assert.stderr.txt` | `assert(false);` injected into `combine_sorted` |
| `fail-assume-no-cheating.json`, `fail-assume-no-cheating.stderr.txt` | `assume(false);` injected into `combine_sorted`, rejected by `--no-cheating` |

What U15 needs to know about the shape:

- stdout is not pure JSON. Every verified dependency crate prints a plain `verification results:: N verified, M errors` line first (here `vstd` and `proob-trusted`, which do not receive `--output-json` under `--fwd-verus-args-to roots`). The object for the root crate starts at the first line that is exactly `{`.
- Top-level keys: `func-details`, `verification-results`, `verus`.
- `verification-results`: `{"encountered-error", "encountered-vir-error", "success", "verified", "errors", "is-verifying-entire-crate"}`. On a `--no-cheating` rejection (`encountered-vir-error: true`) the `verified`/`errors` counts are absent.
- `func-details` maps every function in scope (including `vstd` items) to `{"obligation_proof_notes": [], "failed_proof_notes": []}`. In these fixtures the failing function carries no `failed_proof_notes`; the failing location is reported only in the rustc-style diagnostics on stderr (`assertion failed` at `file:line`). U15 should therefore pair the JSON verdict with stderr diagnostics, or verify per function with `--verify-function`.
- `verus.toolchain` includes the suffix `(overridden by environment variable RUSTUP_TOOLCHAIN)` because `cargo verus` sets that variable.
- The process exit code is 101 on any failure (cargo's build-failure code), 0 on success.
