#!/usr/bin/env bash
# README translations stay in sync with README.md, the English source.
#
# Each translation (README.<BCP 47 tag>.md) carries, on its first line:
#   <!-- translated from README.md sha256:<hex> -->
# naming the SHA-256 of the English README it was translated from. This fails
# when that hash is not the current README.md's, or when the translation's
# level-two heading count differs from the English one. Content hashes, not
# commit ids, so a history rewrite does not invalidate a translation.
set -u
cd "$(dirname "$0")/.." || exit 1
hash() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
want=$(hash README.md)
h2() { grep -c '^## ' "$1"; }
fail=0
for f in README.*.md; do
  [ -e "$f" ] || continue
  got=$(sed -n '1s/^<!-- translated from README\.md sha256:\([0-9a-f]\{64\}\) -->$/\1/p' "$f")
  if [ "$got" != "$want" ]; then
    echo "readme-sync: $f was translated from an older README.md; update it and set its first line to <!-- translated from README.md sha256:$want -->"; fail=1
  elif [ "$(h2 "$f")" != "$(h2 README.md)" ]; then
    echo "readme-sync: $f has $(h2 "$f") sections, README.md has $(h2 README.md)"; fail=1
  else
    echo "readme-sync: ok    $f"
  fi
done
exit $fail
