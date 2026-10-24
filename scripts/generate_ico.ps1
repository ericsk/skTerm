Add-Type -AssemblyName System.Drawing

$srcPath = Join-Path $PSScriptRoot "..\assets\icon_glass_fullbleed.png"
$outPath = Join-Path $PSScriptRoot "..\assets\app_icon.ico"

if (-not (Test-Path $srcPath)) {
    Write-Error "Source image not found: $srcPath"
    exit 1
}

$src = [System.Drawing.Image]::FromFile($srcPath)
# Standard Windows multi-resolution icon sizes (including 24x24 for Windows 11 taskbar/DPI scaling)
$sizes = @(256, 128, 64, 48, 32, 24, 16)
$imagePayloads = @()

foreach ($s in $sizes) {
    $bmp = New-Object System.Drawing.Bitmap($s, $s, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.Clear([System.Drawing.Color]::Transparent)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g.DrawImage($src, 0, 0, $s, $s)
    $g.Dispose()

    $ms = New-Object System.IO.MemoryStream
    if ($s -ge 256) {
        # 256x256 frame uses standard Vista+ PNG compression
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    } else {
        # Sizes < 256 use uncompressed 32-bit BGRA DIB (BITMAPINFOHEADER + XOR + AND mask)
        # for maximum compatibility with Win32 GDI, LoadImageW, WM_SETICON, and Windows Shell.
        $dibWriter = New-Object System.IO.BinaryWriter($ms)
        $andRowStride = [int]([Math]::Ceiling($s / 32.0) * 4)
        $xorSize = $s * $s * 4
        $andSize = $andRowStride * $s

        # BITMAPINFOHEADER (40 bytes)
        $dibWriter.Write([uint32]40)            # biSize
        $dibWriter.Write([int32]$s)             # biWidth
        $dibWriter.Write([int32]($s * 2))       # biHeight (XOR + AND combined height)
        $dibWriter.Write([uint16]1)             # biPlanes
        $dibWriter.Write([uint16]32)            # biBitCount (32bpp BGRA)
        $dibWriter.Write([uint32]0)             # biCompression (BI_RGB = 0)
        $dibWriter.Write([uint32]($xorSize + $andSize)) # biSizeImage
        $dibWriter.Write([int32]0)              # biXPelsPerMeter
        $dibWriter.Write([int32]0)              # biYPelsPerMeter
        $dibWriter.Write([uint32]0)             # biClrUsed
        $dibWriter.Write([uint32]0)             # biClrImportant

        # Write bottom-up BGRA XOR mask
        for ($y = $s - 1; $y -ge 0; $y--) {
            for ($x = 0; $x -lt $s; $x++) {
                $c = $bmp.GetPixel($x, $y)
                $dibWriter.Write([byte]$c.B)
                $dibWriter.Write([byte]$c.G)
                $dibWriter.Write([byte]$c.R)
                $dibWriter.Write([byte]$c.A)
            }
        }

        # Write bottom-up 1bpp AND mask (padded to 32-bit row boundary)
        for ($y = $s - 1; $y -ge 0; $y--) {
            $rowBytes = New-Object byte[] $andRowStride
            for ($x = 0; $x -lt $s; $x++) {
                $c = $bmp.GetPixel($x, $y)
                if ($c.A -eq 0) {
                    $byteIdx = [int]([Math]::Floor($x / 8))
                    $bitIdx = 7 - ($x % 8)
                    $rowBytes[$byteIdx] = $rowBytes[$byteIdx] -bor (1 -shl $bitIdx)
                }
            }
            $dibWriter.Write($rowBytes)
        }
        $dibWriter.Flush()
    }
    $bmp.Dispose()
    $imagePayloads += ,($ms.ToArray())
    $ms.Dispose()
}
$src.Dispose()

$fs = [System.IO.File]::Create($outPath)
$bw = New-Object System.IO.BinaryWriter($fs)

# ICONDIR header: 6 bytes
$bw.Write([uint16]0) # Reserved
$bw.Write([uint16]1) # Type: 1 = ICO
$bw.Write([uint16]$sizes.Count) # Number of images

$offset = 6 + (16 * $sizes.Count)

for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]
    $w = if ($s -ge 256) { [byte]0 } else { [byte]$s }
    $h = if ($s -ge 256) { [byte]0 } else { [byte]$s }
    $bw.Write($w)
    $bw.Write($h)
    $bw.Write([byte]0) # Color count
    $bw.Write([byte]0) # Reserved
    $bw.Write([uint16]1) # Planes
    $bw.Write([uint16]32) # Bit count
    $bytes = $imagePayloads[$i]
    $bw.Write([uint32]$bytes.Length) # BytesInRes
    $bw.Write([uint32]$offset) # ImageOffset
    $offset += $bytes.Length
}

for ($i = 0; $i -lt $sizes.Count; $i++) {
    $bw.Write($imagePayloads[$i])
}

$bw.Flush()
$fs.Close()
Write-Host "Successfully generated $outPath"
