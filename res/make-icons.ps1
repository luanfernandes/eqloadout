# Renders docs/brand/*.svg into res/*.ico and the README banner with headless Edge.
# Usage: powershell -ExecutionPolicy Bypass -File res\make-icons.ps1
param([string]$Edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe")
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$root = Split-Path $PSScriptRoot
$brand = Join-Path $root 'docs\brand'
$tmp = Join-Path ([IO.Path]::GetTempPath()) "eqloadout-icons-$PID"
New-Item -ItemType Directory -Force $tmp | Out-Null

function Shot([string]$url, [int]$w, [int]$h, [string]$out) {
    # Separate profile so an already running Edge doesn't swallow the request.
    $args = @('--headless=new', '--disable-gpu', '--hide-scrollbars', '--default-background-color=00000000',
        "--user-data-dir=$tmp\profile", "--window-size=$w,$h", '--virtual-time-budget=5000', "--screenshot=$out", $url)
    Start-Process -FilePath $Edge -ArgumentList $args -Wait -WindowStyle Hidden
    if (-not (Test-Path $out)) { throw "Edge did not render $url" }
}

function Ico([string]$png, [string]$out) {
    $src = [Drawing.Image]::FromFile($png)
    $entries = foreach ($s in 16, 20, 24, 32, 40, 48, 64, 256) {
        $bmp = New-Object Drawing.Bitmap $s, $s
        $g = [Drawing.Graphics]::FromImage($bmp)
        $g.InterpolationMode = 'HighQualityBicubic'
        $g.PixelOffsetMode = 'HighQuality'
        $g.DrawImage($src, 0, 0, $s, $s)
        $g.Dispose()
        $ms = New-Object IO.MemoryStream
        if ($s -ge 256) {
            $bmp.Save($ms, [Drawing.Imaging.ImageFormat]::Png)
        } else {
            # Small sizes as 32-bit DIBs: every icon API reads these, not all read PNG entries.
            $data = $bmp.LockBits((New-Object Drawing.Rectangle 0, 0, $s, $s), 'ReadOnly', 'Format32bppArgb')
            $px = New-Object byte[] ($s * $s * 4)
            [Runtime.InteropServices.Marshal]::Copy($data.Scan0, $px, 0, $px.Length)
            $bmp.UnlockBits($data)
            $w = New-Object IO.BinaryWriter $ms
            $w.Write([uint32]40); $w.Write([int32]$s); $w.Write([int32]($s * 2)); $w.Write([uint16]1); $w.Write([uint16]32)
            $w.Write([uint32]0); $w.Write([uint32]0); $w.Write([int32]0); $w.Write([int32]0); $w.Write([uint32]0); $w.Write([uint32]0)
            for ($y = $s - 1; $y -ge 0; $y--) { $w.Write($px, $y * $s * 4, $s * 4) } # bottom-up BGRA
            $w.Write((New-Object byte[] ([math]::Ceiling($s / 32) * 4 * $s)))     # empty AND mask
            $w.Flush()
        }
        $bmp.Dispose()
        , @($s, $ms.ToArray())
    }
    $src.Dispose()
    $f = New-Object IO.BinaryWriter ([IO.File]::Create($out))
    $f.Write([uint16]0); $f.Write([uint16]1); $f.Write([uint16]$entries.Count)
    $offset = 6 + 16 * $entries.Count
    foreach ($e in $entries) {
        $d = if ($e[0] -ge 256) { 0 } else { $e[0] }
        $f.Write([byte]$d); $f.Write([byte]$d); $f.Write([byte]0); $f.Write([byte]0)
        $f.Write([uint16]1); $f.Write([uint16]32)
        $f.Write([uint32]$e[1].Length); $f.Write([uint32]$offset)
        $offset += $e[1].Length
    }
    foreach ($e in $entries) { $f.Write($e[1]) }
    $f.Close()
}

foreach ($pair in @(('logo', 'eqloadout'), ('tray-paused', 'paused'), ('tray-warning', 'warning'))) {
    $png = Join-Path $tmp "$($pair[0]).png"
    Shot "file:///$brand/$($pair[0]).svg" 256 256 $png
    Ico $png (Join-Path $PSScriptRoot "$($pair[1]).ico")
}
Copy-Item (Join-Path $tmp 'logo.png') (Join-Path $brand 'logo-256.png')
Shot "file:///$brand/banner.html" 1280 400 (Join-Path $brand 'banner.png')
Shot "file:///$brand/banner.html" 1280 640 (Join-Path $brand 'social.png')
Remove-Item -Recurse -Force $tmp
Write-Host "Icons written to $PSScriptRoot, banner to $brand"
