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

if not exist "node_modules" (
  echo Installing frontend dependencies...
  pnpm install
  if errorlevel 1 exit /b 1
)

echo Building frontend...
pnpm build
if errorlevel 1 exit /b 1

echo Building release binary...
cargo build --manifest-path src-tauri\Cargo.toml --release
if errorlevel 1 exit /b 1

echo Launching Reelspace...
src-tauri\target\release\reelspace.exe
endlocal
