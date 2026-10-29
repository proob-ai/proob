#!/usr/bin/env bash
# The Rust fixture plus one property the human marked kind = "proof" on a
# crate that is not under Verus: verify must propose the proof route for it,
# and only for it, and must not claim a proof it did not run.
set -eu
. "$(dirname -- "$0")/../lib/rust-fixture.sh"
cat > proob/properties/01K8V5NQ4A0F3R7Y2M9D6P1XHP.toml <<'TOML'
id = "01K8V5NQ4A0F3R7Y2M9D6P1XHP"
title = "Sorting permutes its input"
statement = "A sorted list contains exactly the elements of its input, each as many times as the input did."
scope = ["src/ordering.rs"]
severity = "high"
rationale = "The team wants this one proven rather than tested; the maintainer marked it kind = proof before any crate was placed under Verus."
kind = "proof"
TOML
git -c user.name="proob eval" -c user.email=eval@proob.invalid add -A
git -c user.name="proob eval" -c user.email=eval@proob.invalid commit -q -m "declare a proof-kind property"
