[CmdletBinding()]
param(
  [string]$ExpectedVersion,
  [string]$Tag
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

function Read-JsonVersion([string]$Path) {
  $value = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
  if (-not ($value.version -is [string]) -or [string]::IsNullOrWhiteSpace($value.version)) {
    throw "Missing version in $Path"
  }
  return $value.version
}

$packageVersion = Read-JsonVersion (Join-Path $repoRoot "package.json")
if ([string]::IsNullOrWhiteSpace($ExpectedVersion)) {
  $ExpectedVersion = $packageVersion
}

if ($ExpectedVersion -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$') {
  throw "Expected version is not valid semantic versioning: $ExpectedVersion"
}

$cargoTomlPath = Join-Path $repoRoot "src-tauri\Cargo.toml"
$cargoToml = Get-Content -LiteralPath $cargoTomlPath -Raw
$cargoMatch = [regex]::Match($cargoToml, '(?ms)^\[package\]\s*.*?^version\s*=\s*"([^"]+)"')
if (-not $cargoMatch.Success) {
  throw "Could not find the package version in $cargoTomlPath"
}

$cargoLockPath = Join-Path $repoRoot "src-tauri\Cargo.lock"
$cargoLock = Get-Content -LiteralPath $cargoLockPath -Raw
$lockMatch = [regex]::Match($cargoLock, '(?ms)^\[\[package\]\]\s*\r?\nname\s*=\s*"reelspace"\s*\r?\nversion\s*=\s*"([^"]+)"')
if (-not $lockMatch.Success) {
  throw "Could not find the Reelspace package version in $cargoLockPath"
}

$versions = [ordered]@{
  "package.json" = $packageVersion
  "src-tauri/Cargo.toml" = $cargoMatch.Groups[1].Value
  "src-tauri/Cargo.lock" = $lockMatch.Groups[1].Value
  "src-tauri/tauri.conf.json" = Read-JsonVersion (Join-Path $repoRoot "src-tauri\tauri.conf.json")
}

foreach ($entry in $versions.GetEnumerator()) {
  if ($entry.Value -cne $ExpectedVersion) {
    throw "$($entry.Key) has version '$($entry.Value)'; expected '$ExpectedVersion'"
  }
  Write-Host "$($entry.Key): $($entry.Value)"
}

if ([string]::IsNullOrWhiteSpace($Tag) -and $env:GITHUB_REF_TYPE -eq "tag") {
  $Tag = $env:GITHUB_REF_NAME
}
if (-not [string]::IsNullOrWhiteSpace($Tag) -and $Tag -cne "v$ExpectedVersion") {
  throw "Tag '$Tag' does not match version 'v$ExpectedVersion'"
}

Write-Host "Version consistency verified: $ExpectedVersion" -ForegroundColor Green
