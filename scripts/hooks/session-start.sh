#!/bin/sh
# proob SessionStart hook: the one-line status, or the shim's one-line hint.
#
# Passes the hook payload on stdin to `proob hook session-start` through the
# plugin's shim and prints what the binary prints (a JSON object with
# `systemMessage` and `additionalContext`, or nothing without a proob/
# directory). A hook never downloads: PROOB_NO_FETCH makes the shim refuse
# step 3, so when no binary is cached the shim's one-line `proob:` hint
# becomes the session's systemMessage instead. Exits 0 whatever happens: a
# missing proob never blocks normal work (R12).
set -u
root="${CLAUDE_PLUGIN_ROOT:-$(CDPATH= cd -- "$(dirname -- "$0")/../.." 2>/dev/null && pwd)}"
shim="$root/scripts/proob"

# $1 as the body of a JSON string literal: control characters dropped,
# backslashes and double quotes escaped. Nothing else is interpolated.
json_string() {
  printf '%s' "$1" | tr -d '\000-\037' | sed 's/\\/\\\\/g; s/"/\\"/g'
}

err=$(mktemp 2>/dev/null) || exit 0
trap 'rm -f "$err"' EXIT
if out=$(PROOB_NO_FETCH=1 "$shim" hook session-start 2>"$err"); then
  [ -n "$out" ] && printf '%s\n' "$out"
  exit 0
fi
hint=$(sed -n '/^proob: /{p;q;}' "$err")
[ -n "$hint" ] || hint="proob: the plugin's scripts are missing at $root; reinstall with /plugin install proob@proob"
printf '{"systemMessage":"%s"}\n' "$(json_string "$hint")"
exit 0
