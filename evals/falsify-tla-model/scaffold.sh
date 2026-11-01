#!/usr/bin/env bash
# A small Python payments service whose client resends a charge with the same
# idempotency key until a reply arrives, set up with `proob init --tla`, and
# one approved-looking kind = "model" property no check binds yet. Falsify must
# write a TLA+ spec of the retry protocol under proob/specs/, bind the property
# to its invariant, run it through TLC, and report it as a claim about the
# design over the cfg's constants, never about the code.
#
# This case needs TLC: Java 11 or later on PATH and PROOB_TLA2TOOLS naming a
# tla2tools.jar, as the TLA+ adapter does (docs/guide.md, "Models of a design").
# Like the Kani cases' scaffold, it fails naming what is missing rather than
# letting the agent run against a check that can only error.
set -eu

command -v java >/dev/null 2>&1 && java -version >/dev/null 2>&1 || {
  echo "scaffold: this case needs Java 11 or later on PATH for TLC" >&2
  exit 1
}
[ -n "${PROOB_TLA2TOOLS:-}" ] && [ -f "$PROOB_TLA2TOOLS" ] || {
  echo "scaffold: this case needs PROOB_TLA2TOOLS set to a tla2tools.jar (https://github.com/tlaplus/tlaplus/releases)" >&2
  exit 1
}

# The proob binary where the plugin's shim looks, found as lib/rust-fixture.sh finds it.
plugin_root=$(cd -- "$(dirname -- "$0")/../.." && pwd)
real_home=$(eval echo "~$(id -un)")
version=$(sed -n 's/^PROOB_VERSION="\(.*\)"$/\1/p' "$plugin_root/scripts/proob")
bin_dir="$HOME/.proob/bin"
if [ ! -x "$bin_dir/proob-$version" ]; then
  src=""
  for c in "$plugin_root/evals/.bin/proob" "${PROOB_BIN:-}" "$plugin_root/target/debug/proob" "$real_home/.proob/bin/proob-$version"; do
    if [ -n "$c" ] && [ -x "$c" ]; then src=$c; break; fi
  done
  [ -n "$src" ] || { echo "scaffold: no proob binary found; build one with \`cargo build --locked -p proob\` in $plugin_root and copy it to evals/.bin/proob" >&2; exit 1; }
  [ "$HOME" != "$real_home" ] || { echo "scaffold: HOME is the operator's home; refusing to write $bin_dir/proob-$version there. Copy $src to that path yourself." >&2; exit 1; }
  mkdir -p "$bin_dir"
  cp "$src" "$bin_dir/proob-$version"
fi
if [ "$HOME" != "$real_home" ] && [ ! -e "$HOME/.gitconfig" ]; then
  printf '[user]\n\tname = proob eval\n\temail = eval@proob.invalid\n[init]\n\tdefaultBranch = main\n' > "$HOME/.gitconfig"
fi

mkdir -p payments
cat > payments/__init__.py <<'PY'
PY
cat > payments/ledger.py <<'PY'
"""The ledger: every charge it is asked to make, in order."""


class Ledger:
    def __init__(self):
        self.charges = []

    def charge(self, account, amount):
        self.charges.append((account, amount))
        return len(self.charges)
PY
cat > payments/server.py <<'PY'
"""The payment server. It takes requests from one queue and handles them one at
a time, so no two requests are ever handled at once."""


class PaymentServer:
    def __init__(self, ledger):
        self.ledger = ledger
        self.done = {}  # idempotency key -> the receipt of its one charge

    def handle(self, request):
        """Charge once per idempotency key; a resent request gets the first receipt back."""
        key = request["key"]
        if key in self.done:
            return self.done[key]
        receipt = self.ledger.charge(request["account"], request["amount"])
        self.done[key] = receipt
        return receipt
PY
cat > payments/client.py <<'PY'
"""The client: send a charge, and resend it with the same key until a reply
arrives. The transport may lose a request before the server sees it, or lose
the reply after the server has charged."""
import uuid

MAX_ATTEMPTS = 3


class Lost(Exception):
    """The request or its reply was lost in transit."""


def pay(transport, account, amount):
    key = str(uuid.uuid4())
    request = {"key": key, "account": account, "amount": amount}
    for _ in range(MAX_ATTEMPTS):
        try:
            return transport.send(request)
        except Lost:
            continue
    raise Lost(f"no reply after {MAX_ATTEMPTS} attempts for {key}")
PY

git init -q -b main
git -c user.name="proob eval" -c user.email=eval@proob.invalid add -A
git -c user.name="proob eval" -c user.email=eval@proob.invalid commit -q -m "payments service"

"$bin_dir/proob-$version" init --tla >/dev/null
grep -qx 'language = "tla"' proob/config.toml || { echo "scaffold: proob init --tla wrote no tla adapter" >&2; exit 1; }
mkdir -p proob/properties
cat > proob/properties/01K8V5NQ4A0F3R7Y2M9D6P1XTM.toml <<'TOML'
id = "01K8V5NQ4A0F3R7Y2M9D6P1XTM"
title = "A resent payment is charged once"
statement = "A payment is charged at most once, however many times the client resends it."
scope = ["payments/server.py", "payments/client.py"]
severity = "critical"
rationale = "The client resends a charge whenever a reply is lost (payments/client.py); the server's idempotency keys (payments/server.py) are all that stands between a lost reply and a double charge."
kind = "model"
TOML
git -c user.name="proob eval" -c user.email=eval@proob.invalid add -A
git -c user.name="proob eval" -c user.email=eval@proob.invalid commit -q -m "proob init --tla, and a model property"
