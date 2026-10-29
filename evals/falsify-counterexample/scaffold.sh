#!/usr/bin/env bash
# The Rust fixture as checked in: 01K8V5NQ4A0F3R7Y2M9D6P1XHB is bound to a
# test that fails, because src/bounds.rs ignores the upper bound.
set -eu
. "$(dirname -- "$0")/../lib/rust-fixture.sh"
