@echo off
setlocal
cd /d "%~dp0"
set CI=true

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
  echo Copy FFmpeg binaries into vendor\ffmpeg\bin before scanning or trimming.
)

if not exist "vendor\ffmpeg\bin\ffprobe.exe" (
  echo Missing vendor\ffmpeg\bin\ffprobe.exe
  echo Copy FFprobe into vendor\ffmpeg\bin before scanning or trimming.
)

if not exist "node_modules" (
  echo Installing frontend dependencies...
  pnpm install
  if errorlevel 1 exit /b 1
)

set RUST_BACKTRACE=1
pnpm tauri dev
endlocal
