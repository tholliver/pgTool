param(
    [Parameter(Mandatory = $true)]
    [string]$Source,
    [string]$AssetsDir = (Join-Path $PSScriptRoot "..\assets")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

New-Item -ItemType Directory -Force -Path $AssetsDir | Out-Null

$src = [System.Drawing.Image]::FromFile($Source)
try {
    $side = [Math]::Min($src.Width, $src.Height)
    $crop = [System.Drawing.Rectangle]::new(
        [int](($src.Width - $side) / 2),
        [int](($src.Height - $side) / 2),
        $side,
        $side
    )

    function Resize-Square {
        param([int]$Size)
        $bmp = [System.Drawing.Bitmap]::new($Size, $Size)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
        $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $dest = [System.Drawing.Rectangle]::new(0, 0, $Size, $Size)
        $g.DrawImage($script:src, $dest, $script:crop, [System.Drawing.GraphicsUnit]::Pixel)
        $g.Dispose()
        $bmp
    }

    function Get-PngBytes {
        param([System.Drawing.Bitmap]$Bmp)
        $ms = [System.IO.MemoryStream]::new()
        $Bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $bytes = $ms.ToArray()
        $ms.Dispose()
        , $bytes
    }

    $sizes = @(16, 24, 32, 48, 64, 128, 256)
    $pngs = @{}
    foreach ($s in $sizes) {
        $bmp = Resize-Square $s
        try { $pngs[$s] = Get-PngBytes $bmp } finally { $bmp.Dispose() }
    }

    $icoPath = Join-Path $AssetsDir 'app.ico'
    $fs = [System.IO.File]::Create($icoPath)
    $bw = [System.IO.BinaryWriter]::new($fs)
    try {
        $sorted = $sizes | Sort-Object
        $bw.Write([uint16]0)
        $bw.Write([uint16]1)
        $bw.Write([uint16]$sorted.Count)
        $offset = 6 + 16 * $sorted.Count
        foreach ($s in $sorted) {
            $dim = if ($s -ge 256) { [byte]0 } else { [byte]$s }
            $len = $pngs[$s].Length
            $bw.Write($dim)
            $bw.Write($dim)
            $bw.Write([byte]0)
            $bw.Write([byte]0)
            $bw.Write([uint16]1)
            $bw.Write([uint16]32)
            $bw.Write([uint32]$len)
            $bw.Write([uint32]$offset)
            $offset += $len
        }
        foreach ($s in $sorted) { $bw.Write([byte[]]$pngs[$s]) }
        $bw.Flush()
    } finally {
        $bw.Dispose()
        $fs.Dispose()
    }

    $iconSize = 128
    $bmp = Resize-Square $iconSize
    try {
        $rect = [System.Drawing.Rectangle]::new(0, 0, $iconSize, $iconSize)
        $locked = $bmp.LockBits(
            $rect,
            [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
            [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
        )
        try {
            $raw = [byte[]]::new($iconSize * $iconSize * 4)
            [System.Runtime.InteropServices.Marshal]::Copy($locked.Scan0, $raw, 0, $raw.Length)
        } finally {
            $bmp.UnlockBits($locked)
        }
        for ($i = 0; $i -lt $raw.Length; $i += 4) {
            $b = $raw[$i]
            $raw[$i] = $raw[$i + 2]
            $raw[$i + 2] = $b
        }
        $out = [System.IO.MemoryStream]::new()
        $ow = [System.IO.BinaryWriter]::new($out)
        $ow.Write([uint32]$iconSize)
        $ow.Write([uint32]$iconSize)
        $ow.Write($raw)
        $ow.Flush()
        [System.IO.File]::WriteAllBytes((Join-Path $AssetsDir 'icon.rgba'), $out.ToArray())
        $ow.Dispose()
        $out.Dispose()
    } finally {
        $bmp.Dispose()
    }
} finally {
    $src.Dispose()
}

Get-Item (Join-Path $AssetsDir 'app.ico'), (Join-Path $AssetsDir 'icon.rgba') |
    Select-Object Name, Length
