@echo off
setlocal EnableExtensions
cd /d "%~dp0"

for /f %%I in ('powershell -NoProfile -Command "Get-Date -Format yyyyMMdd-HHmmss"') do set "RUN_TS=%%I"
set "LOG_DIR=%CD%\logs\runners"
set "RUN_LOG=%LOG_DIR%\build-run-%RUN_TS%.txt"
set "NO_PAUSE="

for %%A in (%*) do if /i "%%~A"=="--no-pause" set "NO_PAUSE=1"

if not exist "%LOG_DIR%" mkdir "%LOG_DIR%"

echo ============================================================
echo Reelspace build and run
echo ============================================================
echo Workspace: %CD%
echo Live log:  %RUN_LOG%
echo.

powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%CD%\scripts\runner-supervisor.ps1" -Workspace "%CD%" -LogPath "%RUN_LOG%" -RunnerScript "%CD%\scripts\runner.ps1" %*
set "RUN_EXIT=%ERRORLEVEL%"

echo.
if "%RUN_EXIT%"=="0" (
  echo Reelspace runner finished normally.
) else (
  echo Reelspace runner failed with exit code %RUN_EXIT%.
)
echo Full log: %RUN_LOG%

if not "%RUN_EXIT%"=="0" if not defined NO_PAUSE (
  echo.
  echo Press any key to close this runner...
  pause >nul
)

endlocal & exit /b %RUN_EXIT%
