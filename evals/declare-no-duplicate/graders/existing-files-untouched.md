---
type: llm
focus: files
---
The four committed property files proob/properties/01K8V5NQ4A0F3R7Y2M9D6P1XHA.toml, ...XHB.toml, ...XHC.toml, and ...XHD.toml must be byte-for-byte unchanged, and proob/config.toml must be unchanged. Any new file under proob/properties/ must be named <26-character ULID>.toml with an id equal to its file name. Fail if a new statement means the same as any of these four existing statements, in any wording: "Sorting a list never adds or drops an element."; "A clamped value is never below the lower bound and never above the upper bound."; "The number of even values in a list is at most the number of values in it."; "Counting evens reports how many values of a list are even." Pass if there are no new files at all (proposing nothing is allowed) or every new statement is a distinct property (for example about sorted output being ordered, sorting keeping the same elements, or clamping being idempotent).
