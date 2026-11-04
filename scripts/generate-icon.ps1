$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$source = Join-Path $root "src-tauri\icons\icon.svg"
$pnpm = Get-Command pnpm -ErrorAction SilentlyContinue
if (-not $pnpm) {
  throw "pnpm is required to generate the Tauri icon family."
}

Push-Location $root
try {
  & $pnpm.Source tauri icon $source
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
  Pop-Location
}

$required = @(
  "src-tauri\icons\32x32.png",
  "src-tauri\icons\128x128.png",
  "src-tauri\icons\128x128@2x.png",
  "src-tauri\icons\icon.png",
  "src-tauri\icons\icon.ico"
)
foreach ($asset in $required) {
  if (-not (Test-Path -LiteralPath (Join-Path $root $asset) -PathType Leaf)) {
    throw "Icon generation did not produce $asset."
  }
}
