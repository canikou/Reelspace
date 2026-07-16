@echo off
setlocal
cd /d "%~dp0"
set CI=true
call scripts\use-msvc-env.bat

where pnpm >nul 2>nul
if errorlevel 1 (
  echo pnpm was not found.
  exit /b 1
)

where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust/Cargo was not found.
  exit /b 1
)

echo Checking TypeScript...
call node_modules\.bin\tsc.cmd
if errorlevel 1 exit /b 1

echo Building frontend...
call node_modules\.bin\vite.cmd build
if errorlevel 1 exit /b 1

echo Checking Rust backend...
cargo check --manifest-path src-tauri\Cargo.toml
if errorlevel 1 exit /b 1

echo All fast checks passed.
endlocal
