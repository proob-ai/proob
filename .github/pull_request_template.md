<!--
Pull requests go into `develop` (CONTRIBUTING.md, "Branches").
The title is a Conventional Commit subject, checked by CI: `type(scope): description`.
-->

## Summary

<!-- What changes and why, in a few sentences. -->

## Linked issue

<!-- Fixes #123, or Refs #123. A change in behavior should have an issue first. -->

## Type

<!-- Keep the one that applies. -->

- [ ] `feat`: a new capability
- [ ] `fix`: a bug fix
- [ ] `docs`: documentation only
- [ ] `refactor`: no change in behavior
- [ ] `perf`: faster or smaller, same behavior
- [ ] `test`: tests, fixtures, or the simulation only
- [ ] `ci` / `build` / `chore`: workflows, xtask, dependencies, tooling
- [ ] `bench`: the benchmark or the regression corpus

## Checklist

- [ ] `cargo xtask ci` passes locally (or the stages it could not run are named below, with why).
- [ ] Tests are added or updated for the change; no existing test was weakened to make it pass.
- [ ] Docs are updated. If `README.md` changed, every `README.<language>.md` is updated too and `cargo xtask readme-sync` passes.
- [ ] `CHANGELOG.md` has an entry under `## Unreleased` (not needed for `docs`, `test`, `ci`, `chore`).
- [ ] The title, and every commit, is a Conventional Commit subject (`cargo xtask commit-lint origin/develop..HEAD`).
- [ ] `area: core` only: `cargo xtask verify` passes under `--no-cheating`, and `crates/proob-core/ASSERTIONS.md` is updated if a guarantee changed.
- [ ] Skills only: an eval case under `evals/` is added or updated for the changed behavior.
- [ ] Detection changes only (declare, falsify, adapters): tuned on the `dev` split of `bench/bugs.json` only, and new catches are added to `bench/corpus/`.

## Notes for the reviewer

<!-- Anything you are unsure about, stages you could not run locally, or follow-ups. -->
