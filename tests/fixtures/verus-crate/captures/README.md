# Captured verifier outputs

What `cargo verus` printed on this fixture under Verus `0.2026.10.26.d17fbd0`, for the parser and classifier tests in `crates/proob-adapters/tests/verus.rs` and `crates/proob-facts/src/parsers/verus.rs`. Each capture is three files: `<stem>.stdout.txt`, `<stem>.stderr.txt`, `<stem>.exit`. Local paths are replaced by `<workspace>` and `<cargo-registry>`. Regenerate with the same commands after a Verus pin bump.

Commands, run from the workspace root with the package's artifacts cleaned first (`cargo clean -p clamp --target-dir <X>`; cargo replays a cached build without the verifier's stdout):

| Stem | Command | Source |
|---|---|---|
| `pass` | `cargo verus verify --locked -p clamp --fwd-verus-args-to roots --target-dir <X> -- --no-cheating --rlimit 10 --output-json` | unchanged |
| `item` | `cargo verus focus --locked -p clamp --target-dir <X> -- --no-cheating --rlimit 10 --output-json --verify-root --verify-function clamp` | unchanged; the attribution run, whose JSON has no `success` key and `verified: 1` |
| `proof-failed` | as `pass` | `clamp` returns `hi` when `x < lo`: `postcondition not satisfied` |
| `error` | as `pass` | `let _bad: u8 = a;` in `smaller`: rustc's `mismatched types`, no JSON |

What the adapter reads from them: the JSON object starts at the first stdout line that is exactly `{`; `verification-results.success` is present for a whole-crate run and absent for a partial one; a failing proof is located only by the `error: … --> file:line:col` pair on stderr; a type error leaves no JSON, so it is `Errored` for a check and unviable for a mutant.
