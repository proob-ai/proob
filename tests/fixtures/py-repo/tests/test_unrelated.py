"""A plain test with no token. It would kill mutants of `interval/count.py`,
and the vacuous property's score must not count them."""

from interval.count import count_covering
from interval.merge import Interval


def test_count_covering_is_exact():
    xs = [Interval(0, 2), Interval(2, 5), Interval(7, 7)]
    assert count_covering(xs, 2) == 2
    assert count_covering(xs, 6) == 0
    assert count_covering(xs, 7) == 1
    assert count_covering([], 0) == 0
