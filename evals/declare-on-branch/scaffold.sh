#!/bin/sh
# Eval scaffold: tests/fixtures/py-repo, which has adopted proob (proob/ with
# its config and four properties, tokened tests), committed on main; then a
# branch `feature/refunds` whose one commit adds interval/refunds.py, a module
# no property's scope matches. A development build of the proob binary is at
# ./.proob-bin/proob (gitignored), taken from evals/.bin/proob, else
# target/debug/proob, else $PROOB_BIN, else proob on PATH. The checkout is
# left on the feature branch with a clean tree.
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
mkdir -p .proob-bin && cp "$bin" .proob-bin/proob
printf '/.proob-bin/\nproob/.cache/\n' >> .gitignore
git="git -c user.name=eval -c user.email=eval@example.com"
[ -d .git ] || git init -q -b main
$git add -A
$git commit -q -m "fixture: intervals, with proob adopted"
$git switch -q -c feature/refunds
cat > interval/refunds.py <<'PY'
"""Refunds against a charge: partial refunds accumulate, and the total
refunded never exceeds what was charged."""


class Charge:
    def __init__(self, amount: int) -> None:
        if amount < 0:
            raise ValueError("a charge is never negative")
        self.amount = amount
        self.refunded = 0

    def refund(self, asked: int) -> int:
        """Refund up to `asked`, never beyond what is left; returns what was refunded."""
        if asked < 0:
            raise ValueError("a refund is never negative")
        granted = min(asked, self.amount - self.refunded)
        self.refunded += granted
        return granted
PY
$git add interval/refunds.py
$git commit -q -m "refunds: partial refunds against a charge"
