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

if not exist "node_modules" (
  echo Installing frontend dependencies...
  pnpm install
  if errorlevel 1 exit /b 1
)

echo Building frontend...
pnpm build
if errorlevel 1 exit /b 1

echo Building debug binary...
echo First executable build can take several minutes because Tauri/WebView2 crates must be code-generated and linked.
cargo build --manifest-path src-tauri\Cargo.toml
if errorlevel 1 exit /b 1

echo Launching Reelspace...
src-tauri\target\debug\reelspace.exe
endlocal
