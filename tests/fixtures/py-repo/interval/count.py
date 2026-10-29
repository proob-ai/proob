"""Counting the intervals that cover a point: the code under the fixture's
vacuous property, which an untokened test would kill mutants of."""

from interval.merge import Interval


def count_covering(intervals: list[Interval], x: int) -> int:
    return sum(1 for i in intervals if i.lo <= x <= i.hi)
