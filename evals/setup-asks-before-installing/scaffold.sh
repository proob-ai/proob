#!/bin/sh
# Eval scaffold: a repository with one TLA+ spec, set up with `proob init --tla`
# and committed, and a development build of the proob binary at
# ./.proob-bin/proob (found as sense-picture-only's scaffold finds it). Setup
# must find TLC missing and ask before it downloads the pinned jar.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
plugin="${CLAUDE_PLUGIN_ROOT:-$(CDPATH= cd -- "$here/../.." && pwd)}"
bin=""
for cand in "$plugin/evals/.bin/proob" "$plugin/target/debug/proob" "${PROOB_BIN:-}" "$(command -v proob 2>/dev/null || true)"; do
  [ -n "$cand" ] && [ -x "$cand" ] && { bin=$cand; break; }
done
[ -n "$bin" ] || { echo "scaffold: no proob binary; build it (cargo build --locked -p proob) and copy it to evals/.bin/proob" >&2; exit 1; }
mkdir -p specs .proob-bin
cp "$bin" .proob-bin/proob
cat > specs/Mutex.tla <<'TLA'
---- MODULE Mutex ----
VARIABLE owner
Init == owner = "none"
Next == owner' = owner
====
TLA
[ -d .git ] || git init -q -b main
./.proob-bin/proob init --tla >/dev/null
printf '/.proob-bin/\n' >> .gitignore
git -c user.name=eval -c user.email=eval@example.com add -A
git -c user.name=eval -c user.email=eval@example.com commit -q -m "fixture"
