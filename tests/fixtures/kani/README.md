# Kani 0.68.0 outputs

Captured on 2026-10-31 from Kani 0.68.0 (`cargo-kani`, CBMC 6.11.0, nightly
`nightly-2026-08-21`) on macOS arm64, for the parser in
`crates/proob-facts/src/parsers/kani.rs`. Local paths are replaced by
`<crate>` (the crate's directory) and `<home>`.

Each case is `<case>.stdout.txt`, `<case>.stderr.txt`, `<case>.exit` (the exit
code), and `<case>.sarif` when Kani wrote one. Command, run from the crate
below with `PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin"`:

    cargo kani --harness verification::<case> --exact --sarif <case>.sarif

except: `harness_not_found` names `verification::nope`; `preprocessor_failed`
runs `verification::pass` with a `PATH` that puts an Alire GCC
(`~/.alire/bin`) before `/usr/bin`, whose `stdint.h` shadows the system one
for CBMC's `goto-cc`; `compile_error` runs `verification::broken` in a crate
whose only harness is `fn broken() { let x: u8 = "not a number"; assert!(x == 0); }`.

The crate (`kani-capture`, edition 2021, an empty `[workspace]` table, and
`unexpected_cfgs = { level = "warn", check-cfg = ['cfg(kani)'] }`), `src/lib.rs`:

```rust
pub fn clamp(x: i32, lo: i32, hi: i32) -> i32 { if x < lo { lo } else if x > hi { hi } else { x } }
pub fn buggy_abs(x: i32) -> i32 { if x < 0 { -x } else { x } }
pub fn sum_to(n: u32) -> u64 { let mut s = 0u64; let mut i = 0u32; while i < n { s += i as u64; i += 1; } s }

#[cfg(kani)]
mod verification {
    use super::*;
    #[kani::proof]
    fn pass() { let x: i32 = kani::any(); let r = clamp(x, -10, 10); assert!(r >= -10 && r <= 10); kani::cover!(x < -10); kani::cover!(x > 10); }
    #[kani::proof]
    fn fail_assert() { let x: i32 = kani::any(); kani::assume(x > i32::MIN); assert!(buggy_abs(x) > 0); }
    #[kani::proof]
    fn fail_overflow() { let x: i32 = kani::any(); let _ = buggy_abs(x); }
    #[kani::proof]
    #[kani::unwind(3)]
    fn unwind_short() { let n: u32 = kani::any(); kani::assume(n < 10); let _ = sum_to(n); }
    #[kani::proof]
    fn cover_unsat() { let x: i32 = kani::any(); kani::assume(x > 0); kani::cover!(x < 0); assert!(clamp(x, 0, 5) >= 0); }
    #[kani::proof]
    fn unsupported() { let x: u64 = kani::any(); let y: u64; unsafe { core::arch::asm!("mov {0}, {1}", out(reg) y, in(reg) x); } assert!(y == x); }
}
```

| Case | Exit | What decides it |
|---|---|---|
| `pass` | 0 | `VERIFICATION:- SUCCESSFUL`, every check `SUCCESS`, both covers `SATISFIED`; SARIF has no results |
| `fail_assert` | 1 | a harness assertion `FAILURE`; `Failed Checks:` names it |
| `fail_overflow` | 1 | a Kani safety check (`attempt to negate with overflow`) `FAILURE` |
| `unwind_short` | 1 | `sum_to.unwind.0` (`unwinding assertion loop 0`) `FAILURE`, two checks `UNDETERMINED`, `[Kani] info: … unwinding failures.` |
| `cover_unsat` | 0 | `VERIFICATION:- SUCCESSFUL`, but the cover `Status: UNSATISFIABLE`; SARIF has no results (it never reports covers) |
| `unsupported` | 1 | `….unsupported_construct.1` `FAILURE`, the assertion `UNDETERMINED`, `** WARNING: A Rust construct that is not currently supported by Kani was found to be reachable.` |
| `harness_not_found` | 1 | `error: Failed to match the following harness(es):`; no SARIF |
| `preprocessor_failed` | 1 | `preprocessing has failed`, `error: goto-cc exited with status exit status: 1`; no SARIF |
| `compile_error` | 1 | rustc's `error[E0308]` on stdout, `error: could not compile` on stderr; no SARIF |

What the parser needs to know about the shape:

- SARIF lists only checks that did not succeed (`FAILURE` at level `error`,
  `UNDETERMINED` at `warning`), never successes or covers, so the verdict,
  the check count, and the cover statuses are read from stdout's `RESULTS:`
  block; SARIF is read to fail closed (a `SUCCESSFUL` run must have written
  one with no results) and for the `kani.cbmc.timeout` and `kani.cbmc.oom`
  rule ids.
- `SAT checker: instance is UNSATISFIABLE` appears in passing runs; only a
  cover check's own `- Status:` line says whether it was satisfied.
- Exit codes are 0 or 1 only; `cover_unsat` exits 0.
- Not captured (no small harness triggers them on a laptop): CBMC's timeout
  (`CBMC timed out. You may want to rerun your proof with a larger timeout …`)
  and out-of-memory (`CBMC appears to have run out of memory. …`) messages,
  taken verbatim from the `kani-driver` 0.68.0 binary; the parser tests
  build those outputs from the passing capture.
