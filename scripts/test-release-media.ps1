[CmdletBinding()]
param(
  [string]$SourceRoot,
  [string]$OutputRoot,
  [string]$ArchivePath,
  [switch]$KeepArtifacts
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$generatedRoot = $null
if ([string]::IsNullOrWhiteSpace($SourceRoot)) {
  $generatedRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("reelspace-release-fixture-" + [guid]::NewGuid().ToString("N"))
  $SourceRoot = Join-Path $generatedRoot "source"
  New-Item -ItemType Directory -Path $SourceRoot | Out-Null
  $ffmpeg = Join-Path $repoRoot "vendor\ffmpeg\bin\ffmpeg.exe"
  if (-not (Test-Path -LiteralPath $ffmpeg -PathType Leaf)) {
    throw "Verified FFmpeg resources are required. Run scripts/prepare-ffmpeg.ps1 first."
  }
  $validFixture = Join-Path $SourceRoot "Sanitized Release Fixture.mp4"
  & $ffmpeg -hide_banner -loglevel error -f lavfi -i "testsrc2=size=1280x720:rate=30" -f lavfi -i "sine=frequency=440:sample_rate=48000" -t 5 -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a aac -shortest $validFixture
  if ($LASTEXITCODE -ne 0) {
    throw "FFmpeg could not generate the sanitized release fixture."
  }
  Set-Content -LiteralPath (Join-Path $SourceRoot "Malformed Release Fixture.mp4") -Value "not media" -Encoding ascii
  Start-Sleep -Seconds 4
}
if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
  $OutputRoot = if ($generatedRoot) { Join-Path $generatedRoot "output" } else { Join-Path ([System.IO.Path]::GetTempPath()) ("reelspace-release-output-" + [guid]::NewGuid().ToString("N")) }
}
if ([string]::IsNullOrWhiteSpace($ArchivePath)) {
  $ArchivePath = if ($generatedRoot) { Join-Path $generatedRoot "library-data.json" } else { Join-Path ([System.IO.Path]::GetTempPath()) ("reelspace-release-archive-" + [guid]::NewGuid().ToString("N") + ".json") }
}

$source = (Resolve-Path -LiteralPath $SourceRoot).Path
$output = [System.IO.Path]::GetFullPath($OutputRoot)
$archive = [System.IO.Path]::GetFullPath($ArchivePath)
if (-not [System.IO.Path]::IsPathRooted($source) -or -not [System.IO.Path]::IsPathRooted($output) -or -not [System.IO.Path]::IsPathRooted($archive)) {
  throw "All release smoke paths must be absolute."
}
if (Test-Path -LiteralPath $archive) {
  throw "The archive destination already exists: $archive"
}

New-Item -ItemType Directory -Path $output -Force | Out-Null
Push-Location $repoRoot
try {
  cargo run --release --manifest-path src-tauri/Cargo.toml --example release_media_smoke -- $source $output $archive
  if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
  }
}
finally {
  Pop-Location
  if ($generatedRoot -and -not $KeepArtifacts) {
    $resolvedGenerated = [System.IO.Path]::GetFullPath($generatedRoot)
    $temporaryRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ($resolvedGenerated.StartsWith($temporaryRoot, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolvedGenerated) -like "reelspace-release-fixture-*") {
      Remove-Item -LiteralPath $resolvedGenerated -Recurse -Force -ErrorAction SilentlyContinue
    }
  }
}
