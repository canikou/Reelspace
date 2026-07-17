param(
  [Parameter(Mandatory = $true)]
  [string]$Workspace,

  [Parameter(Mandatory = $true)]
  [string]$LogPath,

  [Parameter(Mandatory = $true)]
  [string]$RunnerScript,

  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$RunnerArgs
)

$ErrorActionPreference = "Stop"

function Write-SupervisorLine {
  param([string]$Line)

  $message = "[supervisor] $Line"
  Write-Host $message -ForegroundColor DarkGray
  Add-Content -LiteralPath $LogPath -Value $message
}

function Get-AncestorProcessIds {
  $ids = New-Object 'System.Collections.Generic.HashSet[int]'
  try {
    $current = Get-CimInstance Win32_Process -Filter "ProcessId = $PID" -ErrorAction Stop
    while ($current -and $current.ParentProcessId) {
      [void]$ids.Add([int]$current.ParentProcessId)
      $parentId = [int]$current.ParentProcessId
      $current = Get-CimInstance Win32_Process -Filter "ProcessId = $parentId" -ErrorAction SilentlyContinue
    }
  } catch {
    Write-SupervisorLine ("Ancestor lookup unavailable: {0}" -f $_.Exception.Message)
  }
  return $ids
}

function Test-ReelspaceProcess {
  param($Process)

  $command = [string]$Process.CommandLine
  $executable = [string]$Process.ExecutablePath
  $workspaceMatch =
    $command.IndexOf($Workspace, [StringComparison]::OrdinalIgnoreCase) -ge 0 -or
    $executable.IndexOf($Workspace, [StringComparison]::OrdinalIgnoreCase) -ge 0
  $knownCommand =
    $command -match 'build-run\.bat' -or
    $command -match 'runner(-supervisor|-watchdog)?\.ps1' -or
    $command -match 'vite(\.cmd)?' -or
    $command -match 'tauri(\.cmd)?\s+dev' -or
    $command -match 'cargo(\.exe)?\s+(run|build)' -or
    $command -match 'src-tauri\\target\\debug\\reelspace\.exe' -or
    $executable -match 'src-tauri\\target\\debug\\reelspace\.exe'

  return $workspaceMatch -and $knownCommand
}

function Stop-ProcessTree {
  param(
    [int]$ProcessId,
    [string]$Description
  )

  if (-not (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) {
    return
  }

  Write-SupervisorLine ("Stopping {0} (PID {1})." -f $Description, $ProcessId)
  $previousErrorPreference = $ErrorActionPreference
  try {
    $ErrorActionPreference = "Continue"
    & taskkill.exe /PID $ProcessId /T /F 2>$null | Out-Null
    $taskkillExitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousErrorPreference
  }
  if ($taskkillExitCode -ne 0 -and (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) {
    Stop-Process -Id $ProcessId -Force -ErrorAction SilentlyContinue
  }
}

function Stop-StalePortListener {
  try {
    $connections = @(Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction Stop)
  } catch {
    return
  }

  foreach ($connection in $connections) {
    $owner = Get-Process -Id $connection.OwningProcess -ErrorAction SilentlyContinue
    if ($owner -and $owner.ProcessName -in @("node", "reelspace")) {
      Stop-ProcessTree -ProcessId $owner.Id -Description "stale dev-server listener on 127.0.0.1:1420"
    } elseif ($owner) {
      throw "Port 1420 is occupied by $($owner.ProcessName) (PID $($owner.Id)). Close it, then run Reelspace again."
    }
  }
}

function Stop-StaleReelspaceProcesses {
  param([string]$Reason)

  $ancestorIds = Get-AncestorProcessIds
  try {
    $candidates = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
      $_.ProcessId -ne $PID -and
      -not $ancestorIds.Contains([int]$_.ProcessId) -and
      (Test-ReelspaceProcess $_)
    })
  } catch {
    Write-SupervisorLine ("Command-line process cleanup unavailable during {0}: {1}" -f $Reason, $_.Exception.Message)
    $candidates = @()
  }

  foreach ($candidate in $candidates) {
    Stop-ProcessTree -ProcessId $candidate.ProcessId -Description ("stale {0} process {1}" -f $Reason, $candidate.Name)
  }

  if ($Reason -eq "startup") {
    Stop-StalePortListener
  }
}

function Quote-ProcessArgument {
  param([string]$Value)

  if ($null -eq $Value -or $Value.Length -eq 0) {
    return '""'
  }
  return '"' + ($Value -replace '"', '\"') + '"'
}

function Convert-RunnerArguments {
  $mode = "run"
  $noPause = $false

  foreach ($argument in @($RunnerArgs)) {
    switch -Regex ($argument) {
      '^--check$' { $mode = "check"; continue }
      '^--no-launch$' { $mode = "check"; continue }
      '^--build-only$' { $mode = "build"; continue }
      '^--dist$' { $mode = "dist"; continue }
      '^--no-pause$' { $noPause = $true; continue }
      default { throw "Unknown runner option: $argument" }
    }
  }

  $arguments = @(
    "-NoLogo",
    "-NoProfile",
    "-ExecutionPolicy", "Bypass",
    "-File", $RunnerScript,
    "-Mode", $mode,
    "-Workspace", $Workspace,
    "-LogPath", $LogPath
  )
  if ($noPause) {
    Write-SupervisorLine "Non-interactive failure handling enabled."
  }
  return $arguments
}

if (-not ([System.Management.Automation.PSTypeName]'Reelspace.JobApi').Type) {
  Add-Type @"
using System;
using System.Runtime.InteropServices;

namespace Reelspace {
  [StructLayout(LayoutKind.Sequential)]
  public struct IO_COUNTERS {
    public UInt64 ReadOperationCount;
    public UInt64 WriteOperationCount;
    public UInt64 OtherOperationCount;
    public UInt64 ReadTransferCount;
    public UInt64 WriteTransferCount;
    public UInt64 OtherTransferCount;
  }

  [StructLayout(LayoutKind.Sequential)]
  public struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
    public Int64 PerProcessUserTimeLimit;
    public Int64 PerJobUserTimeLimit;
    public UInt32 LimitFlags;
    public UIntPtr MinimumWorkingSetSize;
    public UIntPtr MaximumWorkingSetSize;
    public UInt32 ActiveProcessLimit;
    public Int64 Affinity;
    public UInt32 PriorityClass;
    public UInt32 SchedulingClass;
  }

  [StructLayout(LayoutKind.Sequential)]
  public struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
    public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
    public IO_COUNTERS IoInfo;
    public UIntPtr ProcessMemoryLimit;
    public UIntPtr JobMemoryLimit;
    public UIntPtr PeakProcessMemoryUsed;
    public UIntPtr PeakJobMemoryUsed;
  }

  public static class JobApi {
    public const int JobObjectExtendedLimitInformation = 9;
    public const UInt32 JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr CreateJobObject(IntPtr lpJobAttributes, string lpName);

    [DllImport("kernel32.dll")]
    public static extern bool SetInformationJobObject(IntPtr hJob, int infoType, IntPtr lpJobObjectInfo, uint cbJobObjectInfoLength);

    [DllImport("kernel32.dll")]
    public static extern bool AssignProcessToJobObject(IntPtr hJob, IntPtr hProcess);

    [DllImport("kernel32.dll")]
    public static extern bool CloseHandle(IntPtr hObject);
  }
}
"@
}

$logDir = Split-Path -Parent $LogPath
if (-not (Test-Path -LiteralPath $logDir)) {
  New-Item -ItemType Directory -Force -Path $logDir | Out-Null
}

$job = [IntPtr]::Zero
$infoPtr = [IntPtr]::Zero
$process = $null
$childExitCode = 1

try {
  Write-SupervisorLine "Purging stale Reelspace app, dev-server, and build processes."
  Stop-StaleReelspaceProcesses -Reason "startup"

  $watchdogPath = Join-Path $PSScriptRoot "runner-watchdog.ps1"
  if (Test-Path -LiteralPath $watchdogPath) {
    Start-Process -FilePath "powershell.exe" -ArgumentList @(
      "-NoLogo",
      "-NoProfile",
      "-ExecutionPolicy", "Bypass",
      "-File", $watchdogPath,
      "-ParentPid", $PID,
      "-Workspace", $Workspace,
      "-LogPath", $LogPath
    ) -WindowStyle Hidden | Out-Null
    Write-SupervisorLine "Close/crash watchdog armed."
  }

  $job = [Reelspace.JobApi]::CreateJobObject([IntPtr]::Zero, $null)
  if ($job -eq [IntPtr]::Zero) {
    throw "Windows process cleanup job could not be created."
  }

  $info = New-Object Reelspace.JOBOBJECT_EXTENDED_LIMIT_INFORMATION
  $info.BasicLimitInformation.LimitFlags = [Reelspace.JobApi]::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
  $infoSize = [Runtime.InteropServices.Marshal]::SizeOf([type][Reelspace.JOBOBJECT_EXTENDED_LIMIT_INFORMATION])
  $infoPtr = [Runtime.InteropServices.Marshal]::AllocHGlobal($infoSize)
  [Runtime.InteropServices.Marshal]::StructureToPtr($info, $infoPtr, $false)
  if (-not [Reelspace.JobApi]::SetInformationJobObject($job, [Reelspace.JobApi]::JobObjectExtendedLimitInformation, $infoPtr, [uint32]$infoSize)) {
    throw "Windows process cleanup job could not be configured."
  }

  $childArguments = Convert-RunnerArguments
  $startInfo = New-Object System.Diagnostics.ProcessStartInfo
  $startInfo.FileName = "powershell.exe"
  $startInfo.Arguments = ($childArguments | ForEach-Object { Quote-ProcessArgument $_ }) -join " "
  $startInfo.WorkingDirectory = $Workspace
  $startInfo.UseShellExecute = $false
  $startInfo.CreateNoWindow = $false

  $process = New-Object System.Diagnostics.Process
  $process.StartInfo = $startInfo
  [void]$process.Start()

  if ([Reelspace.JobApi]::AssignProcessToJobObject($job, $process.Handle)) {
    Write-SupervisorLine ("Full process tree protected by cleanup job (root PID {0})." -f $process.Id)
  } else {
    Write-SupervisorLine "Cleanup-job assignment was unavailable; watchdog cleanup remains active."
  }

  $process.WaitForExit()
  $childExitCode = $process.ExitCode
} catch {
  Write-SupervisorLine ("FAILED: {0}" -f $_.Exception.Message)
  $childExitCode = 1
} finally {
  if ($process -and -not $process.HasExited) {
    Stop-ProcessTree -ProcessId $process.Id -Description "active runner process tree"
  }
  if ($infoPtr -ne [IntPtr]::Zero) {
    [Runtime.InteropServices.Marshal]::FreeHGlobal($infoPtr)
  }
  if ($job -ne [IntPtr]::Zero) {
    [void][Reelspace.JobApi]::CloseHandle($job)
  }
  Stop-StaleReelspaceProcesses -Reason "shutdown"
}

exit $childExitCode
