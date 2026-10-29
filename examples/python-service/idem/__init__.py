"""idem: an idempotency-key store with a time to live.

A client that retries a request sends the same idempotency key. The service
must hand the retry the first response, never do the work twice, and refuse
a different request that reuses a key.
"""

from idem.fingerprint import fingerprint
from idem.store import Conflict, Decision, IdempotencyStore, InProgress, Proceed, Replay, Response

__all__ = [
    "Conflict",
    "Decision",
    "IdempotencyStore",
    "InProgress",
    "Proceed",
    "Replay",
    "Response",
    "fingerprint",
]
