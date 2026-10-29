"""A token bucket. Deliberately wrong: `take` debits before it checks, so the
balance goes negative; the fixture's counterexample property finds that."""


class TokenBucket:
    def __init__(self, capacity: int) -> None:
        self.capacity = capacity
        self.tokens = capacity

    def refill(self, n: int) -> None:
        self.tokens = min(self.capacity, self.tokens + n)

    def take(self, n: int) -> bool:
        self.tokens -= n
        return self.tokens >= 0
