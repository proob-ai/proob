# Naming the mutants behind a score

`proob run score` prints counts (`killed`, `survived`, `timeout`, `unviable`) and discards
cargo-mutants' per-mutant file with its temporary directory. To explain which mutants a test let
through, list the mutants the tool would make and read them against the bound test. Listing
compiles nothing and runs nothing.

```
cargo mutants --list --json --file <scope file> [--file <scope file> …]
```

Run it from the adapter root (`root` in `proob/config.toml`), with the scope files from
`proob hash <id> --json`. Each entry gives `file`, `function.function_name`, `span.start.line`,
`genre`, and `replacement`. The same list, in the same order, is what the score ran: cargo-mutants
enumerates mutants deterministically and `run score` passes `--no-shuffle`.

For the Rust fixture's `src/ordering.rs`:

```
src/ordering.rs:3:5: replace sorted -> Vec<u32> with vec![]
src/ordering.rs:3:5: replace sorted -> Vec<u32> with vec![0]
src/ordering.rs:3:5: replace sorted -> Vec<u32> with vec![1]
```

Against a test that asserts only "no element is greater than the next", all three survive: an
empty vector and a one-element vector are non-decreasing. Against `sorted_preserves_length` all
three die, because the length differs from the input's on almost every generated case. That is
the whole method: for each mutant, ask on which generated input the bound test would observe a
different value, and whether the generator produces such an input.

## Writing it up

For each mutant you believe survived:

- the line and the replacement, verbatim from the list;
- the input on which the real code and the mutant differ;
- the assertion the bound test lacks (a length check, a permutation check, a value at a boundary,
  a case the generator never produces).

Then the smallest change to the test that kills them, as a diff the human can read, with the
reminder that the change reopens the binding's approval (`Re-approve (check amended)` after it
merges, until a reviewer approves it).

State plainly that the attribution is yours: proob reported K killed and S survived; the names
come from the list and your reading of the test. When `survived` is larger than the number of
mutants you can explain, say so rather than inventing a reason.

## Counting

- M, the denominator, is `killed + survived + timeout`.
- `unviable` is outside M: the mutant did not compile (a replacement value with no `Default`, a
  type the operator swap broke). It says nothing about the test.
- `timeout` is inside M but is not a kill: cargo-mutants gave the mutant five times the
  baseline's time and it did not finish. A mutant that turns a loop infinite is the usual cause;
  mention it, do not count it as detected.
- For mutation-under-proof the list is not cargo-mutants' (it cannot see inside `verus!`); the
  operator set is in `proof-route.md`, and the count beyond `max_mutants` arrives as `unsampled`.
