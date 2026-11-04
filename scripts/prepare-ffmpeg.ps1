[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$archiveName = "ffmpeg-n8.1.3-32-g760d10603e-win64-gpl-8.1.zip"
$archiveUrl = "https://github.com/canikou/Reelspace/releases/download/vendor-ffmpeg-n8.1.3/$archiveName"
$archiveSha256 = "f41877437ec23e4619e5507180a020005738fe09376fdfcd3fdc0f4c549afc7c"
$toolHashes = @{
  "ffmpeg.exe" = "e49f39b5720de3edf15d63a76e4d6e8d04c0c2e641b0ca118360ee4c4bd52e36"
  "ffprobe.exe" = "8a2e6a26236f154d093eebe98251c52adaf047de2d6c73478da5e4df8c48ce7b"
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$vendorRoot = Join-Path $repoRoot "vendor\ffmpeg"
$binRoot = Join-Path $vendorRoot "bin"
$licensePath = Join-Path $vendorRoot "LICENSE.txt"

function Test-FileHash([string]$Path, [string]$ExpectedHash) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    return $false
  }
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ieq $ExpectedHash
}

$toolsReady = $true
foreach ($tool in $toolHashes.GetEnumerator()) {
  if (-not (Test-FileHash (Join-Path $binRoot $tool.Key) $tool.Value)) {
    $toolsReady = $false
  }
}
if ($toolsReady -and (Test-Path -LiteralPath $licensePath -PathType Leaf)) {
  Write-Host "Pinned FFmpeg and FFprobe resources are already verified." -ForegroundColor Green
  return
}

$scratchParent = [System.IO.Path]::GetTempPath()
$scratch = Join-Path $scratchParent ("reelspace-ffmpeg-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $scratch | Out-Null

try {
  $archivePath = Join-Path $scratch $archiveName
  Write-Host "Downloading pinned FFmpeg build..."
  Invoke-WebRequest -Uri $archiveUrl -OutFile $archivePath -UseBasicParsing
  if (-not (Test-FileHash $archivePath $archiveSha256)) {
    throw "The FFmpeg archive hash did not match the pinned SHA-256 value."
  }

  $extractRoot = Join-Path $scratch "extracted"
  Expand-Archive -LiteralPath $archivePath -DestinationPath $extractRoot
  $ffmpegPath = @(Get-ChildItem -LiteralPath $extractRoot -Recurse -File -Filter "ffmpeg.exe")
  $ffprobePath = @(Get-ChildItem -LiteralPath $extractRoot -Recurse -File -Filter "ffprobe.exe")
  $sourceLicense = @(Get-ChildItem -LiteralPath $extractRoot -Recurse -File -Filter "LICENSE.txt")
  if ($ffmpegPath.Count -ne 1 -or $ffprobePath.Count -ne 1 -or $sourceLicense.Count -ne 1) {
    throw "The pinned FFmpeg archive did not contain the expected unique tools and license."
  }
  if (-not (Test-FileHash $ffmpegPath[0].FullName $toolHashes["ffmpeg.exe"])) {
    throw "The extracted ffmpeg.exe hash did not match the pinned SHA-256 value."
  }
  if (-not (Test-FileHash $ffprobePath[0].FullName $toolHashes["ffprobe.exe"])) {
    throw "The extracted ffprobe.exe hash did not match the pinned SHA-256 value."
  }

  New-Item -ItemType Directory -Path $binRoot -Force | Out-Null
  Copy-Item -LiteralPath $ffmpegPath[0].FullName -Destination (Join-Path $binRoot "ffmpeg.exe") -Force
  Copy-Item -LiteralPath $ffprobePath[0].FullName -Destination (Join-Path $binRoot "ffprobe.exe") -Force
  Copy-Item -LiteralPath $sourceLicense[0].FullName -Destination $licensePath -Force
  Write-Host "Pinned FFmpeg resources verified and staged." -ForegroundColor Green
}
finally {
  $resolvedScratch = [System.IO.Path]::GetFullPath($scratch)
  $resolvedParent = [System.IO.Path]::GetFullPath($scratchParent)
  if ($resolvedScratch.StartsWith($resolvedParent, [System.StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolvedScratch) -like "reelspace-ffmpeg-*") {
    Remove-Item -LiteralPath $resolvedScratch -Recurse -Force -ErrorAction SilentlyContinue
  }
}
