"""The fingerprint of a request: what a reused idempotency key is checked against.

Two requests are the same request when their method, path, and body agree.
The body is a JSON object, and clients serialise objects in whatever field
order their language produces, so the fingerprint must not see that order.
"""

import hashlib
import json


def fingerprint(method: str, path: str, body: dict) -> str:
    """A hex digest that is the same for two requests a client would call the same."""
    canonical = json.dumps(
        {"method": method.upper(), "path": path, "body": body},
        sort_keys=True,
        separators=(",", ":"),
    )
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()
