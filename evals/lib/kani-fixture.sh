#!/usr/bin/env bash
# Scaffold tests/fixtures/kani-crate, set up for Kani as `proob init --kani`
# sets up a repository, into an eval case's workspace.
#
# Sourced by each Kani case's scaffold.sh, which then adds its own property
# and code and commits. On top of rust-fixture.sh (toolchains, the proob
# binary, a git repository), it leaves behind:
#   - the fixture without its harnesses, tokens, properties, or config: `clamp`
#     in src/lib.rs and an unbound proptest of it in tests/props.rs;
#   - the operator's ~/.kani linked into the eval home, where the adapter finds
#     Kani's bundle (`cargo-kani` itself comes from PATH);
#   - what `proob init --kani` writes: proob/config.toml with the rust and kani
#     adapters, both workflows, .claude/settings.json, .gitignore, and the build
#     probe's line in proob/.cache/kani-probe.toml.
# These cases need the Kani release pinned in tools/kani.toml installed
# (`cargo install --locked kani-verifier --version <pin> && cargo kani setup`);
# the scaffold fails, naming the probe's reason, when the probe did not pass.
set -eu

fixture_name=kani-crate
. "$(dirname -- "${BASH_SOURCE[0]}")/rust-fixture.sh"

if [ "$HOME" != "$real_home" ] && [ -e "$real_home/.kani" ] && [ ! -e "$HOME/.kani" ]; then
  ln -s "$real_home/.kani" "$HOME/.kani"
fi

rm -rf target proob
sed -i.bak '/^#\[cfg(kani)\]$/,$d' src/lib.rs
sed -i.bak '/\/\/ proob:0/d' tests/props.rs
rm -f src/*.bak tests/*.bak

"$bin_dir/proob-$version" init --kani
grep -qx 'kani_probe = "passed"' proob/.cache/kani-probe.toml || {
  echo "scaffold: these cases need Kani; the build probe said: $(cat proob/.cache/kani-probe.toml)" >&2
  exit 1
}
mkdir -p proob/properties
