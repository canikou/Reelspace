@echo off
set "CARGO_HOME=%USERPROFILE%\.cargo"
set "PATH=%CARGO_HOME%\bin;%PATH%"

if exist "C:\BuildTools\VC\Auxiliary\Build\vcvars64.bat" (
  call "C:\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
  goto :eof
)

if exist "%ProgramFiles%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" (
  call "%ProgramFiles%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
  goto :eof
)

if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" (
  for /f "usebackq tokens=*" %%I in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do (
    if exist "%%I\VC\Auxiliary\Build\vcvars64.bat" call "%%I\VC\Auxiliary\Build\vcvars64.bat" >nul
  )
)
