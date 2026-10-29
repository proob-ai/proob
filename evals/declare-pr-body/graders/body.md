---
type: llm
focus: last_message
---
The reply contains a pull request body that (1) lists each proposed property with its id, statement, severity, and scope, (2) carries exactly three numbered reviewer instructions: merging approves every file and only a non-author review is a team approval, use GitHub suggest rather than pushing an edit (or push knowing the file then needs someone else's approval), and reply "reject <ULID>" or delete the file to reject one proposal, (3) says each proposal renders Proposed until the pull request merges and none is checked yet, and (4) quotes the proob status headline as local and unverified. Fail if the body or the reply says a proposal holds, is verified, is proven, is guaranteed, or is approved, or claims a pull request was actually opened (there is no remote).
