#!/bin/sh
# Eval scaffold: tests/fixtures/py-repo as a repository that has never seen
# proob (no proob/ directory, no binding tokens), committed, with a development
# build of the proob binary at ./.proob-bin/proob (gitignored), taken from
# evals/.bin/proob, else target/debug/proob, else $PROOB_BIN, else proob on
# PATH. The Python adapter needs python3 with tools/python-tools.txt on PATH.
# After the scaffold `git status --porcelain` is empty, and the graders hold
# the run to that: shadow mode must not change this repository.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
plugin="${CLAUDE_PLUGIN_ROOT:-$(CDPATH= cd -- "$here/../.." && pwd)}"
fixture="$plugin/tests/fixtures/py-repo"
[ -d "$fixture" ] || { echo "scaffold: no fixture at $fixture" >&2; exit 1; }
bin=""
for cand in "$plugin/evals/.bin/proob" "$plugin/target/debug/proob" "${PROOB_BIN:-}" "$(command -v proob 2>/dev/null || true)"; do
  [ -n "$cand" ] && [ -x "$cand" ] && { bin=$cand; break; }
done
[ -n "$bin" ] || { echo "scaffold: no proob binary; build it (cargo build --locked -p proob) and copy it to evals/.bin/proob" >&2; exit 1; }
cp -R "$fixture/." .
rm -rf proob
sed -i.bak '/^# proob:/d' tests/*.py && rm -f tests/*.bak
mkdir -p .proob-bin && cp "$bin" .proob-bin/proob
printf '/.proob-bin/\n' >> .gitignore
[ -d .git ] || git init -q -b main
git -c user.name=eval -c user.email=eval@example.com add -A
git -c user.name=eval -c user.email=eval@example.com commit -q -m "fixture"
