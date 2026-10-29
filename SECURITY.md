# Security policy

## Supported versions

proob is before 1.0. Security fixes go into the latest release only; there are
no backports to earlier `0.x` versions.

| Version | Supported |
|---|---|
| latest `0.x` release | yes |
| older releases | no |

The GitHub Action follows the same rule: pin it to the commit SHA of the latest
release.

## Reporting a vulnerability

Report privately through GitHub: on the repository's **Security** tab, choose
**Report a vulnerability**, or go straight to
<https://github.com/proob-ai/proob/security/advisories/new>. Do not open a
public issue, discussion, or pull request for a vulnerability.

Please include the proob version (`proob --version`), the Action SHA if the
gate or the evidence workflow is involved, and the steps that reproduce it. A
public repository and commit, or a scenario for `cargo xtask sim`, is the most
useful reproduction.

The maintainer acknowledges a report within a week, keeps you informed in the
advisory while a fix is prepared, and credits you in the advisory and the
CHANGELOG unless you ask otherwise. Please give us a reasonable time to release
a fix before you disclose the issue publicly.

## Scope

proob's promise is that `Holds` means a reviewed property, checked by approved
evidence, and able to fail. Anything that breaks that promise is in scope,
including:

- anything that lets proob render `Holds`, or lets the gate pass, when the
  property, its approval, or its evidence does not support it: a forged or
  stale evidence record, evidence accepted from a pull request run or a branch
  other than the default branch, a cache entry that changes a verdict, a parsing
  difference between the approved text and the governed text;
- anything that lets an agent approve its own work, or count as a second
  reviewer: self-signed content read as team-approved, an author counted as an
  eligible reviewer, a direct push treated as reviewed;
- a property file, config, or repository content that can steer a child
  process (arguments read as options, shell injection) or run code outside the
  adapter's own check;
- a GitHub token reaching a job or process that should run without one, or
  write access reaching code from a fork;
- an unsound proof in `crates/proob-core`: a way to make Verus accept it under
  `--no-cheating` while a stated guarantee is false.

Out of scope: bugs in the code under test that proob does or does not find
(file those as **Missed bug** or **False alarm** issues), vulnerabilities in a
third-party tool proob drives (report those to the tool), and attacks that
need write access to the repository's default branch or its settings.

A wrong `Holds` that nobody could cause on purpose is still a critical bug:
file it with the **Soundness report** issue form.
