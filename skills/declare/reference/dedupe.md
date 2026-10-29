# Statements never to propose again

Run from the repository root; `default` is the default branch from step 1
of SKILL.md. Collect every `id` and `statement` these print. A candidate
that means the same as any of them, in any wording, is out.

## 1. Existing property files

```sh
grep -H -E '^(id|statement) =' proob/properties/*.toml 2>/dev/null
```

## 2. Files deleted from the default branch

Each deleted file's last content, read from the parent of the commit that
deleted it:

```sh
git fetch -q origin "$default" 2>/dev/null
ref="origin/$default"; git rev-parse -q --verify "$ref" >/dev/null 2>&1 || ref="$default"
git log --diff-filter=D --format=%H --name-only "$ref" -- proob/properties/ 2>/dev/null \
  | awk 'NF' | awk '/^[0-9a-f]{40}$/{c=$0;next}{print c":"$0}' \
  | while IFS=: read -r c p; do git show "$c^:$p" 2>/dev/null | grep -E '^(id|statement) ='; done
```

## 3. `reject <ULID>` replies on past pull requests

The pull requests that touched `proob/properties/`, then every conversation
comment, review body, and inline review comment on them:

```sh
gh pr list --state all --limit 200 --json number,files \
  --jq '.[] | select(any(.files[]; .path | startswith("proob/properties/"))) | .number' \
| while read -r n; do
    gh api "repos/{owner}/{repo}/issues/$n/comments" --jq '.[].body'
    gh api "repos/{owner}/{repo}/pulls/$n/reviews" --jq '.[].body'
    gh api "repos/{owner}/{repo}/pulls/$n/comments" --jq '.[].body'
  done | grep -o -i -E 'reject [0-7][0-9A-HJKMNP-TV-Z]{25}' | awk '{print toupper($2)}' | sort -u
```

For each rejected ULID, the statement it named, from the last commit that
added or changed the file on any branch still present:

```sh
c=$(git log --all --diff-filter=AM --format=%H -- "proob/properties/$ulid.toml" | head -1)
[ -n "$c" ] && git show "$c:proob/properties/$ulid.toml" | grep -E '^statement ='
```

If the branch is gone, `gh pr view <n> --json headRefOid -q .headRefOid`
gives the head commit and
`gh api "repos/{owner}/{repo}/contents/proob/properties/$ulid.toml?ref=<sha>" --jq .content | base64 -d`
reads the file.

When `gh` is missing, unauthenticated, or offline, step 3 cannot run: say so
in the PR body's "Not proposed" section instead of guessing.
