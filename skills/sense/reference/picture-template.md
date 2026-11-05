# The picture file

`/proob:sense` writes `proob/.cache/sense/picture.md` with exactly these
headings, in this order; `/proob:declare` reads them by name. Keep the
headings verbatim, fill every section (write "none found" rather than
dropping one), and cite every claim. The file lives in the ignored cache and
is never committed.

~~~markdown
# proob picture

- repository: <root path>
- commit: <git rev-parse HEAD>, branch <name>, <date>
- focus: <the path argument, or "whole repository">
- reading: this file is the agent's reading of the code, not a fact about
  the system; nothing in it is approved or checked

## Languages and adapters

| language | signal | proob adapter |
|---|---|---|
| Rust | Cargo.toml | available: proptest |
| Go | go.mod | none: properties render Not checked (no adapter) |

## Entry points

- <what it is and who calls it> — `path:line`

## State that must stay consistent

- <the state, who writes it, who reads it, what breaks when it drifts> — `path:line`

## Interfaces and their laws

For every standard interface a public type implements, name the interface and
state its laws as candidate sentences below. A type that implements an
interface promises its laws, whether or not anyone wrote them down, and bugs
hide where an implementation keeps most of a law but not all of it. Check every
variant of the type (the plain one, not only the ordered or the special one).

| type | interface | laws to state |
|---|---|---|
| <type> | <interface> — `path:line` | <laws> |

Common interfaces and their laws, to start from:

- mapping or dictionary: `keys()`, `values()`, and `items()` describe the same pairs in the same order; `len` matches; lookup agrees with iteration; after any change, the inverse (if any) holds the same pairs flipped
- iterator from both ends (Rust `DoubleEndedIterator`, Python `__reversed__`): reading from the back gives the forward items reversed, also after `take(n)` or `skip(n)`; mixing ends never repeats or loses an item; size hints and `len` agree with what is yielded
- equality, hashing, ordering (`Eq`/`Hash`/`Ord`, `__eq__`/`__hash__`/`__lt__`): equal values hash equally; ordering is total and agrees with equality
- parse and print (serializers, formatters, `Display`/`FromStr`, `__str__`/constructors): printing then parsing gives back an equal value, for every value the type can hold, including the ones that need quoting or escaping
- containers with capacity or size limits: never exceed the limit, never lose an item that was not evicted by the documented rule
- copy, clone, freeze: the copy is equal and independent

Write "none found" when the code implements no such interface.

## Invariants (candidates for declare)

One block per candidate, strongest evidence first.

### <one sentence, present tense, plain language>
- evidence: <name | test | comment | doc | incident | changelog> `path:line` — <what it says>
- evidence: <another source, when there is one>
- severity: <low | medium | high | critical> — <why: who notices, what is lost: money, data, safety, uptime>
  (by the consequence when it is violated, the highest class that applies:
  critical: money moves or is lost, an action that cannot be undone runs (an order, a payment,
  a deletion, a message sent), a security or permission boundary is crossed, or stored data is
  lost or corrupted with no way back;
  high: stored data is wrong but recoverable, a limit or refusal that should stop something does
  not, or a crash or hang on a path users reach;
  medium: a wrong value is shown or returned that a person notices and can redo;
  low: diagnostics, formatting, speed, or an input no caller can produce)
- scope: `<glob>`, `<glob>` (<N> tracked files match)
- kind: <proof | model> — only when the files are already under Verus or a spec exists; otherwise omit this line

## Concurrency and persistence boundaries

- <threads, async tasks, processes, locks, queues, transactions, files, caches> — `path:line`

## Existing tests

- <framework and runner> — <where>; property-based tests: <yes, where | no>

## Existing declaration

- headline: `<the headline from proob status --json, verbatim>` (<GitHub records consulted | local, unverified: reason>)
- `<ULID>` — "<statement>"
(or: no declaration yet)

## Excluded paths

<count> tracked paths skipped as secret-bearing, by name only:
- `path`

## Open questions

- <what a human should confirm before approving anything above>
~~~
