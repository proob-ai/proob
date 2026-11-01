# Installing proob for a teammate

The repository's committed `.claude/settings.json` carries the recommendation:

```json
{
  "extraKnownMarketplaces": {
    "proob": {
      "source": { "source": "github", "repo": "proob-ai/proob" },
      "autoUpdate": true
    }
  },
  "enabledPlugins": { "proob@proob": true }
}
```

What a teammate sees, in order:

1. Opening the repository in Claude Code asks them to trust the folder. Until
   they trust it, nothing in project settings applies.
2. Once trusted, Claude Code knows the `proob` marketplace
   (`extraKnownMarketplaces`). Enabling a plugin in project settings does not
   install it on their machine: the `/plugin` Errors tab shows
   "proob@proob is enabled in project settings but isn't installed here"
   until they install it.
3. They install it once, either with the slash commands

   ```text
   /plugin marketplace add proob-ai/proob
   /plugin install proob@proob
   ```

   or from a shell, scoped to this project:

   ```sh
   claude plugin install proob@proob --scope project
   ```

4. From the next session on they see one line at session start:
   `proob: <counts> [unverified: no GitHub token; ...]`. The proob binary is
   downloaded from GitHub Releases and checksum-verified the first time a
   proob skill runs; the hooks never download, so until then the session line
   reads `proob: proob <version> is not installed yet; run /proob:guard once
   to download it`.
5. For a verified view locally they need a GitHub token with read access to
   the repository: `gh auth login`, or `GITHUB_TOKEN` / `GH_TOKEN` in the
   environment. Without one every rendering carries the `unverified` marker
   and nothing reads Holds; the PR check and the default-branch runs are
   verified regardless.
6. To run checks locally they need the tools each adapter drives (cargo-nextest,
   the pinned Python tools, Kani, Verus, Java and TLC). `proob doctor` lists
   what is missing on their machine; `/proob:setup` installs it, each step
   after they approve it.

Reviewing and approving a declaration needs no plugin at all: an approving
review on the pull request, by someone with write access who did not write
it, is the approval. Proposing (`/proob:sense`, `/proob:declare`) and
running checks locally need the plugin.
