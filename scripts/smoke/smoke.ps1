[CmdletBinding()]
param(
  [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path,
  [int]$StartupTimeoutMinutes = 8
)

$ErrorActionPreference = "Stop"
$smokeDir = $PSScriptRoot
$outDir = Join-Path $smokeDir "out"
$logPath = Join-Path $smokeDir "tauri-dev.log"
$ffmpeg = Join-Path $RepoRoot "vendor\ffmpeg\bin\ffmpeg.exe"
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("reelspace-smoke-" + [guid]::NewGuid().ToString("N"))
$sourceDir = Join-Path $tempRoot "source"
$appData = Join-Path $env:APPDATA "com.reelspace.app"
$tauriPid = $null
$exitCode = 1

try {
  # a. temp source folder + 5 s fixture (same lavfi command as scripts/test-release-media.ps1:21)
  if (-not (Test-Path -LiteralPath $ffmpeg -PathType Leaf)) { throw "ffmpeg missing: $ffmpeg (run scripts/prepare-ffmpeg.ps1)" }
  New-Item -ItemType Directory -Path $sourceDir -Force | Out-Null
  $fixture = Join-Path $sourceDir "Smoke Fixture.mp4"
  & $ffmpeg -hide_banner -loglevel error -f lavfi -i "testsrc2=size=1280x720:rate=30" -f lavfi -i "sine=frequency=440:sample_rate=48000" -t 5 -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a aac -shortest $fixture
  if ($LASTEXITCODE -ne 0) { throw "ffmpeg fixture generation failed ($LASTEXITCODE)" }
  Write-Host "fixture: $fixture"

  # Refuse to run if something already answers on the debug port (would attach to the wrong app).
  $stale = $null
  try { $stale = Invoke-RestMethod -Uri "http://127.0.0.1:9333/json" -TimeoutSec 2 } catch { }
  if ($stale) { throw "port 9333 already in use; close the other WebView2 debug session first" }

  # b. env for the child (inherited by Start-Process)
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
  $env:CARGO_BUILD_JOBS = "2"

  # c. start pnpm tauri:dev; cmd does the redirect so stdout and stderr share one log
  if (Test-Path -LiteralPath $logPath) { Remove-Item -LiteralPath $logPath -Force }
  $proc = Start-Process -FilePath "cmd.exe" -ArgumentList @("/d", "/c", "pnpm tauri:dev --config `"$smokeDir\cdp.conf.json`" > `"$logPath`" 2>&1") `
    -WorkingDirectory $RepoRoot -WindowStyle Hidden -PassThru
  $tauriPid = $proc.Id
  Write-Host "tauri dev tree root PID: $tauriPid (log: $logPath)"

  # d. wait for the CDP page on the Vite dev URL
  $deadline = (Get-Date).AddMinutes($StartupTimeoutMinutes)
  $ready = $false
  while ((Get-Date) -lt $deadline) {
    if ($proc.HasExited) { throw "tauri dev exited early (code $($proc.ExitCode)); see $logPath" }
    $freeGb = (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
    if ($freeGb -lt 1.5) { throw ("RAM_ABORT: Terra free RAM {0:N2} GB < 1.5 GB" -f $freeGb) }
    try {
      $pages = Invoke-RestMethod -Uri "http://127.0.0.1:9333/json" -TimeoutSec 3
      $lastCdp = ($pages | ConvertTo-Json -Depth 3 -Compress)
      if (@($pages | Where-Object { $_.url -like "http://127.0.0.1:1420*" }).Count -gt 0) { $ready = $true; break }
    } catch { $lastCdp = "error: $($_.Exception.Message)" }
    Start-Sleep -Seconds 3
  }
  if (-not $ready) {
    # diagnostics: what CDP said, and what the WebView2 processes were started with / listen on
    Write-Host "last /json result: $lastCdp"
    $wv = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'")
    Write-Host "msedgewebview2 processes: $($wv.Count)"
    $wv | Where-Object { $_.CommandLine -notmatch '--type=' } | ForEach-Object { Write-Host "browser $($_.ProcessId): $($_.CommandLine)" }
    $ids = $wv.ProcessId
    Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $ids -contains $_.OwningProcess -or $_.LocalPort -eq 9333 } |
      ForEach-Object { Write-Host "listen $($_.LocalAddress):$($_.LocalPort) pid $($_.OwningProcess)" }
    throw "timed out after $StartupTimeoutMinutes min waiting for a CDP page at http://127.0.0.1:1420"
  }
  Write-Host "app page is up"

  # e. run the driver
  New-Item -ItemType Directory -Path $outDir -Force | Out-Null
  & node (Join-Path $smokeDir "smoke.mjs") $sourceDir $outDir
  $exitCode = $LASTEXITCODE
}
catch {
  Write-Host "ERROR: $($_.Exception.Message)"
  $exitCode = 1
}
finally {
  # f. kill only the tree we started, then drop the temp source
  if ($tauriPid) {
    & taskkill.exe /T /F /PID $tauriPid | Out-Host
  }
  if (Test-Path -LiteralPath $tempRoot) {
    Start-Sleep -Seconds 2   # let the app release file handles
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
  }
  Write-Host "smoke.mjs exit code: $exitCode"
  Write-Host ("RESULT: " + $(if ($exitCode -eq 0) { "PASS" } else { "FAIL" }))
  Write-Host "outputs: $outDir (result.json, console-errors.json, NN-*.png); dev log: $logPath"
  # g. app data is left alone
  Write-Host "app data NOT deleted: $appData"
}
exit $exitCode
