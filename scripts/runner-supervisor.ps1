param(
  [Parameter(Mandatory = $true)]
  [string]$BatchPath,

  [Parameter(Mandatory = $true)]
  [string]$Workspace,

  [Parameter(Mandatory = $true)]
  [string]$LogPath,

  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$RunnerArgs
)

$ErrorActionPreference = "Stop"

function Write-RunnerLine {
  param([string]$Line)
  Write-Host $Line
  Add-Content -LiteralPath $LogPath -Value $Line
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
    Write-RunnerLine ("Warning: process ancestor lookup is unavailable: {0}" -f $_.Exception.Message)
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
    $command -match 'runner-supervisor\.ps1' -or
    $command -match 'pnpm(\.cmd)?\s+dev' -or
    $command -match 'vite(\.cmd)?\s+--host\s+127\.0\.0\.1' -or
    $command -match 'tauri(\.cmd)?\s+"?dev"?' -or
    $command -match 'cargo(\.exe)?\s+run' -or
    $command -match 'target\\debug\\reelspace\.exe' -or
    $command -match 'src-tauri\\target\\debug\\reelspace\.exe' -or
    $executable -match 'src-tauri\\target\\debug\\reelspace\.exe'

  return $workspaceMatch -and $knownCommand
}

function Stop-StaleReelspaceProcesses {
  param([string]$Reason)

  $ancestorIds = Get-AncestorProcessIds
  try {
    $candidates = Get-CimInstance Win32_Process -ErrorAction Stop |
      Where-Object {
        $_.ProcessId -ne $PID -and
        -not $ancestorIds.Contains([int]$_.ProcessId) -and
        (Test-ReelspaceProcess $_)
      }
  } catch {
    Write-RunnerLine ("Warning: stale process purge skipped ({0}): {1}" -f $Reason, $_.Exception.Message)
    return
  }

  foreach ($candidate in $candidates) {
    Write-RunnerLine ("Stopping stale process ({0}): PID {1} {2}" -f $Reason, $candidate.ProcessId, $candidate.Name)
    try {
      Stop-Process -Id $candidate.ProcessId -Force -ErrorAction Stop
    } catch {
      Write-RunnerLine ("Warning: failed to stop PID {0}: {1}" -f $candidate.ProcessId, $_.Exception.Message)
    }
  }
}

function Quote-CmdArgument {
  param([string]$Value)
  if ($null -eq $Value) {
    return '""'
  }
  return '"' + ($Value -replace '"', '""') + '"'
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

Write-RunnerLine "Supervisor: purging stale Reelspace runner/dev/app processes before startup."
Stop-StaleReelspaceProcesses -Reason "startup"

$watchdogPath = Join-Path $PSScriptRoot "runner-watchdog.ps1"
if (Test-Path -LiteralPath $watchdogPath) {
  Start-Process -FilePath "powershell.exe" `
    -ArgumentList @(
      "-NoProfile",
      "-ExecutionPolicy", "Bypass",
      "-File", $watchdogPath,
      "-ParentPid", $PID,
      "-Workspace", $Workspace,
      "-LogPath", $LogPath
    ) `
    -WindowStyle Hidden | Out-Null
  Write-RunnerLine "Supervisor: watchdog started for close/crash cleanup."
}

$job = [Reelspace.JobApi]::CreateJobObject([IntPtr]::Zero, $null)
if ($job -eq [IntPtr]::Zero) {
  throw "CreateJobObject failed."
}

$info = New-Object Reelspace.JOBOBJECT_EXTENDED_LIMIT_INFORMATION
$info.BasicLimitInformation.LimitFlags = [Reelspace.JobApi]::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
$infoSize = [Runtime.InteropServices.Marshal]::SizeOf([type][Reelspace.JOBOBJECT_EXTENDED_LIMIT_INFORMATION])
$infoPtr = [Runtime.InteropServices.Marshal]::AllocHGlobal($infoSize)

$process = $null
$childExitCode = 1
try {
  [Runtime.InteropServices.Marshal]::StructureToPtr($info, $infoPtr, $false)
  if (-not [Reelspace.JobApi]::SetInformationJobObject($job, [Reelspace.JobApi]::JobObjectExtendedLimitInformation, $infoPtr, [uint32]$infoSize)) {
    throw "SetInformationJobObject failed."
  }

  $extraArgs = @()
  if ($RunnerArgs) {
    $extraArgs = $RunnerArgs | ForEach-Object { Quote-CmdArgument $_ }
  }
  $quotedArgs = @((Quote-CmdArgument $BatchPath)) + $extraArgs
  $innerCommand = $quotedArgs -join ' '

  $startInfo = New-Object System.Diagnostics.ProcessStartInfo
  $startInfo.FileName = $env:ComSpec
  $startInfo.Arguments = "/d /c `"$innerCommand`""
  $startInfo.WorkingDirectory = $Workspace
  $startInfo.UseShellExecute = $false
  $startInfo.RedirectStandardOutput = $true
  $startInfo.RedirectStandardError = $true
  $startInfo.CreateNoWindow = $true
  [Environment]::SetEnvironmentVariable("REELSPACE_LOGGING", "1", "Process")
  [Environment]::SetEnvironmentVariable("REELSPACE_LOG", $LogPath, "Process")

  $process = New-Object System.Diagnostics.Process
  $process.StartInfo = $startInfo
  $process.EnableRaisingEvents = $true

  $outputAction = {
    if ($EventArgs.Data) {
      Write-Host $EventArgs.Data
      Add-Content -LiteralPath $Event.MessageData -Value $EventArgs.Data
    }
  }

  $outSub = Register-ObjectEvent -InputObject $process -EventName OutputDataReceived -Action $outputAction -MessageData $LogPath
  $errSub = Register-ObjectEvent -InputObject $process -EventName ErrorDataReceived -Action $outputAction -MessageData $LogPath

  [void]$process.Start()
  if (-not [Reelspace.JobApi]::AssignProcessToJobObject($job, $process.Handle)) {
    Write-RunnerLine "Warning: AssignProcessToJobObject failed. Fallback cleanup will still run on exit."
  } else {
    Write-RunnerLine ("Supervisor: process tree assigned to cleanup job. Root PID {0}." -f $process.Id)
  }

  $process.BeginOutputReadLine()
  $process.BeginErrorReadLine()
  $process.WaitForExit()
  $process.WaitForExit()
  $childExitCode = $process.ExitCode

  Unregister-Event -SubscriptionId $outSub.Id -ErrorAction SilentlyContinue
  Unregister-Event -SubscriptionId $errSub.Id -ErrorAction SilentlyContinue
  Remove-Job -Id $outSub.Id,$errSub.Id -Force -ErrorAction SilentlyContinue
} finally {
  if ($process -and -not $process.HasExited) {
    try {
      Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    } catch {}
  }
  Stop-StaleReelspaceProcesses -Reason "shutdown"
  if ($infoPtr -ne [IntPtr]::Zero) {
    [Runtime.InteropServices.Marshal]::FreeHGlobal($infoPtr)
  }
  if ($job -ne [IntPtr]::Zero) {
    [void][Reelspace.JobApi]::CloseHandle($job)
  }
}

exit $childExitCode
