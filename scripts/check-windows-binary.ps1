[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string]$Path,
  [string]$ExpectedVersion
)

$ErrorActionPreference = "Stop"
$resolvedPath = (Resolve-Path -LiteralPath $Path).Path
$bytes = [System.IO.File]::ReadAllBytes($resolvedPath)
if ($bytes.Length -lt 512 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
  throw "Not a valid Windows PE file: $resolvedPath"
}

$peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
if ($peOffset -lt 0 -or $peOffset + 96 -gt $bytes.Length) {
  throw "The Windows PE header is outside the file: $resolvedPath"
}
if ([BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x00004550) {
  throw "The Windows PE signature is invalid: $resolvedPath"
}

$machine = [BitConverter]::ToUInt16($bytes, $peOffset + 4)
$optionalMagic = [BitConverter]::ToUInt16($bytes, $peOffset + 24)
$subsystem = [BitConverter]::ToUInt16($bytes, $peOffset + 24 + 68)
if ($machine -ne 0x8664 -or $optionalMagic -ne 0x020b) {
  throw "Expected an x64 PE32+ executable; machine=0x$($machine.ToString('X4')), optional=0x$($optionalMagic.ToString('X4'))"
}
if ($subsystem -ne 2) {
  throw "Expected the Windows GUI subsystem (2), but found subsystem $subsystem. A console window may appear."
}

$version = (Get-Item -LiteralPath $resolvedPath).VersionInfo.ProductVersion
if (-not [string]::IsNullOrWhiteSpace($ExpectedVersion) -and $version -cne $ExpectedVersion) {
  throw "Executable version '$version' does not match '$ExpectedVersion': $resolvedPath"
}

Write-Host "Verified Windows x64 GUI executable $resolvedPath (version $version)." -ForegroundColor Green
