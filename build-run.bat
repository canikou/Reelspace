@echo off
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

if defined REELSPACE_LOGGING goto :inner_main

for /f %%I in ('powershell -NoProfile -Command "Get-Date -Format yyyyMMdd-HHmmss"') do set "RUN_TS=%%I"
set "LOG_DIR=logs\runners"
if not exist "logs" mkdir logs
if not exist "%LOG_DIR%" mkdir "%LOG_DIR%"
set "REELSPACE_LOG=%CD%\%LOG_DIR%\build-run-%RUN_TS%.txt"

echo Reelspace runner starting...
echo Live log: %REELSPACE_LOG%
echo.

powershell -NoProfile -ExecutionPolicy Bypass -Command "$env:REELSPACE_LOGGING='1'; $env:REELSPACE_LOG='%REELSPACE_LOG%'; & '%~f0' %* 2>&1 | ForEach-Object { $_.ToString() } | Tee-Object -FilePath $env:REELSPACE_LOG -Append; exit $LASTEXITCODE"
set "OUTER_CODE=%ERRORLEVEL%"
echo.
echo Reelspace runner finished with exit code %OUTER_CODE%.
echo Full log: %REELSPACE_LOG%
endlocal
exit /b %OUTER_CODE%

:inner_main
set "CI=true"
set "MODE=run"
set "NO_PAUSE="

:parse_args
if "%~1"=="" goto :args_done
if /i "%~1"=="--check" set "MODE=check"
if /i "%~1"=="--no-launch" set "MODE=check"
if /i "%~1"=="--build-only" set "MODE=build"
if /i "%~1"=="--dist" set "MODE=dist"
if /i "%~1"=="--no-pause" set "NO_PAUSE=1"
shift
goto :parse_args

:args_done
call :banner "Reelspace build-run"
echo Mode: %MODE%
echo Started: %DATE% %TIME%
echo Log: %REELSPACE_LOG%
echo Workspace: %CD%
echo.

call :step "Loading Rust and MSVC build environment"
call :load_tools
if errorlevel 1 goto :fail

call :step "Checking required tools"
where pnpm >nul 2>nul
if errorlevel 1 (
  echo ERROR: pnpm was not found.
  goto :fail
)
where cargo >nul 2>nul
if errorlevel 1 (
  echo ERROR: Rust/Cargo was not found.
  goto :fail
)
where link >nul 2>nul
if errorlevel 1 (
  echo ERROR: MSVC link.exe was not found.
  goto :fail
)
call pnpm --version
cargo --version
echo.

if not exist "node_modules" (
  call :step "Installing frontend dependencies"
  call pnpm install
  if errorlevel 1 goto :fail
) else (
  call :step "Frontend dependencies already installed"
)

call :step "Building frontend"
call pnpm build
if errorlevel 1 goto :fail

if /i "%MODE%"=="check" (
  call :step "Checking Rust backend"
  cargo check --manifest-path src-tauri\Cargo.toml
  if errorlevel 1 goto :fail
  echo.
  echo Check mode complete. The app was not launched.
  goto :success
)

if /i "%MODE%"=="dist" (
  call :step "Building optimized installer"
  echo This is the slow path. Tauri will compile release code and package NSIS assets.
  call pnpm tauri build
  if errorlevel 1 goto :fail
  if not exist "dist" mkdir dist
  call :step "Copying installer artifacts to dist"
  for %%F in (src-tauri\target\release\bundle\nsis\*.exe) do copy /Y "%%F" "dist\"
  if errorlevel 1 goto :fail
  goto :success
)

call :step "Building debug binary"
echo The first executable build can take several minutes. Later builds should be much faster.
cargo build --manifest-path src-tauri\Cargo.toml
if errorlevel 1 goto :fail

if not exist "src-tauri\target\debug\reelspace.exe" (
  echo ERROR: src-tauri\target\debug\reelspace.exe was not created.
  goto :fail
)

if /i "%MODE%"=="build" (
  echo.
  echo Build-only mode complete. The app was not launched.
  goto :success
)

call :step "Launching Reelspace"
echo The runner will wait until the app closes.
start "" /wait "src-tauri\target\debug\reelspace.exe"
if errorlevel 1 goto :fail

:success
echo.
echo SUCCESS: build-run completed.
echo Full log: %REELSPACE_LOG%
if not defined NO_PAUSE pause
endlocal
exit /b 0

:fail
set "FAIL_CODE=%ERRORLEVEL%"
if "%FAIL_CODE%"=="0" set "FAIL_CODE=1"
echo.
echo FAILED: build-run stopped with exit code %FAIL_CODE%.
echo Full log: %REELSPACE_LOG%
pause
endlocal
exit /b %FAIL_CODE%

:banner
echo ============================================================
echo %~1
echo ============================================================
exit /b 0

:step
echo.
echo ------------------------------------------------------------
echo %~1
echo ------------------------------------------------------------
exit /b 0

:load_tools
set "CARGO_HOME=%USERPROFILE%\.cargo"
set "PATH=%CARGO_HOME%\bin;%PATH%"
echo Cargo bin: %CARGO_HOME%\bin

if exist "C:\BuildTools\VC\Auxiliary\Build\vcvars64.bat" (
  echo MSVC environment: C:\BuildTools
  call "C:\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
  exit /b 0
)

if exist "%ProgramFiles%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" (
  echo MSVC environment: %ProgramFiles%\Microsoft Visual Studio\2022\BuildTools
  call "%ProgramFiles%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
  exit /b 0
)

if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" (
  for /f "usebackq tokens=*" %%I in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do (
    if exist "%%I\VC\Auxiliary\Build\vcvars64.bat" (
      echo MSVC environment: %%I
      call "%%I\VC\Auxiliary\Build\vcvars64.bat" >nul
      exit /b 0
    )
  )
)

echo ERROR: MSVC environment was not found.
echo Install Visual Studio Build Tools with the C++ workload.
exit /b 1
