#!/usr/bin/env bash
# Negative tests for tools/crate-graph-check.sh. Each case copies the workspace
# to a throwaway directory, adds one forbidden dependency line under a crate's
# `[dependencies]`, and requires the check to fail:
#
#   1. proob-core + serde            (rule 1)
#   2. proob-facts + proob-trusted   (rule 3)
#   3. proob-effects + proob-trusted (rule 4)
#   4. proob-spawn + proob-core      (rule 5)
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"

# $1: crate directory, $2: dependency line to add, $3: label
expect_violation() {
  local crate=$1 line=$2 label=$3 tmp manifest
  tmp=$(mktemp -d)
  (cd "$root" && tar --exclude=./target --exclude=./.git -cf - .) | (cd "$tmp" && tar -xf -)
  manifest="$tmp/crates/$crate/Cargo.toml"
  awk -v line="$line" '{ print } /^\[dependencies\]$/ { print line }' "$manifest" > "$manifest.new"
  mv "$manifest.new" "$manifest"
  grep -qF -- "$line" "$manifest"

  echo "crate-graph-negative: running the check against $label (must fail)"
  if (cd "$tmp" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}" tools/crate-graph-check.sh); then
    echo "crate-graph-negative: FAILED, the check accepted $label" >&2
    rm -rf "$tmp"
    exit 1
  fi
  echo "crate-graph-negative: ok, the check rejected $label"
  rm -rf "$tmp"
}

expect_violation proob-core 'serde = "1"' "serde in proob-core"
if [ -d "$root/crates/proob-facts" ]; then
  expect_violation proob-facts 'proob-trusted.workspace = true' "proob-trusted in proob-facts"
else
  echo "crate-graph-negative: skipped proob-facts case, crate does not exist yet"
fi
if [ -d "$root/crates/proob-effects" ]; then
  expect_violation proob-effects 'proob-trusted.workspace = true' "proob-trusted in proob-effects"
else
  echo "crate-graph-negative: skipped proob-effects case, crate does not exist yet"
fi
if [ -d "$root/crates/proob-spawn" ]; then
  expect_violation proob-spawn 'proob-core.workspace = true' "proob-core in proob-spawn"
else
  echo "crate-graph-negative: skipped proob-spawn case, crate does not exist yet"
fi
