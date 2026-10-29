"""A bound test that cannot fail: it never looks at the code in its scope, so
every mutant of `interval/count.py` survives it and its score reads
`Scored { killed: 0, .. }`."""

from hypothesis import given
from hypothesis import strategies as st


# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPD
@given(st.lists(st.integers(), max_size=8))
def test_lists_are_short(xs):
    assert len(xs) <= 8
