[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$archiveName = "ffmpeg-N-125628-ga5e6c0175a-win64-gpl.zip"
$archiveUrl = "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-07-15-14-01/$archiveName"
$archiveSha256 = "1c7a6a3d7126e85c6148c42e83bdfb6f964c4a04453075323828a52b331ca149"
$toolHashes = @{
  "ffmpeg.exe" = "87d2e2adfd15e8f134f5afcd4c5f0b1982e41bc9b28b9b790aa00532506aa855"
  "ffprobe.exe" = "353945bafa10aec047ced391f4a9fecc5934c2c15a9471fdcd98f5f2caa30bdf"
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
