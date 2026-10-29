#!/bin/sh
# Eval scaffold: a copy of tests/fixtures/rust-repo as a fresh git repository
# in the case workspace (the current directory), with a development build of
# the proob binary at ./.proob-bin/proob, copied from evals/.bin/proob (gitignored;
# put a build there: cargo build --locked -p proob && cp target/debug/proob evals/.bin/proob),
# else target/debug/proob, else $PROOB_BIN, else proob on PATH. The eval runner refuses
# a plugin tree that holds hard links, and Cargo's incremental build makes them, so
# build with CARGO_TARGET_DIR outside the checkout before running the evals.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
plugin="${CLAUDE_PLUGIN_ROOT:-$(CDPATH= cd -- "$here/../.." && pwd)}"
fixture="$plugin/tests/fixtures/rust-repo"
[ -d "$fixture" ] || { echo "scaffold: no fixture at $fixture" >&2; exit 1; }
bin=""
for cand in "$plugin/evals/.bin/proob" "$plugin/target/debug/proob" "${PROOB_BIN:-}" "$(command -v proob 2>/dev/null || true)"; do
  [ -n "$cand" ] && [ -x "$cand" ] && { bin=$cand; break; }
done
[ -n "$bin" ] || { echo "scaffold: no proob binary; build it (cargo build --locked -p proob) and copy it to evals/.bin/proob" >&2; exit 1; }
cp -R "$fixture/." .
rm -rf target
mkdir -p .proob-bin && cp "$bin" .proob-bin/proob
printf '/.proob-bin/\n' >> .gitignore
[ -d .git ] || git init -q -b main
git -c user.name=eval -c user.email=eval@example.com add -A
git -c user.name=eval -c user.email=eval@example.com commit -q -m "fixture"
