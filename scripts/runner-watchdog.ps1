param(
  [Parameter(Mandatory = $true)]
  [int]$ParentPid,

  [Parameter(Mandatory = $true)]
  [string]$Workspace,

  [Parameter(Mandatory = $true)]
  [string]$LogPath
)

$ErrorActionPreference = "SilentlyContinue"

function Write-WatchdogLine {
  param([string]$Line)
  Add-Content -LiteralPath $LogPath -Value ("[watchdog] " + $Line)
}

function Test-ReelspaceProcess {
  param($Process)

  $command = [string]$Process.CommandLine
  $executable = [string]$Process.ExecutablePath
  $workspaceMatch =
    $command.IndexOf($Workspace, [StringComparison]::OrdinalIgnoreCase) -ge 0 -or
    $executable.IndexOf($Workspace, [StringComparison]::OrdinalIgnoreCase) -ge 0

  $knownCommand =
    $command -match 'build-(run|setup)\.bat' -or
    $command -match 'runner(-supervisor|-watchdog)?\.ps1' -or
    $command -match 'vite(\.cmd)?' -or
    $command -match 'tauri(\.cmd)?\s+dev' -or
    $command -match 'cargo(\.exe)?\s+(run|build)' -or
    $command -match 'target\\debug\\reelspace\.exe' -or
    $command -match 'src-tauri\\target\\debug\\reelspace\.exe' -or
    $executable -match 'src-tauri\\target\\debug\\reelspace\.exe'

  return $workspaceMatch -and $knownCommand
}

Write-WatchdogLine "watching supervisor PID $ParentPid"

while (Get-Process -Id $ParentPid -ErrorAction SilentlyContinue) {
  Start-Sleep -Seconds 2
}

Start-Sleep -Seconds 2
Write-WatchdogLine "supervisor exited; purging remaining Reelspace processes"

try {
  Get-CimInstance Win32_Process -ErrorAction Stop |
    Where-Object { $_.ProcessId -ne $PID -and (Test-ReelspaceProcess $_) } |
    ForEach-Object {
      Write-WatchdogLine ("stopping PID {0} {1}" -f $_.ProcessId, $_.Name)
      Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
    }
} catch {
  Write-WatchdogLine ("process purge skipped: {0}" -f $_.Exception.Message)
}

try {
  Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue |
    ForEach-Object {
      $owner = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue
      if ($owner -and $owner.ProcessName -in @("node", "reelspace")) {
        Write-WatchdogLine ("stopping port 1420 listener PID {0} {1}" -f $owner.Id, $owner.ProcessName)
        & taskkill.exe /PID $owner.Id /T /F 2>$null | Out-Null
      }
    }
} catch {
  Write-WatchdogLine ("port cleanup failed: {0}" -f $_.Exception.Message)
}

Write-WatchdogLine "done"
