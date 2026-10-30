# Sourced inside the VHS session (tools/demo/try.tape): a stock Claude Code with only this
# checkout's plugin, no user settings, MCP servers, or parent-session variables.
root=$PWD
demo=${PROOB_DEMO_DIR:-$HOME/proob-demo}
unset $(env | grep -o '^CLAUDE[A-Z_]*') CLAUDECODE
export PATH="$demo/venv/bin:$PATH" PROOB_BIN="$root/target/debug/proob" PS1='$ '
alias claude="claude --plugin-dir $root --settings $root/tools/demo/settings.json --setting-sources project,local --strict-mcp-config --model claude-opus-5-5 --disallowedTools WebFetch WebSearch"
cd "$demo/python-semver" || return
