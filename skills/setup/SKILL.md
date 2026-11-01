---
name: setup
description: Install what proob's checks need on this machine. Runs `proob doctor` to find which configured adapters cannot run here (cargo-nextest and cargo-mutants, the pinned pytest, hypothesis, and mutmut, Kani, Verus, Java and TLC for TLA+), shows each install command with its source and size, runs each one only after the user says yes, then sets PROOB_TLA2TOOLS or PROOB_VERUS_DIR and runs doctor again. Use when a check errors with a missing tool, after `proob init`, or for "set up proob", "install TLC", "install Kani".
argument-hint: "[adapter]"
allowed-tools: Read Bash
---

# /proob:setup

Gets this machine ready to run the checks `proob/config.toml` configures.
`proob doctor` decides what is missing; this skill installs it, one approved
step at a time. `$ARGUMENTS` is an optional adapter (`rust`, `python`,
`kani`, `verus`, `tla`) to limit the work to.

## Boundaries

- Install only what doctor reports as not ready, at the version its `reason`
  names. Nothing else: no extra tools, no newer releases, no upgrades of
  tools that are ready.
- Nothing is downloaded or installed until the user has approved that step
  in this conversation. One approval covers only the steps listed when it
  was given.
- Never run `sudo`, and never run a script piped from the network
  (`curl … | sh`). When a step needs either (a system package, rustup),
  print the command and ask the user to run it themselves, as
  `! <command>` in this session.
- Never edit a shell profile, `.claude/settings*.json`, or any file in the
  repository without asking first. Setup changes this machine, never the
  repository: `proob/config.toml` and the workflows belong to `proob init`,
  and the workflows install their own pinned tools in CI.
- A download whose SHA-256 does not match the pin is deleted and never used.
  Say so, and stop.

## Procedure

1. Find the binary, as the other skills do.
   ```sh
   root=$(git rev-parse --show-toplevel) && cd "$root"
   proob="${CLAUDE_PLUGIN_ROOT}/scripts/proob"; [ -x "$proob" ] || proob="${PROOB_BIN:-proob}"
   ```

2. Ask doctor.
   ```sh
   "$proob" doctor --json
   ```
   - Exit 0: every adapter is ready. Print each one's versions and stop.
   - Exit 2, or `adapters` is empty: there is nothing to set up yet. Say
     `proob init` (or `/proob:try`, which needs no setup in the repository)
     comes first, and stop.
   - Exit 1: continue with the adapters whose `ready` is false, and with
     `disk` when its `ok` is false.

3. Plan. For each adapter that is not ready, take its `reason`. The reason
   names the first missing tool and the exact command that installs it.
   Match it against the table below for the source and the size. A later
   tool in the same adapter shows up only after the first one is installed,
   so expect to run doctor more than once.

   | `reason` begins with | Step | Source, size |
   |---|---|---|
   | `cargo was not found` | the user installs Rust from https://rustup.rs | rustup's installer script; about 1 GB with a toolchain |
   | `cargo-nextest is not installed`, `cargo-mutants is not installed` | the `cargo install --locked … --version …` command in the reason | crates.io, built from source; a few minutes each |
   | `python3 was not found` | the user installs Python 3.10 or newer | python.org or the system's package manager |
   | `… is not installed for python3`, `… is installed but this proob drives …` | the `python3 -m pip install …` command in the reason | PyPI; a few MB. If pip refuses with "externally-managed-environment", propose `python3 -m venv .venv` and installing into it, and say the venv must be active wherever proob runs |
   | `the Kani toolchain was not found`, `cargo-kani … is installed but` | the `cargo install --locked kani-verifier --version …` and `cargo kani setup` commands in the reason | crates.io, then Kani's release bundle and a Rust nightly from GitHub and rustup; about 3 GB of disk, and a proof can use several GB of memory |
   | `the Verus toolchain was not found` | the Verus release that the repository's `vstd` dependency was built for, unpacked into its own directory, with `PROOB_VERUS_DIR` set to it | https://github.com/verus-lang/verus/releases (check its SHA-256 against the release's digest); about 100 MB |
   | `Java was not found`, `` `java -version` failed `` | macOS: `brew install openjdk` (Homebrew), else the user installs a Java 11 or later runtime | Homebrew or the system's package manager; about 200 MB |
   | `TLC was not found`, `the jar PROOB_TLA2TOOLS names is not TLA+` | download the pinned jar (below) and set `PROOB_TLA2TOOLS` to it | the URL and the SHA-256 in the reason; about 2 MB |

   Before any step over 1 GB, read `disk` from the doctor output: when it is
   not ok, say so and do not plan the step.

4. Ask. Show the plan as one list: for each step, the tool, the exact
   command, where it downloads from, and its size. Copy URLs, versions, and
   SHA-256 digests in full, never shortened: the user checks them. Ask whether to run all of
   it, some of it, or none. Wait for the answer. Steps the user runs
   themselves are listed with their `! <command>` form.

5. Run each approved step on its own, in the order listed. On a failure,
   stop there, quote the last lines of the output, and say what is still
   missing. Do not try a different way to install the tool.

   The TLC jar, with the version, URL, and SHA-256 copied from the reason:
   ```sh
   dir="$HOME/.proob/tools"; jar="$dir/tla2tools-<version>.jar"
   mkdir -p "$dir" && curl -fsSL --retry 3 -o "$jar.part" "<url>"
   echo "<sha256>  $jar.part" | shasum -a 256 -c - && mv "$jar.part" "$jar"
   ```
   (`sha256sum -c -` where there is no `shasum`.) If the check fails, delete
   `$jar.part`, say the download did not match the pin, and stop.

6. Set the variables. `PROOB_TLA2TOOLS` (and `PROOB_VERUS_DIR` for Verus)
   must be set wherever proob runs: in the user's shell and in Claude Code's.
   For the rest of this session, put them in front of every proob command.
   To keep them, offer to append the `export` line to the user's shell
   profile (name the file and show the line), and do it only on a yes. Say
   that Claude Code picks the variable up when it is next started from that
   shell.

7. Check again: `"$proob" doctor` with the variables set. Repeat steps 3 to 6
   for any adapter whose next tool is now the missing one.

## Report

End with a table: each adapter, `ready` before and after, and the versions
doctor now reports. Then list every command that ran, every file written
outside the repository (the jar, a profile line), and anything left for the
user to do, with its command. Say plainly what is still not ready. Never say
a property holds or is checked: setup installs tools, it runs no check.
