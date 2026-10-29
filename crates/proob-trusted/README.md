# proob-trusted

The trusted base of proob. The verified crate (`proob-core`) is checked by Verus
under `--no-cheating`, which forbids `assume`, `admit`, `external_body`, and
`assume_specification` there. Whatever cannot be verified lives here instead, as
`#[verifier::external_body]` functions whose contracts Verus takes on trust.

Growth of this file is growth of the trusted computing base. The CI verify job
prints the line count of `src/lib.rs` so that reviewers see it move.

## Trusted functions

| Function | Contract the verifier trusts | Why it is trusted |
|---|---|---|
| `sha256(bytes: &[u8]) -> [u8; 32]` | `digest@ == sha256_spec(bytes@)` | Wraps the `sha2` crate. `sha256_spec` is an uninterpreted spec function, so proofs may only use that SHA-256 is a function of its input. Correctness of the hash itself is `sha2`'s. |
| `now_secs() -> Option<u64>` | none | Wraps `std::time::SystemTime`. `None` means the platform clock is before the Unix epoch. It has no specification, so no verified property can depend on the current time, only on a time value passed in as data. |

## What is not here

No I/O, no git, no GitHub client, no process spawning. Those live in
`proob-effects` and `proob-spawn` (later units), which are not verified and are
reached only through data types in `proob-facts`.
