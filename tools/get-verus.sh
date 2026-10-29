#!/usr/bin/env bash
# Fetch the pinned Verus release for this host into a cache directory, verify it
# against the pinned sha256, and print the directory to put on PATH. Idempotent:
# a previously verified unpack is reused. Used by CI and by developers:
#
#     export PATH="$(tools/get-verus.sh):$PATH"
#
# Pins. These move together with rust-toolchain.toml and the vstd version in
# Cargo.toml, in one reviewed PR, never alone.
set -euo pipefail

readonly VERUS_VERSION="0.2026.10.26.d17fbd0"
readonly VERUS_TAG="release/${VERUS_VERSION}"
readonly VERUS_COMMIT="d17fbd0c58abf0d5e86825ea64b96aba72b1b31f"
readonly Z3_VERSION="4.16.0"   # shipped inside each release zip as ./z3

# sha256 of every asset of the pinned tag, from the GitHub release API digest field
# (gh api repos/verus-lang/verus/releases/tags/release%2F${VERUS_VERSION}).
sha256_for() {
  case "$1" in
    arm64-linux) echo 55d0179000c2c78732825d47a8b8fed1aa4afbe6d24896acc6e060eddcd338e4 ;;
    arm64-macos) echo 5e62be446a5da782d64caeee41bb1490c8aa3c65c0afe380d1c1dcbe207571ff ;;
    arm64-win)   echo e6f9b18e142a6d1e9489e3c3b118ab66156bf8b55124283b833893770cb0966b ;;
    x86-linux)   echo 361674d05f0c1b2c9c3f4d5bc56a8fb2ab9241ed0cceca27780668c292bb4940 ;;
    x86-macos)   echo 24a23b0267f7a225c6ece4bc019930ca273bd57207fe0c1dd3cc7dc5726a37bc ;;
    x86-win)     echo 9bf26f8a114f5012485770092e26c91ed04ecd1541d0d45a578a0c36d5cb387a ;;
    *) return 1 ;;
  esac
}

die() { echo "get-verus: $*" >&2; exit 1; }

platform() {
  local os arch
  os=$(uname -s); arch=$(uname -m)
  case "$os/$arch" in
    Darwin/arm64)               echo arm64-macos ;;
    Darwin/x86_64)              echo x86-macos ;;
    Linux/x86_64)               echo x86-linux ;;
    Linux/aarch64|Linux/arm64)  echo arm64-linux ;;
    *) die "unsupported platform $os/$arch (macOS and Linux, x86_64 and arm64)" ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

tmp=""
trap '[ -n "$tmp" ] && rm -rf "$tmp"' EXIT

main() {
  local plat dest dir asset url zip expected actual unpacked
  plat=$(platform)
  dest="${PROOB_VERUS_DIR:-$HOME/.proob-dev/verus/$VERUS_VERSION}"
  dir="$dest/verus-$plat"
  if [ -x "$dir/verus" ] && [ -f "$dir/.proob-verified-$VERUS_VERSION" ]; then
    echo "$dir"
    return 0
  fi

  asset="verus-$VERUS_VERSION-$plat.zip"
  url="https://github.com/verus-lang/verus/releases/download/$VERUS_TAG/$asset"
  expected=$(sha256_for "$plat")
  tmp=$(mktemp -d)
  zip="$tmp/$asset"

  echo "get-verus: downloading $url" >&2
  curl -fsSL --retry 3 --retry-delay 2 -o "$zip" "$url" || die "download failed: $url"
  actual=$(sha256_of "$zip")
  [ "$actual" = "$expected" ] || die "sha256 mismatch for $asset: expected $expected, got $actual"

  mkdir -p "$tmp/unpack"
  unzip -q "$zip" -d "$tmp/unpack" || die "unzip failed for $asset"
  if [ -x "$tmp/unpack/verus-$plat/verus" ]; then unpacked="$tmp/unpack/verus-$plat"
  elif [ -x "$tmp/unpack/verus" ]; then unpacked="$tmp/unpack"
  else die "$asset does not contain a verus binary where expected"; fi

  mkdir -p "$dest"
  rm -rf "$dir"
  mv "$unpacked" "$dir"
  if [ -f "$dir/macos_allow_gatekeeper.sh" ]; then
    (cd "$dir" && sh ./macos_allow_gatekeeper.sh) >&2 || die "macos_allow_gatekeeper.sh failed"
  fi
  "$dir/verus" --version >&2 || die "the unpacked verus binary does not run"
  echo "verus=$VERUS_VERSION commit=$VERUS_COMMIT z3=$Z3_VERSION sha256=$expected" > "$dir/.proob-verified-$VERUS_VERSION"
  echo "$dir"
}

main "$@"
