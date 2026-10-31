#!/usr/bin/env bash
# The Kani fixture with nothing declared: a correct, pure `clamp`, a kani
# adapter, and a passed build probe. Declare must propose the requirement's
# properties as proofs, and falsify must bind each to a Kani harness whose
# bounds trace to the statement.
set -eu
. "$(dirname -- "$0")/../lib/kani-fixture.sh"
git -c user.name="proob eval" -c user.email=eval@proob.invalid add -A
git -c user.name="proob eval" -c user.email=eval@proob.invalid commit -q --amend -m "Kani fixture, set up with proob init --kani"
