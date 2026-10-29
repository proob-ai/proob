---
type: llm
focus: files
---
The workspace after the run must differ from the committed fixture in exactly one way: a new file proob/.cache/sense/picture.md. Fail if any other file was created or modified (a new property file under proob/properties/, an edited source or test file, a changed proob/config.toml, a new commit, or a staged change). The picture must contain the headings "Invariants (candidates for declare)", "Existing declaration", and "Excluded paths", list the four existing property ids under "Existing declaration", and state that it is the agent's reading, not a fact about the system. Fail if the picture quotes the content of any file that looks secret-bearing (.env, *.pem, *.key, secrets/). Pass otherwise.
