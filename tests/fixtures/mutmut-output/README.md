# Captured mutmut outputs

What mutmut `3.8.0` (the pin in `tools/python-tools.txt`) wrote on `tests/fixtures/py-repo`, for the parser tests in `crates/proob-facts/src/parsers/mutmut.rs` and the adapter tests in `crates/proob-adapters/tests/python.rs`. Each `.meta` is the sidecar mutmut writes at `mutants/<source file>.meta`; its `exit_code_by_key` is the only key proob reads. Regenerate with the same commands after a mutmut pin bump.

Commands, run from a copy of the fixture with `[tool.mutmut]` appended to its `pyproject.toml` as the adapter writes it (`source_paths = ["interval"]`, `only_mutate` and `pytest_add_cli_args_test_selection` as below, `pytest_add_cli_args = ["-p", "no:cacheprovider", "-p", "proob_hypothesis", "--hypothesis-profile", "proob", "--hypothesis-seed", "<seed>"]`, `use_git_change_detection = false`), the profile plugin on `PYTHONPATH`, then `python3 -m mutmut run`:

| File | `only_mutate` | Selected test | Result |
|---|---|---|---|
| `merge.py.meta` | `interval/merge.py` | `tests/test_props.py::test_merge_yields_disjoint_sorted_intervals_covering_the_input` | exit 0; all 21 mutants exit code 1 (killed) |
| `bucket.py.meta` | `interval/bucket.py` | `tests/test_unrelated.py::test_count_covering_is_exact` | exit 0; all 12 mutants exit code 33 (no tests: the selected test never calls `TokenBucket`); method keys use `ǁ` |
| `count.py.meta` | `interval/count.py` | `tests/test_vacuous.py::test_lists_are_short` | exit 1; mutants generated and never run (`null`); stdout in `unreached.stdout.txt` |
| `baseline-failed.stdout.txt` | `interval/bucket.py` | `tests/test_props.py::test_bucket_never_goes_negative` | exit 1; the bound test fails before any mutation (`failed to collect stats`) |

What the adapter reads from them: exit code 1 or 3 is killed, 0 survived, 33 (or 5) no tests, 36 and the SIGXCPU codes timeout, `null` not checked, as mutmut 3.8.0's `status_by_exit_code` says; the two stdout captures carry the messages the adapter matches to tell a test that reaches no mutant (every mutant survives) from a failing baseline (nothing is scored).
