#!/usr/bin/env bash
# Proves the verify job can fail. In a throwaway copy of the workspace, the same
# command that must pass on the real crate is run three times:
#
#   1. unchanged                     -> must exit 0
#   2. `assume(false);` injected     -> must fail (rejected by --no-cheating)
#   3. `assert(false);` injected     -> must fail (rejected by the verifier)
#
# The injection point is the anchor comment inside `combine_sorted`, the first
# verified exec function, so the test does not depend on line numbers.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
readonly anchor='// verify-negative-anchor'
readonly file='crates/proob-core/src/scope_hash.rs'
verus_args=(--no-cheating --rlimit 10)

if ! command -v cargo-verus >/dev/null 2>&1; then
  PATH="$("$root/tools/get-verus.sh"):$PATH"
  export PATH
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
(cd "$root" && tar --exclude=./target --exclude=./.git -cf - .) | (cd "$tmp" && tar -xf -)
grep -q "$anchor" "$tmp/$file" || { echo "verify-negative: anchor missing from $file" >&2; exit 1; }
cp "$tmp/$file" "$tmp/scope_hash.rs.pristine"

inject() { # $1: statement to place right after the anchor
  awk -v stmt="$1" -v anchor="$anchor" '{ print } index($0, anchor) { print "    " stmt }' \
    "$tmp/scope_hash.rs.pristine" > "$tmp/$file"
}

verify() { # runs the exact CI command in the copy; returns its exit status
  (cd "$tmp" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}/verify-negative" \
    cargo verus verify -p proob-core --fwd-verus-args-to roots -- "${verus_args[@]}") \
    > "$tmp/out.txt" 2>&1
}

expect() { # $1: pass|fail, $2: label
  local want=$1 label=$2
  if verify; then got=pass; else got=fail; fi
  if [ "$got" = "$want" ]; then
    echo "verify-negative: ok    $label -> $got"
  else
    echo "verify-negative: FAILED $label -> $got, expected $want" >&2
    tail -30 "$tmp/out.txt" >&2
    exit 1
  fi
}

cp "$tmp/scope_hash.rs.pristine" "$tmp/$file"; expect pass "unchanged proob-core"
inject 'assume(false);';                        expect fail "assume(false) under --no-cheating"
grep -q 'not allowed with `--no-cheating`' "$tmp/out.txt" || { echo "verify-negative: assume was rejected for an unexpected reason" >&2; tail -20 "$tmp/out.txt" >&2; exit 1; }
inject 'assert(false);';                        expect fail "assert(false)"
grep -q 'assertion failed' "$tmp/out.txt" || { echo "verify-negative: assert was rejected for an unexpected reason" >&2; tail -20 "$tmp/out.txt" >&2; exit 1; }
echo "verify-negative: ok, the verify job can fail"
