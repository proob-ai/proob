#!/usr/bin/env bash
# The Kani fixture plus one kind = "proof" property whose code reaches inline
# assembly, which Kani 0.68 does not support: falsify's Kani run errors with
# "Kani cannot verify a construct this code reaches", so it must fall back to
# a proptest, change kind to "test", and show the reviewer why.
set -eu
. "$(dirname -- "$0")/../lib/kani-fixture.sh"
cat > src/word.rs <<'RS'
//! Bit counts on a word read back through a register, as the hot path does.

/// The number of set bits in `x`, after a register-to-register move.
pub fn ones(x: u64) -> u32 {
    let y: u64;
    // SAFETY: a move between registers; no memory is read or written.
    // `mov` reads the same on x86_64 and aarch64.
    unsafe { core::arch::asm!("mov {0}, {1}", out(reg) y, in(reg) x) };
    y.count_ones()
}
RS
printf '\npub mod word;\n' >> src/lib.rs
cat > proob/properties/01K8V5NQ4A0F3R7Y2M9D6P1XKW.toml <<'TOML'
id = "01K8V5NQ4A0F3R7Y2M9D6P1XKW"
title = "A word and its complement share the bits"
statement = "The set bits of a 64-bit word and of its complement always add up to 64."
scope = ["src/word.rs"]
severity = "medium"
rationale = "The allocator's free-slot count is the complement's bit count; src/word.rs is its hot path."
kind = "proof"
TOML
git -c user.name="proob eval" -c user.email=eval@proob.invalid add -A
git -c user.name="proob eval" -c user.email=eval@proob.invalid commit -q --amend -m "Kani fixture, set up with proob init --kani, and a proof-kind property"
