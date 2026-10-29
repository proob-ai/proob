"""The idempotency-key store.

`begin` is called before the work, `complete` after it. Every entry carries
the fingerprint of the request that created it and expires `ttl_seconds`
after it was first seen; time is passed in, never read from a clock, so the
store is deterministic and easy to test.
"""

from dataclasses import dataclass


@dataclass(frozen=True)
class Response:
    """What the service answered: kept so a retry gets the same answer."""

    status: int
    body: str


@dataclass(frozen=True)
class Proceed:
    """A new key: do the work, then call `complete`."""


@dataclass(frozen=True)
class Replay:
    """The key was completed before: answer with the stored response, do no work."""

    response: Response


@dataclass(frozen=True)
class InProgress:
    """The key's first request has not completed yet: tell the client to retry later."""


@dataclass(frozen=True)
class Conflict:
    """The key is in use by a different request: refuse."""


Decision = Proceed | Replay | InProgress | Conflict


@dataclass(frozen=True)
class _Entry:
    fingerprint: str
    response: Response | None
    expires_at: float


class IdempotencyStore:
    def __init__(self, ttl_seconds: float) -> None:
        if ttl_seconds <= 0:
            raise ValueError("ttl_seconds must be positive")
        self._ttl = ttl_seconds
        self._entries: dict[str, _Entry] = {}

    def begin(self, key: str, fingerprint: str, now: float) -> Decision:
        """Decide what to do with a request under `key` at time `now`."""
        self.expire(now)
        entry = self._entries.get(key)
        if entry is None:
            self._entries[key] = _Entry(fingerprint, None, now + self._ttl)
            return Proceed()
        if entry.fingerprint != fingerprint:
            return Conflict()
        if entry.response is None:
            return InProgress()
        return Replay(entry.response)

    def complete(self, key: str, response: Response) -> None:
        """Record the response for a key that `begin` told the caller to proceed with."""
        entry = self._entries.get(key)
        if entry is None:
            raise KeyError(f"{key!r} was never begun, or has expired")
        if entry.response is not None:
            raise ValueError(f"{key!r} was already completed")
        self._entries[key] = _Entry(entry.fingerprint, response, entry.expires_at)

    def expire(self, now: float) -> None:
        """Drop every entry whose time to live has passed."""
        self._entries = {k: e for k, e in self._entries.items() if e.expires_at > now}

    def __len__(self) -> int:
        return len(self._entries)

    def __contains__(self, key: str) -> bool:
        return key in self._entries
