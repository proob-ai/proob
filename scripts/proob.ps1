# proob shim for Windows: find or fetch the pinned proob binary, verify it, run it.
#
# Resolution order mirrors scripts/proob: $env:PROOB_BIN, then the cache at
# $env:PROOB_HOME (default $env:CLAUDE_PLUGIN_DATA\bin or $HOME\.proob\bin),
# then a verified download of proob-x86_64-pc-windows-msvc.zip and its .sha256.
# Any failure prints one line starting with "proob:" and exits 1.
$PROOB_VERSION = "0.2.0"
$ErrorActionPreference = "Stop"

function Fail([string]$Message) {
  [Console]::Error.WriteLine("proob: $Message")
  exit 1
}

try {
  if ($env:PROOB_BIN) {
    if (-not (Test-Path -LiteralPath $env:PROOB_BIN -PathType Leaf)) { Fail "PROOB_BIN=$($env:PROOB_BIN) is not a file" }
    & $env:PROOB_BIN @args
    exit $LASTEXITCODE
  }

  $dataRoot = if ($env:CLAUDE_PLUGIN_DATA) { $env:CLAUDE_PLUGIN_DATA } else { Join-Path $HOME ".proob" }
  $cacheDir = if ($env:PROOB_HOME) { $env:PROOB_HOME } else { Join-Path $dataRoot "bin" }
  $bin = Join-Path $cacheDir "proob-$PROOB_VERSION.exe"

  if (-not (Test-Path -LiteralPath $bin -PathType Leaf)) {
    if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") { Fail "unsupported Windows architecture $($env:PROCESSOR_ARCHITECTURE); only x86_64 is released; set PROOB_BIN to a local build" }
    $archive = "proob-x86_64-pc-windows-msvc.zip"
    $base = if ($env:PROOB_RELEASE_BASE_URL) { $env:PROOB_RELEASE_BASE_URL } else { "https://github.com/proob-ai/proob/releases/download" }
    $url = "$base/v$PROOB_VERSION/$archive"
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("proob-" + [System.Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
      $archivePath = Join-Path $tmp $archive
      try { Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing }
      catch { Fail "cannot download $url (offline, or no release v$PROOB_VERSION yet); set PROOB_BIN to a local build" }
      try { Invoke-WebRequest -Uri "$url.sha256" -OutFile "$archivePath.sha256" -UseBasicParsing }
      catch { Fail "cannot download $url.sha256; refusing to run an unverified download" }

      $firstLine = Get-Content -LiteralPath "$archivePath.sha256" -TotalCount 1
      $expected = (($firstLine -split '\s+')[0]).ToLowerInvariant()
      $actual = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
      if (-not $expected -or $expected -ne $actual) { Fail "checksum mismatch for $archive (expected $expected, got $actual); refusing to run it" }

      $extract = Join-Path $tmp "x"
      Expand-Archive -LiteralPath $archivePath -DestinationPath $extract -Force
      $found = Get-ChildItem -LiteralPath $extract -Recurse -Filter "proob.exe" | Select-Object -First 1
      if (-not $found) { Fail "$archive does not contain proob.exe" }
      New-Item -ItemType Directory -Path $cacheDir -Force | Out-Null
      Move-Item -LiteralPath $found.FullName -Destination $bin -Force
    } finally {
      Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
  }

  & $bin @args
  exit $LASTEXITCODE
} catch {
  Fail $_.Exception.Message
}
