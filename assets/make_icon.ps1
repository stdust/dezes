# Generates assets/dezes.ico.
#
# The icon is drawn in code rather than committed as an opaque binary so it can be
# tweaked (colours, grid, radius) without an image editor. Run from the repo root:
#
#   pwsh -File assets/make_icon.ps1
#
# Output: a multi-size .ico (16/32/48/64), 32-bit BGRA with an alpha channel, which
# is what `winresource` embeds via build.rs.

$ErrorActionPreference = 'Stop'

# --- palette (BGRA order is applied when writing) -----------------------------
$bg     = @(0x16, 0x21, 0x2E)  # near-black blue, matching the dark theme
$border = @(0x3E, 0x7C, 0xB1)  # accent blue, same family as the address column
$byte   = @(0x7E, 0xC8, 0xA9)  # green "bytes"
$edited = @(0xFF, 0xC9, 0x4D)  # gold, the colour a changed byte gets in the view

function New-IconImage([int]$size) {
    $w = $size
    $h = $size
    # pixels[y][x] = @(r, g, b, a), row 0 at the top.
    $pixels = New-Object 'object[]' $h
    for ($y = 0; $y -lt $h; $y++) {
        $row = New-Object 'object[]' $w
        for ($x = 0; $x -lt $w; $x++) { $row[$x] = @(0, 0, 0, 0) }
        $pixels[$y] = $row
    }

    $radius = [Math]::Max(2, [int][Math]::Round($size * 0.16))
    $borderWidth = if ($size -ge 32) { [int][Math]::Round($size / 16.0) } else { 1 }

    # Rounded square: filled with the background, outlined in the accent.
    for ($y = 0; $y -lt $h; $y++) {
        for ($x = 0; $x -lt $w; $x++) {
            # Distance from the nearest corner centre, for the rounded corners.
            $cx = if ($x -lt $radius) { $radius } elseif ($x -ge $w - $radius) { $w - 1 - $radius } else { $x }
            $cy = if ($y -lt $radius) { $radius } elseif ($y -ge $h - $radius) { $h - 1 - $radius } else { $y }
            $dx = $x - $cx
            $dy = $y - $cy
            $dist = [Math]::Sqrt($dx * $dx + $dy * $dy)
            if ($dist -gt $radius) { continue }

            $edge = ($dist -gt ($radius - $borderWidth)) -or
                    ($x -lt $borderWidth) -or ($x -ge $w - $borderWidth) -or
                    ($y -lt $borderWidth) -or ($y -ge $h - $borderWidth)

            $pixels[$y][$x] = if ($edge) {
                @($border[0], $border[1], $border[2], 255)
            } else {
                @($bg[0], $bg[1], $bg[2], 255)
            }
        }
    }

    # A 3x3 grid of "bytes", with the middle one gold - the shape of a hex dump
    # with one patched value in it.
    $cell = [Math]::Max(1, [int][Math]::Round($size * 0.18))
    $gap = [Math]::Max(1, [int][Math]::Round($size * 0.07))
    $span = 3 * $cell + 2 * $gap
    $originX = [int][Math]::Round(($w - $span) / 2.0)
    $originY = [int][Math]::Round(($h - $span) / 2.0)

    for ($gy = 0; $gy -lt 3; $gy++) {
        for ($gx = 0; $gx -lt 3; $gx++) {
            $color = if ($gx -eq 1 -and $gy -eq 1) { $edited } else { $byte }
            $x0 = $originX + $gx * ($cell + $gap)
            $y0 = $originY + $gy * ($cell + $gap)
            for ($y = $y0; $y -lt $y0 + $cell; $y++) {
                for ($x = $x0; $x -lt $x0 + $cell; $x++) {
                    if ($x -ge 0 -and $x -lt $w -and $y -ge 0 -and $y -lt $h) {
                        $pixels[$y][$x] = @($color[0], $color[1], $color[2], 255)
                    }
                }
            }
        }
    }

    # BITMAPINFOHEADER + bottom-up BGRA rows + an empty AND mask, which is what an
    # .ico entry holds for a 32-bit image.
    $bytes = New-Object System.Collections.Generic.List[byte]
    function Add-U16([int]$v) { $bytes.Add([byte]($v -band 0xFF)); $bytes.Add([byte](($v -shr 8) -band 0xFF)) }
    function Add-U32([int]$v) {
        $bytes.Add([byte]($v -band 0xFF))
        $bytes.Add([byte](($v -shr 8) -band 0xFF))
        $bytes.Add([byte](($v -shr 16) -band 0xFF))
        $bytes.Add([byte](($v -shr 24) -band 0xFF))
    }

    Add-U32 40          # biSize
    Add-U32 $w          # biWidth
    Add-U32 ($h * 2)    # biHeight: image + mask, per the .ico convention
    Add-U16 1           # biPlanes
    Add-U16 32          # biBitCount
    Add-U32 0           # biCompression = BI_RGB
    Add-U32 ($w * $h * 4)
    Add-U32 0; Add-U32 0; Add-U32 0; Add-U32 0

    for ($y = $h - 1; $y -ge 0; $y--) {
        for ($x = 0; $x -lt $w; $x++) {
            $p = $pixels[$y][$x]
            $bytes.Add([byte]$p[2])  # B
            $bytes.Add([byte]$p[1])  # G
            $bytes.Add([byte]$p[0])  # R
            $bytes.Add([byte]$p[3])  # A
        }
    }

    $maskRow = [int][Math]::Ceiling($w / 32.0) * 4
    for ($i = 0; $i -lt $maskRow * $h; $i++) { $bytes.Add([byte]0) }

    return $bytes.ToArray()
}

$sizes = @(16, 32, 48, 64)
$images = @()
foreach ($size in $sizes) { $images += ,(New-IconImage $size) }

$out = New-Object System.Collections.Generic.List[byte]
function Out-U16([int]$v) { $out.Add([byte]($v -band 0xFF)); $out.Add([byte](($v -shr 8) -band 0xFF)) }
function Out-U32([int]$v) {
    $out.Add([byte]($v -band 0xFF))
    $out.Add([byte](($v -shr 8) -band 0xFF))
    $out.Add([byte](($v -shr 16) -band 0xFF))
    $out.Add([byte](($v -shr 24) -band 0xFF))
}

Out-U16 0                 # reserved
Out-U16 1                 # type: icon
Out-U16 $sizes.Count

$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $size = $sizes[$i]
    $data = $images[$i]
    $out.Add([byte]($size -band 0xFF))
    $out.Add([byte]($size -band 0xFF))
    $out.Add([byte]0)     # palette entries
    $out.Add([byte]0)     # reserved
    Out-U16 1             # planes
    Out-U16 32            # bits per pixel
    Out-U32 $data.Length
    Out-U32 $offset
    $offset += $data.Length
}
foreach ($data in $images) {
    # A cast: PowerShell hands back Object[] from the helper, and AddRange wants a
    # typed byte sequence.
    $out.AddRange([byte[]]$data)
}

$target = Join-Path $PSScriptRoot 'dezes.ico'
[System.IO.File]::WriteAllBytes($target, $out.ToArray())
Write-Host "wrote $target ($($out.Count) bytes, sizes: $($sizes -join ', '))"
