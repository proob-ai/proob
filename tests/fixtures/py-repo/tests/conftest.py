"""Shared generators, registered for `st.from_type`: the analogue of the Rust
fixture's `tests/common/mod.rs`, and a member of every binding set under
`tests/` (a `conftest.py` on the path to the test file is one by rule)."""

from hypothesis import strategies as st

from interval.merge import Interval

BOUND = 20


def intervals() -> st.SearchStrategy[Interval]:
    return st.integers(0, BOUND).flatmap(lambda lo: st.builds(Interval, st.just(lo), st.integers(lo, BOUND)))


st.register_type_strategy(Interval, intervals())
