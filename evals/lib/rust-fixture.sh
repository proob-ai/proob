#!/usr/bin/env bash
# Scaffold the Rust fixture into an eval case's workspace.
#
# Sourced by each case's scaffold.sh, which `claude plugin eval` runs only
# with --scaffold, as the operator, with cwd = the case's fresh workspace and
# HOME = the eval's own home directory (not the operator's). It leaves behind:
#   - a copy of tests/fixtures/rust-repo with the repository's toolchain pin
#     (outside the repository rustup would fall back to the default toolchain,
#     which cannot read an edition-2024 manifest);
#   - the operator's ~/.rustup and ~/.cargo linked into the eval home, so the
#     pinned toolchain and the cached registry resolve without a network;
#   - the proob binary at $HOME/.proob/bin/proob-<version>, where the plugin's
#     shim looks, taken from evals/.bin/proob (gitignored; where the sense and
#     declare scaffolds and the evals workflow put a build), $PROOB_BIN, the
#     plugin's target/debug build, or the operator's own shim cache, in that
#     order;
#   - a git repository with the fixture committed, since proob reads the tree
#     through git.
# Nothing is written outside the workspace and the eval home.
#
# The eval runner refuses a plugin directory that holds a hard link, and cargo
# hard-links its build artifacts, so do not build in <plugin>/target before an
# eval run: build with CARGO_TARGET_DIR elsewhere (or take a release binary) and
# copy the one file to <plugin>/target/debug/proob, or put it in the shim cache.
set -eu

plugin_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
fixture="$plugin_root/tests/fixtures/rust-repo"
real_home=$(eval echo "~$(id -un)")

cp -R "$fixture/." .
cp "$plugin_root/rust-toolchain.toml" .

if [ "$HOME" != "$real_home" ]; then
  for d in .rustup .cargo; do
    [ -e "$HOME/$d" ] || ln -s "$real_home/$d" "$HOME/$d"
  done
  [ -e "$HOME/.gitconfig" ] || printf '[user]\n\tname = proob eval\n\temail = eval@proob.invalid\n[init]\n\tdefaultBranch = main\n' > "$HOME/.gitconfig"
fi

version=$(sed -n 's/^PROOB_VERSION="\(.*\)"$/\1/p' "$plugin_root/scripts/proob")
bin_dir="$HOME/.proob/bin"
if [ ! -x "$bin_dir/proob-$version" ]; then
  src=""
  for c in "$plugin_root/evals/.bin/proob" "${PROOB_BIN:-}" "$plugin_root/target/debug/proob" "$real_home/.proob/bin/proob-$version"; do
    if [ -n "$c" ] && [ -x "$c" ]; then src=$c; break; fi
  done
  if [ -z "$src" ]; then
    echo "scaffold: no proob binary found; build one with \`cargo build --locked -p proob\` in $plugin_root and copy it to evals/.bin/proob" >&2
    exit 1
  fi
  if [ "$HOME" = "$real_home" ]; then
    echo "scaffold: HOME is the operator's home; refusing to write $bin_dir/proob-$version there. Copy $src to that path yourself." >&2
    exit 1
  fi
  mkdir -p "$bin_dir"
  cp "$src" "$bin_dir/proob-$version"
fi

git init -q -b main
git -c user.name="proob eval" -c user.email=eval@proob.invalid add -A
git -c user.name="proob eval" -c user.email=eval@proob.invalid commit -q -m "Rust fixture"
