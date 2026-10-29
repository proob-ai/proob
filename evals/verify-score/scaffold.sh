#!/usr/bin/env bash
# The Rust fixture as checked in: 01K8V5NQ4A0F3R7Y2M9D6P1XHD is bound to a
# test that never reads src/parity.rs, so every mutant of it survives.
set -eu
. "$(dirname -- "$0")/../lib/rust-fixture.sh"
