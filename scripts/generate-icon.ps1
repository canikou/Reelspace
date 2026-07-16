$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent $PSScriptRoot
$iconDir = Join-Path $root "src-tauri\icons"
$pngPath = Join-Path $iconDir "icon.png"
$icoPath = Join-Path $iconDir "icon.ico"

New-Item -ItemType Directory -Force $iconDir | Out-Null

$bitmap = New-Object System.Drawing.Bitmap 256, 256
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.Clear([System.Drawing.Color]::FromArgb(24, 26, 31))

$brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
  (New-Object System.Drawing.Rectangle 0, 0, 256, 256),
  [System.Drawing.Color]::FromArgb(39, 110, 241),
  [System.Drawing.Color]::FromArgb(113, 215, 183),
  45
)
$graphics.FillRectangle($brush, 28, 28, 200, 200)

$inner = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(24, 26, 31))
$graphics.FillRectangle($inner, 58, 66, 140, 124)

$accent = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 255, 255))
$graphics.FillRectangle($accent, 78, 88, 100, 18)
$graphics.FillRectangle($accent, 78, 124, 64, 18)
$graphics.FillRectangle($accent, 78, 160, 92, 18)

$graphics.Dispose()
$bitmap.Save($pngPath, [System.Drawing.Imaging.ImageFormat]::Png)

$pngBytes = [System.IO.File]::ReadAllBytes($pngPath)
$stream = New-Object System.IO.FileStream($icoPath, [System.IO.FileMode]::Create)
$writer = New-Object System.IO.BinaryWriter($stream)
$writer.Write([UInt16]0)
$writer.Write([UInt16]1)
$writer.Write([UInt16]1)
$writer.Write([Byte]0)
$writer.Write([Byte]0)
$writer.Write([Byte]0)
$writer.Write([Byte]0)
$writer.Write([UInt16]1)
$writer.Write([UInt16]32)
$writer.Write([UInt32]$pngBytes.Length)
$writer.Write([UInt32]22)
$writer.Write($pngBytes)
$writer.Close()
$stream.Close()

$bitmap.Dispose()
