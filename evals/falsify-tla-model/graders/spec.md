---
type: llm
weight: 3
---
Score 1 only if all of the following hold; otherwise 0.

1. A `.tla` module and a `.cfg` with the same base name sit under `proob/specs/`, and the comment line `\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XTM` sits directly above a definition with no parameters that starts at the beginning of its line; the token appears nowhere else.
2. The bound definition says what the statement says ("A payment is charged at most once, however many times the client resends it"): the number of charges for one payment (one idempotency key) is at most one. It is not an invariant that holds by the types alone (such as `TypeOK`), and not one weaker than the statement.
3. The spec models the protocol in `payments/`: the client sending and resending the same request with the same key, a request or a reply being lost, and the server charging only when the key is not yet recorded. Each action carries a TLA+ comment citing the code it models by file (with line numbers or the function), in `payments/server.py` or `payments/client.py`.
4. The `.cfg` gives every constant a value with a `\*` comment saying why, and the values let a resend happen after a lost request or reply (for example at least two attempts, or the three of `MAX_ATTEMPTS` in `payments/client.py`). It has no `CONSTRAINT` or `ACTION_CONSTRAINT` that excludes resends, and any `CHECK_DEADLOCK FALSE` has a comment saying why stopping is intended.
5. No file under `proob/properties/` and no file under `payments/` was modified.
