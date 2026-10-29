"""Merging intervals: the code under the fixture's passing property."""

from typing import NamedTuple


class Interval(NamedTuple):
    lo: int
    hi: int


def merge(intervals: list[Interval]) -> list[Interval]:
    """The same points as `intervals`, as the fewest sorted, pairwise-disjoint intervals."""
    merged: list[Interval] = []
    for current in sorted(intervals):
        if merged and current.lo <= merged[-1].hi:
            last = merged[-1]
            merged[-1] = Interval(last.lo, max(last.hi, current.hi))
        else:
            merged.append(current)
    return merged
