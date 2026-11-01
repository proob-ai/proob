# TLC 2.19 outputs

Captured on 2026-11-02 from `tla2tools.jar` (TLC2 Version 2.19 of 08 August
2024, rev 5a47802) under OpenJDK 25 on macOS arm64, for the classifier in
`crates/proob-facts/src/parsers/tlc.rs`. Local paths are replaced by
`/tmp/proob-tlc` (the run directory) and `/tmp/java` (Java's temporary
directory); the pid by `1`. TLC wrote nothing to stderr but in `unrecognised`, whose
Java exception is all it printed.

Each case is `<case>.stdout.txt` (`<case>.stderr.txt` for `unrecognised`) and
`<case>.exit` (the exit code). The spec is
`tests/fixtures/tla-spec` (`Mutex.tla`, `Bounds.tla`); each run is

    java -XX:+UseParallelGC -cp tla2tools.jar tlc2.TLC -config <cfg> -workers 1 -metadir <fresh dir> -fp 0 -seed 0 Mutex.tla

with a cfg of `SPECIFICATION Spec`, `CONSTANT Procs = {p1, p2}`, and one line:

| case | checked | spec |
|---|---|---|
| `no_error` | `INVARIANT MutualExclusion` | as is |
| `invariant_violated` | `INVARIANT CountBelowTwo` | as is |
| `temporal_violated` | `PROPERTY SomeoneEnters` | as is (no fairness: stuttering) |
| `deadlock` | `INVARIANT MutualExclusion` | `Next == \E p \in Procs : Acquire(p)` (no `Release`) |
| `parse_error` | `INVARIANT MutualExclusion` | `CountBelowTwo == count < ` |
| `semantic_error` | `INVARIANT MutualExclusion` | `CountBelowTwo == cnt < 2` |
| `unrecognised` | `INVARIANT MutualExclusion` | as is, rerun within the same second into the same `-metadir` |
