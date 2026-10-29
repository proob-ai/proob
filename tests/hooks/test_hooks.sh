#!/usr/bin/env bash
# Tests for scripts/hooks/*.sh: the no-binary path (the shim's hint as the
# SessionStart systemMessage; nothing, which is allow, on PreToolUse), the
# pass-through path with a binary, and exit 0 throughout. No network and no
# proob binary needed: PROOB_HOME is an empty directory and the hooks set
# PROOB_NO_FETCH, so the shim never tries to download.
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
start="$root/scripts/hooks/session-start.sh"
pre="$root/scripts/hooks/pre-tool-use.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
pass=0; fail=0
check() { # $1: description, rest: command
  local what=$1; shift
  if "$@"; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL: $what" >&2; fi
}
lines() { printf '%s' "$1" | awk 'END { print NR }'; }
valid_json() { # best effort: python3 is on both CI runners; absent, the shape checks stand alone
  if command -v python3 >/dev/null 2>&1; then printf '%s' "$1" | python3 -I -c 'import json,sys; json.load(sys.stdin)' 2>/dev/null; else true; fi
}
empty_dir() { [ -z "$(ls -A "$1" 2>/dev/null)" ]; }

unset PROOB_BIN CLAUDE_PLUGIN_DATA
mkdir -p "$tmp/home"
export CLAUDE_PLUGIN_ROOT="$root" PROOB_HOME="$tmp/home" PROOB_RELEASE_BASE_URL=http://127.0.0.1:9 PROOB_SKIP_ATTESTATION=1
session_payload="{\"session_id\":\"s\",\"cwd\":\"$tmp\",\"hook_event_name\":\"SessionStart\",\"source\":\"startup\"}"
pre_payload="{\"session_id\":\"s\",\"cwd\":\"$tmp\",\"hook_event_name\":\"PreToolUse\",\"tool_name\":\"Write\",\"tool_input\":{\"file_path\":\"$tmp/proob/properties/x.toml\",\"content\":\"status = \\\"holds\\\"\\n\"}}"

echo "1. no binary: SessionStart emits the shim's hint as systemMessage, exits 0"
out=$(printf '%s' "$session_payload" | "$start" 2>"$tmp/err1"); rc=$?
check "exit 0" test "$rc" -eq 0
check "one line" test "$(lines "$out")" -eq 1
check "systemMessage carries the proob: hint" grep -q '^{"systemMessage":"proob: ' <<<"$out"
check "hint says how to install" grep -q 'not installed yet' <<<"$out"
check "valid JSON" valid_json "$out"
check "nothing downloaded or installed" empty_dir "$tmp/home"

echo "2. no binary: PreToolUse emits nothing (allow), exits 0"
out=$(printf '%s' "$pre_payload" | "$pre" 2>"$tmp/err2"); rc=$?
check "exit 0" test "$rc" -eq 0
check "no stdout" test -z "$out"
check "no stderr" test ! -s "$tmp/err2"

echo "3. binary present: both hooks pass argv and stdin through and print its stdout"
cat > "$tmp/fake" <<'FAKE'
#!/bin/sh
in=$(cat)
printf '{"systemMessage":"fake %s %s"}\n' "$*" "$(printf '%s' "$in" | wc -c | tr -d ' ')"
FAKE
chmod +x "$tmp/fake"
n=$(printf '%s' "$session_payload" | wc -c | tr -d ' ')
out=$(printf '%s' "$session_payload" | PROOB_BIN="$tmp/fake" "$start" 2>"$tmp/err3"); rc=$?
check "SessionStart exit 0" test "$rc" -eq 0
check "SessionStart stdout is the binary's" test "$out" = "{\"systemMessage\":\"fake hook session-start $n\"}"
n=$(printf '%s' "$pre_payload" | wc -c | tr -d ' ')
out=$(printf '%s' "$pre_payload" | PROOB_BIN="$tmp/fake" "$pre" 2>"$tmp/err3b"); rc=$?
check "PreToolUse exit 0" test "$rc" -eq 0
check "PreToolUse stdout is the binary's" test "$out" = "{\"systemMessage\":\"fake hook pre-tool-use $n\"}"

echo "4. binary fails: SessionStart turns its proob: line into the message, PreToolUse stays silent; exit 0"
cat > "$tmp/broken" <<'BROKEN'
#!/bin/sh
printf 'proob: a "quoted" path\\with backslash\n' >&2
exit 1
BROKEN
chmod +x "$tmp/broken"
out=$(printf '%s' "$session_payload" | PROOB_BIN="$tmp/broken" "$start" 2>"$tmp/err4"); rc=$?
check "SessionStart exit 0" test "$rc" -eq 0
check "quotes and backslashes escaped" test "$out" = '{"systemMessage":"proob: a \"quoted\" path\\with backslash"}'
check "valid JSON" valid_json "$out"
out=$(printf '%s' "$pre_payload" | PROOB_BIN="$tmp/broken" "$pre" 2>"$tmp/err4b"); rc=$?
check "PreToolUse exit 0" test "$rc" -eq 0
check "PreToolUse no stdout" test -z "$out"

echo "5. CLAUDE_PLUGIN_ROOT points nowhere: SessionStart hints, PreToolUse stays silent; exit 0"
out=$(printf '%s' "$session_payload" | CLAUDE_PLUGIN_ROOT="$tmp/nowhere" "$start" 2>"$tmp/err5"); rc=$?
check "SessionStart exit 0" test "$rc" -eq 0
check "hint names the missing scripts" grep -q '^{"systemMessage":"proob: .*missing' <<<"$out"
out=$(printf '%s' "$pre_payload" | CLAUDE_PLUGIN_ROOT="$tmp/nowhere" "$pre" 2>"$tmp/err5b"); rc=$?
check "PreToolUse exit 0" test "$rc" -eq 0
check "PreToolUse no stdout" test -z "$out"

echo "6. empty stdin: both hooks exit 0"
out=$(: | "$start" 2>/dev/null); rc=$?
check "SessionStart exit 0" test "$rc" -eq 0
out=$(: | "$pre" 2>/dev/null); rc=$?
check "PreToolUse exit 0" test "$rc" -eq 0
check "PreToolUse no stdout" test -z "$out"

real="$root/target/debug/proob"
if [ -x "$real" ]; then
  echo "7. the real binary (target/debug/proob): no proob/ is silent; a status key is denied"
  out=$(printf '%s' "$session_payload" | PROOB_BIN="$real" "$start" 2>"$tmp/err7"); rc=$?
  check "SessionStart exit 0" test "$rc" -eq 0
  check "SessionStart silent without proob/" test -z "$out"
  mkdir -p "$tmp/proob/properties"
  out=$(printf '%s' "$pre_payload" | PROOB_BIN="$real" "$pre" 2>"$tmp/err7b"); rc=$?
  check "PreToolUse exit 0" test "$rc" -eq 0
  check "PreToolUse denies" grep -q '"permissionDecision":"deny"' <<<"$out"
  check "reason names status" grep -q 'status' <<<"$out"
  check "valid JSON" valid_json "$out"
else
  echo "7. (skipped: no target/debug/proob; build with cargo build -p proob to run this step)"
fi

echo "hook tests: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
