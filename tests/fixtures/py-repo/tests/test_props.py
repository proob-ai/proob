"""The bound property-based tests. A `# proob:<ULID>` line directly above a
test function (above its decorators) binds that test to the property with
that id; neither a seed nor an example count is set here, so the adapter's
`--hypothesis-seed` and its `proob` settings profile apply."""

from hypothesis import given
from hypothesis import strategies as st

from interval.bucket import TokenBucket
from interval.merge import Interval, merge


# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPA
@given(st.lists(st.from_type(Interval)))
def test_merge_yields_disjoint_sorted_intervals_covering_the_input(intervals):
    merged = merge(intervals)
    assert merged == sorted(merged)
    assert all(a.hi < b.lo for a, b in zip(merged, merged[1:]))
    for x in intervals:
        assert any(m.lo <= x.lo and x.hi <= m.hi for m in merged)
    for m in merged:
        assert any(x.lo == m.lo for x in intervals)
        assert any(x.hi == m.hi for x in intervals)


# proob:01K8V5NQ4A0F3R7Y2M9D6P1XPB
@given(st.lists(st.integers(min_value=0, max_value=10)))
def test_bucket_never_goes_negative(takes):
    bucket = TokenBucket(capacity=5)
    for n in takes:
        bucket.take(n)
        assert bucket.tokens >= 0, f"took {n} with {bucket.tokens + n} left"
