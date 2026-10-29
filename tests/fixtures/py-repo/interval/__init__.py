"""A tiny interval library: the Python fixture proob's adapter is tested against."""

from interval.bucket import TokenBucket
from interval.count import count_covering
from interval.merge import Interval, merge

__all__ = ["Interval", "TokenBucket", "count_covering", "merge"]
