@echo off
setlocal
cd /d "%~dp0"
set CI=true
call scripts\use-msvc-env.bat

where pnpm >nul 2>nul
if errorlevel 1 (
  echo pnpm was not found. Install pnpm, then run this again.
  exit /b 1
)

where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust/Cargo was not found. Install Rust stable from https://rustup.rs/.
  exit /b 1
)

if not exist "vendor\ffmpeg\bin\ffmpeg.exe" (
  echo Missing vendor\ffmpeg\bin\ffmpeg.exe
  exit /b 1
)

if not exist "vendor\ffmpeg\bin\ffprobe.exe" (
  echo Missing vendor\ffmpeg\bin\ffprobe.exe
  exit /b 1
)

if not exist "node_modules" (
  echo Installing frontend dependencies...
  pnpm install
  if errorlevel 1 exit /b 1
)

echo Building optimized installer. This is slower than build-run.bat because NSIS packages bundled assets.
pnpm tauri build
if errorlevel 1 exit /b 1

if not exist "dist" mkdir dist
for %%F in (src-tauri\target\release\bundle\nsis\*.exe) do copy /Y "%%F" "dist\"

echo Installer copied to dist.
endlocal
