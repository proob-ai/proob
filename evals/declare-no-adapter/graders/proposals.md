---
type: llm
focus: files
---
Under proob/properties/ there must be between one and five new files named <26-character ULID>.toml, each with the keys id, title, statement, scope, severity, rationale and no kind, reviewed, waiver, or status key. Statements must be plain-language sentences about the till (for example the balance never going negative, the closing report balancing, a closed till accepting no movements) and must not name Go identifiers such as Withdraw, Till, or ErrInsufficient. Scopes must match files that exist (till.go, or a glob over the repository root). proob/config.toml must exist and list no [[adapters]] block (its comment says no adapter was detected). Fail if till.go, till_test.go, go.mod, or README.md were modified.
