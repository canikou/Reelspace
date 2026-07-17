param(
  [ValidateSet("run", "check", "build", "setup")]
  [string]$Mode = "run",

  [Parameter(Mandatory = $true)]
  [string]$Workspace,

  [Parameter(Mandatory = $true)]
  [string]$LogPath
)

$ErrorActionPreference = "Stop"
$startedAt = Get-Date
$transcribing = $false
$exitCode = 1

function Write-Stage {
  param([string]$Message)

  $elapsed = (Get-Date) - $startedAt
  Write-Host ""
  Write-Host ("[{0:hh\:mm\:ss}] {1}" -f $elapsed, $Message) -ForegroundColor Cyan
  Write-Host ("-" * 60) -ForegroundColor DarkGray
}

function Invoke-RunnerCommand {
  param(
    [Parameter(Mandatory = $true)]
    [string]$FilePath,

    [string[]]$Arguments = @()
  )

  $displayName = Split-Path -Leaf $FilePath
  Write-Host ("> {0} {1}" -f $displayName, ($Arguments -join " ")) -ForegroundColor DarkGray
  $previousErrorPreference = $ErrorActionPreference
  try {
    $ErrorActionPreference = "Continue"
    & $FilePath @Arguments 2>&1 | ForEach-Object {
      Write-Host ([string]$_)
    }
    $commandExitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousErrorPreference
  }
  if ($commandExitCode -ne 0) {
    throw "Command failed with exit code $commandExitCode`: $displayName"
  }
}

function Find-MsvcEnvironmentScript {
  $knownPaths = @(
    "C:\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
    "${env:ProgramFiles}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
  )

  foreach ($candidate in $knownPaths) {
    if (Test-Path -LiteralPath $candidate) {
      return $candidate
    }
  }

  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  if (Test-Path -LiteralPath $vswhere) {
    $installPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($installPath) {
      $candidate = Join-Path $installPath "VC\Auxiliary\Build\vcvars64.bat"
      if (Test-Path -LiteralPath $candidate) {
        return $candidate
      }
    }
  }

  throw "MSVC C++ build tools were not found. Install the Visual Studio 2022 C++ workload."
}

function Import-MsvcEnvironment {
  $vcvars = Find-MsvcEnvironmentScript
  Write-Host "MSVC: $vcvars"

  $environmentLines = & $env:ComSpec /d /s /c "call `"$vcvars`" >nul && set"
  if ($LASTEXITCODE -ne 0) {
    throw "MSVC environment setup failed with exit code $LASTEXITCODE."
  }

  $msvcPath = $null
  foreach ($line in $environmentLines) {
    $separator = $line.IndexOf("=")
    if ($separator -gt 0) {
      $name = $line.Substring(0, $separator)
      $value = $line.Substring($separator + 1)
      if ($name.Equals("Path", [StringComparison]::OrdinalIgnoreCase)) {
        if (-not $msvcPath -or $value -match '\\VC\\Tools\\MSVC\\') {
          $msvcPath = $value
        }
        continue
      }
      [Environment]::SetEnvironmentVariable($name, $value, "Process")
    }
  }
  if (-not $msvcPath) {
    throw "MSVC setup did not return a compiler PATH."
  }
  $env:Path = $msvcPath
}

function Add-ToolPath {
  param([string]$Path)

  if ($Path -and (Test-Path -LiteralPath $Path)) {
    $remaining = @($env:Path -split ";" | Where-Object {
      $_ -and -not $_.Equals($Path, [StringComparison]::OrdinalIgnoreCase)
    })
    $env:Path = (@($Path) + $remaining) -join ";"
  }
}

function Get-DependencyFingerprint {
  $packageHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $Workspace "package.json")).Hash
  $lockHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $Workspace "pnpm-lock.yaml")).Hash
  return "$packageHash`n$lockHash"
}

function Sync-FrontendDependencies {
  param([string]$Pnpm)

  $nodeModules = Join-Path $Workspace "node_modules"
  $fingerprintPath = Join-Path $nodeModules ".reelspace-dependencies"
  $expected = Get-DependencyFingerprint
  $current = if (Test-Path -LiteralPath $fingerprintPath) {
    Get-Content -Raw -LiteralPath $fingerprintPath
  } else {
    ""
  }

  $modulesManifest = Join-Path $nodeModules ".modules.yaml"
  if (-not $current -and (Test-Path -LiteralPath $modulesManifest)) {
    $dependencyTimestamp = (Get-Item -LiteralPath $modulesManifest).LastWriteTimeUtc
    $manifestsAreOlder = @("package.json", "pnpm-lock.yaml") | ForEach-Object {
      (Get-Item -LiteralPath (Join-Path $Workspace $_)).LastWriteTimeUtc -le $dependencyTimestamp
    }
    if ($manifestsAreOlder -notcontains $false) {
      Set-Content -LiteralPath $fingerprintPath -Value $expected -Encoding ASCII
      Write-Host "Frontend dependencies: existing install verified (install skipped)." -ForegroundColor Green
      return
    }
  }

  if ((Test-Path -LiteralPath $modulesManifest) -and $current.Trim() -eq $expected.Trim()) {
    Write-Host "Frontend dependencies: current (install skipped)." -ForegroundColor Green
    return
  }

  Write-Stage "Syncing frontend dependencies"
  Invoke-RunnerCommand -FilePath $Pnpm -Arguments @("install", "--frozen-lockfile", "--prefer-offline")
  Set-Content -LiteralPath $fingerprintPath -Value $expected -Encoding ASCII
}

function Assert-PackagingResources {
  $resources = @(
    @{ Name = "FFmpeg"; Path = "vendor\ffmpeg\bin\ffmpeg.exe"; MinimumBytes = 1MB },
    @{ Name = "FFprobe"; Path = "vendor\ffmpeg\bin\ffprobe.exe"; MinimumBytes = 1MB },
    @{ Name = "application icon"; Path = "src-tauri\icons\icon.ico"; MinimumBytes = 1KB }
  )

  foreach ($resource in $resources) {
    $resolved = Join-Path $Workspace $resource.Path
    if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
      throw "$($resource.Name) is missing: $resolved"
    }
    $file = Get-Item -LiteralPath $resolved
    if ($file.Length -lt $resource.MinimumBytes) {
      throw "$($resource.Name) appears incomplete: $resolved ($($file.Length) bytes)"
    }
    Write-Host ("{0}: {1:N1} MB" -f $resource.Name, ($file.Length / 1MB)) -ForegroundColor Green
  }
}

function Assert-WindowsExecutable {
  param([string]$Path)

  $file = Get-Item -LiteralPath $Path -ErrorAction Stop
  if ($file.Length -lt 1MB) {
    throw "Generated setup is unexpectedly small: $($file.Length) bytes."
  }

  $stream = [IO.File]::OpenRead($file.FullName)
  try {
    if ($stream.ReadByte() -ne 0x4D -or $stream.ReadByte() -ne 0x5A) {
      throw "Generated setup does not have a valid Windows executable header."
    }
  } finally {
    $stream.Dispose()
  }
}

function Copy-InstallerArtifacts {
  $installerDir = Join-Path $Workspace "src-tauri\target\release\bundle\nsis"
  $outputDir = Join-Path $Workspace "dist"
  $installers = @(Get-ChildItem -LiteralPath $installerDir -Filter "*.exe" -File -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTimeUtc -Descending)
  if ($installers.Count -eq 0) {
    throw "Tauri completed, but no NSIS setup executable was found in $installerDir."
  }

  New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
  $setupPath = Join-Path $outputDir "setup.exe"
  Copy-Item -LiteralPath $installers[0].FullName -Destination $setupPath -Force
  Assert-WindowsExecutable -Path $setupPath

  $setupFile = Get-Item -LiteralPath $setupPath
  $setupHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $setupPath).Hash
  Write-Host "Setup:  $setupPath" -ForegroundColor Green
  Write-Host ("Size:   {0:N1} MB" -f ($setupFile.Length / 1MB)) -ForegroundColor Green
  Write-Host "SHA256: $setupHash" -ForegroundColor Green
}

try {
  Set-Location -LiteralPath $Workspace
  Start-Transcript -LiteralPath $LogPath -Append -Force | Out-Null
  $transcribing = $true

  $env:CI = "true"
  $env:CARGO_INCREMENTAL = "1"
  $env:CARGO_TERM_COLOR = "always"
  $env:FORCE_COLOR = "1"
  $env:RUST_BACKTRACE = "1"
  $env:VITE_CJS_IGNORE_WARNING = "true"

  Write-Host "============================================================"
  Write-Host "Reelspace build and run" -ForegroundColor White
  Write-Host "============================================================"
  Write-Host "Mode:      $Mode"
  Write-Host "Started:   $($startedAt.ToString('yyyy-MM-dd HH:mm:ss'))"
  Write-Host "Workspace: $Workspace"
  Write-Host "Live log:  $LogPath"

  Write-Stage "Preparing the Windows build environment"
  Import-MsvcEnvironment

  Add-ToolPath (Join-Path $env:USERPROFILE ".cargo\bin")
  Add-ToolPath "D:\Coding\.tools\nodejs\node_modules\corepack\shims"
  Add-ToolPath "D:\Coding\.tools\nodejs"
  Add-ToolPath (Join-Path $env:USERPROFILE ".cache\codex-runtimes\codex-primary-runtime\dependencies\bin\fallback")

  $pnpmCommand = Get-Command "pnpm.cmd" -ErrorAction SilentlyContinue
  if (-not $pnpmCommand) {
    $pnpmCommand = Get-Command "pnpm" -ErrorAction SilentlyContinue
  }
  $cargoCommand = Get-Command "cargo.exe" -ErrorAction SilentlyContinue
  if (-not $cargoCommand) {
    $cargoCommand = Get-Command "cargo" -ErrorAction SilentlyContinue
  }
  if (-not $pnpmCommand) {
    throw "pnpm was not found on PATH."
  }
  if (-not $cargoCommand) {
    throw "Rust/Cargo was not found on PATH."
  }
  if (-not (Get-Command "link.exe" -ErrorAction SilentlyContinue)) {
    throw "MSVC link.exe was not found after loading the build environment."
  }

  Write-Host "pnpm: $(& $pnpmCommand.Source --version)"
  Write-Host "cargo: $(& $cargoCommand.Source --version)"
  Sync-FrontendDependencies -Pnpm $pnpmCommand.Source

  switch ($Mode) {
    "check" {
      Write-Stage "Checking the frontend"
      Invoke-RunnerCommand -FilePath $pnpmCommand.Source -Arguments @("build")
      Write-Stage "Checking the Rust backend"
      Invoke-RunnerCommand -FilePath $cargoCommand.Source -Arguments @("check", "--manifest-path", "src-tauri\Cargo.toml")
    }
    "build" {
      Write-Stage "Building the frontend"
      Invoke-RunnerCommand -FilePath $pnpmCommand.Source -Arguments @("build")
      Write-Stage "Building the debug application"
      Invoke-RunnerCommand -FilePath $cargoCommand.Source -Arguments @("build", "--manifest-path", "src-tauri\Cargo.toml")
    }
    "setup" {
      Write-Stage "Checking bundled setup resources"
      Assert-PackagingResources
      Write-Stage "Building the optimized app and NSIS installer"
      Write-Host "Release builds are intentionally slower; incremental artifacts are retained for the next build."
      Invoke-RunnerCommand -FilePath $pnpmCommand.Source -Arguments @("run", "tauri:build")
      Write-Stage "Collecting setup files in dist"
      Copy-InstallerArtifacts
    }
    default {
      Write-Stage "Starting Vite, compiling changed Rust code, and launching Reelspace"
      Write-Host "Keep this window open while testing. Frontend edits reload automatically."
      Write-Host "Rust edits rebuild incrementally. Closing this window terminates the full process tree."
      Invoke-RunnerCommand -FilePath $pnpmCommand.Source -Arguments @("run", "tauri:dev")
    }
  }

  $elapsed = (Get-Date) - $startedAt
  Write-Host ""
  Write-Host ("SUCCESS after {0:hh\:mm\:ss}." -f $elapsed) -ForegroundColor Green
  $exitCode = 0
} catch {
  $elapsed = (Get-Date) - $startedAt
  Write-Host ""
  Write-Host ("FAILED after {0:hh\:mm\:ss}: {1}" -f $elapsed, $_.Exception.Message) -ForegroundColor Red
  Write-Host "Full log: $LogPath" -ForegroundColor Yellow
  $exitCode = 1
} finally {
  if ($transcribing) {
    Stop-Transcript | Out-Null
  }
}

exit $exitCode
