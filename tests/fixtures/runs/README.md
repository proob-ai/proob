# Evidence run fixtures

One scenario per clause of evidence acceptance (`proob-core::evidence_accept`)
and one per retrieval rule (`proob-effects::evidence`). Each file holds
workflow-run payloads as GitHub's runs endpoint answers them, artifact
listings, the JSON records inside each artifact (`artifact_lines`, written
as the lines of `evidence.jsonl` in a zip by the fixture API), and the blob
of the evidence workflow at each run's head (`blobs`, as git would answer).
Expected outcomes live in `crates/proob-effects/tests/runs.rs`.

The approved workflow blob is `8` repeated; `9` repeated is some other
content. Heads are `f`, `1f`, `2f`, `3f` repeated. The property under test is
`01J9Z3W1V8Q4K7N2M5R8T1X4C6`; `...C8` is another property.

| Scenario | Clause | Accepted | Retrieval |
|---|---|---|---|
| push-on-default | push, default branch, success, approved blob | yes | one check record; latest run head `f` |
| schedule-on-default | schedule event | yes | check and score records |
| feature-branch | head branch is not the default | no | nothing |
| other-workflow-path | another workflow's run | no | nothing |
| failed-conclusion | conclusion is not success | no | nothing |
| unapproved-workflow-blob | the head ran a blob other than the approved one | no | nothing |
| matching-record-two-runs-back | three accepted runs; our record is in the oldest; a line with an unknown kind is tolerated | yes | records from run 101; latest run head `2f` |
| malformed-line | a line that is not a record | yes | refused, naming run 104, artifact 9004, line 2 |

`runs.rs` also checks that an `Unreviewed` workflow approval accepts no run
and that runs older than the freshness window are skipped.
