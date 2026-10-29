---
type: llm
focus: files
weight: 2
---
Score 1 only if at least one new file under `proob/properties/` (a 26-character Crockford base32 ULID followed by `.toml`, with `id` equal to the file name) has a `scope` that matches `interval/refunds.py` and a `statement` about refunds against a charge (for example, that the total refunded never exceeds the charge, or that a refund is never negative), in one plain sentence that names no function, class, module, or file. The file must contain only the keys id, title, statement, scope, severity, rationale, and optionally kind; never reviewed, waiver, or status. Otherwise 0.
