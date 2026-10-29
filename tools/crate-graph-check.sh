#!/usr/bin/env bash
# The layering rule, checked on the resolved dependency graph (normal deps only):
#
#   1. proob-core depends directly on nothing but vstd and proob-trusted.
#      verus_builtin is tolerated as a direct dependency because some Verus
#      releases require it alongside vstd.
#   2. proob-adapters never depends, directly or transitively, on proob-effects.
#   3. proob-facts never depends directly on proob-trusted (it reaches the hash
#      only through proob-core) and never reaches proob-effects or proob-spawn.
#   4. proob-effects never depends directly on proob-trusted: it may depend on
#      proob-core, proob-facts, proob-spawn, and third-party crates only.
#   5. proob-spawn depends on no proob crate: it is a leaf both adapters and
#      effects can use without reaching each other.
#
# A crate that does not exist yet is skipped with a note, so the rule is in
# force before the crate is. Exit 1 on any violation.
#
# Every cargo invocation is captured into a variable before it is searched.
# Under `pipefail`, `cargo … | grep -q` would report failure whenever grep
# matched early and cargo received SIGPIPE, which both skipped rules and hid
# violations once the metadata grew; a here-string has no upstream process to
# signal.
set -euo pipefail
cd "$(dirname "$0")/.."

readonly ALLOWED_CORE_DEPS="vstd verus_builtin proob-trusted"
status=0

crate_exists() {
  local meta
  meta=$(cargo metadata --no-deps --format-version 1 2>/dev/null) || return 1
  [[ "$meta" == *"\"name\":\"$1\""* ]]
}

# Crate names of the direct normal dependencies of $1, one per line.
direct_deps() {
  local tree
  tree=$(cargo tree -p "$1" -e normal --depth 1 --prefix none --no-dedupe 2>/dev/null) || return 1
  awk 'NR > 1 { print $1 }' <<< "$tree" | sort -u
}

# All crate names reachable through normal dependencies of $1.
transitive_deps() {
  local tree
  tree=$(cargo tree -p "$1" -e normal --prefix none 2>/dev/null) || return 1
  awk 'NR > 1 { print $1 }' <<< "$tree" | sort -u
}

# $1: a newline-separated list, $2: an exact crate name. True when listed.
lists() {
  grep -qx -- "$2" <<< "$1"
}

allowed() {
  local name=$1 a
  for a in $ALLOWED_CORE_DEPS; do [ "$name" = "$a" ] && return 0; done
  return 1
}

echo "crate-graph: rule 1, proob-core direct dependencies must be within: $ALLOWED_CORE_DEPS"
core_deps=$(direct_deps proob-core)
while IFS= read -r dep; do
  [ -n "$dep" ] || continue
  if allowed "$dep"; then
    echo "  ok       $dep"
  else
    echo "  VIOLATION proob-core depends on $dep" >&2
    status=1
  fi
done <<< "$core_deps"

echo "crate-graph: rule 2, proob-adapters must not reach proob-effects"
if crate_exists proob-adapters; then
  adapters_reach=$(transitive_deps proob-adapters)
  if lists "$adapters_reach" proob-effects; then
    echo "  VIOLATION proob-adapters reaches proob-effects" >&2
    status=1
  else
    echo "  ok       proob-adapters does not reach proob-effects"
  fi
else
  echo "  skipped  proob-adapters does not exist yet"
fi

echo "crate-graph: rule 3, proob-facts must not depend on proob-trusted directly nor reach proob-effects or proob-spawn"
if crate_exists proob-facts; then
  facts_direct=$(direct_deps proob-facts)
  facts_reach=$(transitive_deps proob-facts)
  if lists "$facts_direct" proob-trusted; then
    echo "  VIOLATION proob-facts depends directly on proob-trusted" >&2
    status=1
  else
    echo "  ok       proob-facts does not depend directly on proob-trusted"
  fi
  for forbidden in proob-effects proob-spawn; do
    if lists "$facts_reach" "$forbidden"; then
      echo "  VIOLATION proob-facts reaches $forbidden" >&2
      status=1
    else
      echo "  ok       proob-facts does not reach $forbidden"
    fi
  done
else
  echo "  skipped  proob-facts does not exist yet"
fi

echo "crate-graph: rule 4, proob-effects must not depend directly on proob-trusted"
if crate_exists proob-effects; then
  effects_direct=$(direct_deps proob-effects)
  if lists "$effects_direct" proob-trusted; then
    echo "  VIOLATION proob-effects depends directly on proob-trusted" >&2
    status=1
  else
    echo "  ok       proob-effects does not depend directly on proob-trusted"
  fi
else
  echo "  skipped  proob-effects does not exist yet"
fi

echo "crate-graph: rule 5, proob-spawn must depend on no proob crate"
if crate_exists proob-spawn; then
  spawn_reach=$(transitive_deps proob-spawn)
  spawn_proob=$(grep '^proob-' <<< "$spawn_reach" || true)
  if [ -n "$spawn_proob" ]; then
    echo "  VIOLATION proob-spawn reaches $(tr '\n' ' ' <<< "$spawn_proob")" >&2
    status=1
  else
    echo "  ok       proob-spawn reaches no proob crate"
  fi
else
  echo "  skipped  proob-spawn does not exist yet"
fi

[ "$status" -eq 0 ] && echo "crate-graph: ok" || echo "crate-graph: FAILED" >&2
exit "$status"
