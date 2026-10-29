#!/bin/sh
# proob PreToolUse hook (Write|Edit): fail-fast schema feedback under proob/.
#
# Passes the hook payload on stdin to `proob hook pre-tool-use` through the
# plugin's shim and prints what the binary prints: a deny JSON when the
# would-be content of a property file or proob/config.toml is not its schema,
# nothing otherwise. A hook never downloads (PROOB_NO_FETCH); when the shim
# cannot find the binary this prints nothing, which is no decision. Exits 0
# whatever happens (R12): this hook is feedback, never the honesty boundary.
set -u
root="${CLAUDE_PLUGIN_ROOT:-$(CDPATH= cd -- "$(dirname -- "$0")/../.." 2>/dev/null && pwd)}"
out=$(PROOB_NO_FETCH=1 "$root/scripts/proob" hook pre-tool-use 2>/dev/null) && [ -n "$out" ] && printf '%s\n' "$out"
exit 0
