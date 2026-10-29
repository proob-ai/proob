# proob release fetch, shared by scripts/proob and action.yml. POSIX sh.
#
#   proob_fetch VERSION DEST_DIR   prints the path of a verified, executable
#                                  proob binary cached under DEST_DIR
#
# Every failure prints exactly one line starting with "proob:" to stderr and
# returns 1. Nothing is executed before its checksum has been verified.
#
# Environment:
#   PROOB_RELEASE_BASE_URL   override https://github.com/proob-ai/proob/releases/download
#                            (tests use file:// fixtures; attestation is then skipped)
#   PROOB_SKIP_ATTESTATION   set to skip `gh attestation verify` even when gh is usable
#   PROOB_VERBOSE            set to print what was and was not verified

proob_fail() {
  printf 'proob: %s\n' "$1" >&2
  return 1
}

proob_note() {
  [ -n "${PROOB_VERBOSE:-}" ] && printf 'proob: note: %s\n' "$1" >&2
  return 0
}

# The dist target triple for this host, or failure.
proob_target() {
  _os=$(uname -s 2>/dev/null || echo unknown)
  _arch=$(uname -m 2>/dev/null || echo unknown)
  case "$_os/$_arch" in
    Darwin/arm64)                        echo aarch64-apple-darwin ;;
    Darwin/x86_64)                       echo x86_64-apple-darwin ;;
    Linux/x86_64)                        echo x86_64-unknown-linux-musl ;;
    Linux/aarch64|Linux/arm64)           echo aarch64-unknown-linux-musl ;;
    MINGW*/x86_64|MSYS*/x86_64|CYGWIN*/x86_64) echo x86_64-pc-windows-msvc ;;
    *) return 1 ;;
  esac
}

proob_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
  elif command -v openssl >/dev/null 2>&1; then openssl dgst -sha256 "$1" | sed 's/.* //'
  else return 1
  fi
}

# Verifies the GitHub artifact attestation when that is possible; never fails
# open silently on a real release, never fails closed when gh is unusable.
proob_attest() { # $1: archive path, $2: archive name
  if [ -n "${PROOB_RELEASE_BASE_URL:-}" ] || [ -n "${PROOB_SKIP_ATTESTATION:-}" ]; then
    proob_note "attestation skipped (custom release URL or PROOB_SKIP_ATTESTATION)"; return 0
  fi
  if ! command -v gh >/dev/null 2>&1 || ! gh auth status >/dev/null 2>&1; then
    proob_note "attestation not verified (gh unavailable or unauthenticated); checksum verified"; return 0
  fi
  if gh attestation verify "$1" --repo proob-ai/proob >/dev/null 2>&1; then
    proob_note "attestation verified for $2"; return 0
  fi
  proob_fail "attestation verification failed for $2; refusing to run it"
}

proob_fetch() {
  _version=$1
  _dest=$2
  _target=$(proob_target) || { proob_fail "unsupported platform $(uname -s 2>/dev/null) $(uname -m 2>/dev/null); build with 'cargo build -p proob' and set PROOB_BIN"; return 1; }
  case "$_target" in
    *windows*) _archive="proob-$_target.zip";    _bin=proob.exe ;;
    *)         _archive="proob-$_target.tar.xz"; _bin=proob ;;
  esac
  _base="${PROOB_RELEASE_BASE_URL:-https://github.com/proob-ai/proob/releases/download}"
  _url="$_base/v$_version/$_archive"
  command -v curl >/dev/null 2>&1 || { proob_fail "curl is required to download proob $_version; or set PROOB_BIN to a local build"; return 1; }
  _tmp=$(mktemp -d 2>/dev/null) || { proob_fail "cannot create a temporary directory"; return 1; }

  if ! curl -fsSL --retry 2 --connect-timeout 10 -o "$_tmp/$_archive" "$_url" 2>/dev/null; then
    rm -rf "$_tmp"; proob_fail "cannot download $_url (offline, or no release v$_version yet); set PROOB_BIN to a local build"; return 1
  fi
  if ! curl -fsSL --retry 2 --connect-timeout 10 -o "$_tmp/$_archive.sha256" "$_url.sha256" 2>/dev/null; then
    rm -rf "$_tmp"; proob_fail "cannot download $_url.sha256; refusing to run an unverified download"; return 1
  fi
  _expected=$(awk 'NR == 1 { print tolower($1) }' "$_tmp/$_archive.sha256")
  _actual=$(proob_sha256 "$_tmp/$_archive") || { rm -rf "$_tmp"; proob_fail "no sha256sum, shasum, or openssl available to verify the download"; return 1; }
  if [ -z "$_expected" ] || [ "$_expected" != "$_actual" ]; then
    rm -rf "$_tmp"; proob_fail "checksum mismatch for $_archive (expected ${_expected:-nothing}, got $_actual); refusing to run it"; return 1
  fi
  proob_attest "$_tmp/$_archive" "$_archive" || { rm -rf "$_tmp"; return 1; }

  mkdir -p "$_tmp/x"
  case "$_archive" in
    *.zip)    { unzip -q "$_tmp/$_archive" -d "$_tmp/x" || tar -xf "$_tmp/$_archive" -C "$_tmp/x"; } 2>/dev/null ;;
    *.tar.xz) tar -xJf "$_tmp/$_archive" -C "$_tmp/x" 2>/dev/null ;;
  esac || { rm -rf "$_tmp"; proob_fail "cannot extract $_archive (tar with xz support is required)"; return 1; }
  _found=$(find "$_tmp/x" -type f -name "$_bin" 2>/dev/null | head -n 1)
  [ -n "$_found" ] || { rm -rf "$_tmp"; proob_fail "$_archive does not contain $_bin"; return 1; }

  mkdir -p "$_dest" 2>/dev/null || { rm -rf "$_tmp"; proob_fail "cannot create $_dest"; return 1; }
  _final="$_dest/proob-$_version"
  [ "$_bin" = proob.exe ] && _final="$_final.exe"
  chmod +x "$_found" 2>/dev/null
  if ! cp "$_found" "$_final.part.$$" 2>/dev/null || ! mv -f "$_final.part.$$" "$_final" 2>/dev/null; then
    rm -f "$_final.part.$$"; rm -rf "$_tmp"; proob_fail "cannot install into $_dest"; return 1
  fi
  rm -rf "$_tmp"
  echo "$_final"
}
