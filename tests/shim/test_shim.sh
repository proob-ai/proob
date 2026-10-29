#!/usr/bin/env bash
# Tests for scripts/proob. Needs only sh, curl (file:// URLs), tar with xz, and a
# sha256 tool. No network: the "release" is a fixture directory served as file://.
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
shim="$root/scripts/proob"
. "$root/scripts/lib/fetch.sh"
target=$(proob_target) || { echo "test_shim: unsupported platform" >&2; exit 1; }
version=$(sed -n 's/^PROOB_VERSION="\(.*\)"$/\1/p' "$shim")
[ -n "$version" ] || { echo "test_shim: PROOB_VERSION constant not found in the shim" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
pass=0; fail=0
check() { # $1: description, rest: command
  local what=$1; shift
  if "$@"; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL: $what" >&2; fi
}
one_line_hint() { [ "$(wc -l < "$1")" -eq 1 ] && grep -q '^proob: ' "$1"; }
empty_dir() { [ -z "$(ls -A "$1" 2>/dev/null)" ]; }

# A fake binary that echoes its argv and leaves a marker when it ran.
mkfake() {
  cat > "$1" <<'FAKE'
#!/bin/sh
echo "fake-proob $*"
[ -n "${FAKE_MARK:-}" ] && : > "$FAKE_MARK"
exit 0
FAKE
  chmod +x "$1"
}
unset PROOB_BIN PROOB_HOME PROOB_RELEASE_BASE_URL CLAUDE_PLUGIN_DATA
export PROOB_SKIP_ATTESTATION=1

echo "1. PROOB_BIN set: the shim execs it with all arguments"
mkfake "$tmp/fake1"
out=$(PROOB_BIN="$tmp/fake1" "$shim" --version extra 2>"$tmp/err1"); rc=$?
check "exit 0" test "$rc" -eq 0
check "argv forwarded" test "$out" = "fake-proob --version extra"
check "no stderr" test ! -s "$tmp/err1"

echo "2. PROOB_BIN set to a non-executable: one-line hint, exit 1"
: > "$tmp/notexec"
out=$(PROOB_BIN="$tmp/notexec" "$shim" --version 2>"$tmp/err2"); rc=$?
check "exit 1" test "$rc" -eq 1
check "one proob: line" one_line_hint "$tmp/err2"

echo "3. no cached binary and no network: one-line hint, exit 1, nothing installed"
mkdir -p "$tmp/home3"
out=$(PROOB_HOME="$tmp/home3" PROOB_RELEASE_BASE_URL=http://127.0.0.1:9 "$shim" --version 2>"$tmp/err3"); rc=$?
check "exit 1" test "$rc" -eq 1
check "no stdout" test -z "$out"
check "one proob: line" one_line_hint "$tmp/err3"
check "hint names the download problem" grep -q 'cannot download' "$tmp/err3"
check "cache untouched" empty_dir "$tmp/home3"

echo "4. tampered checksum: refuses to execute, exit 1, nothing installed"
fx="$tmp/release/v$version"; mkdir -p "$fx/proob-$target"
mkfake "$fx/proob-$target/proob"
(cd "$fx" && tar -cJf "proob-$target.tar.xz" "proob-$target") && rm -rf "$fx/proob-$target"
printf '%s  proob-%s.tar.xz\n' "0000000000000000000000000000000000000000000000000000000000000000" "$target" > "$fx/proob-$target.tar.xz.sha256"
mkdir -p "$tmp/home4"
out=$(FAKE_MARK="$tmp/mark4" PROOB_HOME="$tmp/home4" PROOB_RELEASE_BASE_URL="file://$tmp/release" "$shim" --version 2>"$tmp/err4"); rc=$?
check "exit 1" test "$rc" -eq 1
check "one proob: line" one_line_hint "$tmp/err4"
check "hint names the checksum" grep -q '^proob: checksum mismatch' "$tmp/err4"
check "fake never ran" test ! -e "$tmp/mark4"
check "cache untouched" empty_dir "$tmp/home4"

echo "5. missing .sha256: refuses to execute, exit 1"
mv "$fx/proob-$target.tar.xz.sha256" "$tmp/stash.sha256"
out=$(FAKE_MARK="$tmp/mark5" PROOB_HOME="$tmp/home4" PROOB_RELEASE_BASE_URL="file://$tmp/release" "$shim" --version 2>"$tmp/err5"); rc=$?
check "exit 1" test "$rc" -eq 1
check "one proob: line" one_line_hint "$tmp/err5"
check "fake never ran" test ! -e "$tmp/mark5"
mv "$tmp/stash.sha256" "$fx/proob-$target.tar.xz.sha256"

echo "6. correct checksum: installs, caches as proob-<version>, execs; second run needs no network"
sha=$(proob_sha256 "$fx/proob-$target.tar.xz")
printf '%s  proob-%s.tar.xz\n' "$sha" "$target" > "$fx/proob-$target.tar.xz.sha256"
mkdir -p "$tmp/home6"
out=$(FAKE_MARK="$tmp/mark6" PROOB_HOME="$tmp/home6" PROOB_RELEASE_BASE_URL="file://$tmp/release" "$shim" status --json 2>"$tmp/err6"); rc=$?
check "exit 0" test "$rc" -eq 0
check "argv forwarded" test "$out" = "fake-proob status --json"
check "no stderr" test ! -s "$tmp/err6"
check "cached under PROOB_HOME" test -x "$tmp/home6/proob-$version"
check "no leftover partial files" test -z "$(ls "$tmp/home6" | grep -v "^proob-$version$")"
out=$(PROOB_HOME="$tmp/home6" PROOB_RELEASE_BASE_URL=http://127.0.0.1:9 "$shim" again 2>"$tmp/err6b"); rc=$?
check "cached run exit 0 offline" test "$rc" -eq 0
check "cached run argv forwarded" test "$out" = "fake-proob again"

echo "7. CLAUDE_PLUGIN_DATA sets the default cache location"
mkdir -p "$tmp/data7"
out=$(CLAUDE_PLUGIN_DATA="$tmp/data7" PROOB_RELEASE_BASE_URL="file://$tmp/release" "$shim" x 2>"$tmp/err7"); rc=$?
check "exit 0" test "$rc" -eq 0
check "installed under CLAUDE_PLUGIN_DATA/bin" test -x "$tmp/data7/bin/proob-$version"
echo "8. PROOB_NO_FETCH set and nothing cached: one-line hint, exit 1, nothing downloaded"
mkdir -p "$tmp/home8"
out=$(PROOB_NO_FETCH=1 PROOB_HOME="$tmp/home8" PROOB_RELEASE_BASE_URL="file://$tmp/release" "$shim" --version 2>"$tmp/err8"); rc=$?
check "exit 1" test "$rc" -eq 1
check "no stdout" test -z "$out"
check "one proob: line" one_line_hint "$tmp/err8"
check "hint says not installed and how to install" grep -q 'not installed yet.*proob:guard' "$tmp/err8"
check "cache untouched although the release is reachable" empty_dir "$tmp/home8"

echo "9. PROOB_NO_FETCH set with a cached binary: runs it, no network"
out=$(PROOB_NO_FETCH=1 PROOB_HOME="$tmp/home6" PROOB_RELEASE_BASE_URL=http://127.0.0.1:9 "$shim" cached 2>"$tmp/err9"); rc=$?
check "exit 0" test "$rc" -eq 0
check "argv forwarded" test "$out" = "fake-proob cached"

echo "shim tests: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
