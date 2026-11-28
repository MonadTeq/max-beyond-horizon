# Builds the NSIS bitmap skin from the owner's PNG art in max-brh-installer/resources.
#
# The installer script (installer.nsi) needs uncompressed BMPs: MUI/nsDialogs load
# bitmaps from files, and static controls cannot use PNG. This script generates a
# reproducible set of 24-bit BMPs and leaves the source PNGs untouched.
#
# Output (default): <repo>/.tmp/max-brh-installer/ui, outside git.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
    [string]$OutDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path '.tmp\max-brh-installer\ui')
)

$ErrorActionPreference = 'Stop'

$resourceDir = Join-Path $RepoRoot 'max-brh-installer\resources'
$framePng = Join-Path $resourceDir 'installer_ui_main_frame.png'
$bannerPng = Join-Path $resourceDir 'installer_ui_banner.png'
$progressPng = Join-Path $resourceDir 'installer_ui_progress_bar.png'
$panelPng = Join-Path $resourceDir 'install_in_progress_ui.png'
$icon = Join-Path $resourceDir 'game-icon.ico'

foreach ($path in @($framePng, $bannerPng, $progressPng, $panelPng, $icon)) {
    if (-not (Test-Path $path)) {
        throw "missing installer resource: $path"
    }
}

Add-Type -AssemblyName System.Drawing

New-Item -ItemType Directory -Force $OutDir | Out-Null

function Save-Bmp24 {
    param(
        [System.Drawing.Image]$Image,
        [string]$Path,
        [int]$Width = 0,
        [int]$Height = 0,
        [System.Drawing.Rectangle]$SourceRect = [System.Drawing.Rectangle]::Empty,
        [int]$DarkenAlpha = 0
    )

    if ($Width -le 0) { $Width = $Image.Width }
    if ($Height -le 0) { $Height = $Image.Height }

    $bitmap = New-Object System.Drawing.Bitmap($Width, $Height, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.Clear([System.Drawing.Color]::FromArgb(255, 16, 12, 8))
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

        $destination = New-Object System.Drawing.Rectangle(0, 0, $Width, $Height)
        if ($SourceRect -eq [System.Drawing.Rectangle]::Empty) {
            $graphics.DrawImage($Image, $destination)
        }
        else {
            $graphics.DrawImage($Image, $destination, $SourceRect, [System.Drawing.GraphicsUnit]::Pixel)
        }

        if ($DarkenAlpha -gt 0) {
            $brush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb($DarkenAlpha, 8, 6, 4))
            $graphics.FillRectangle($brush, $destination)
            $brush.Dispose()
        }
    }
    finally {
        $graphics.Dispose()
    }

    $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Bmp)
    $bitmap.Dispose()
}

$frameImage = [System.Drawing.Image]::FromFile($framePng)
$bannerImage = [System.Drawing.Image]::FromFile($bannerPng)
$panelImage = [System.Drawing.Image]::FromFile($panelPng)
$progressImage = [System.Drawing.Image]::FromFile($progressPng)

try {
    # Full-bleed page background: the owner's MAX window frame, darkened so that the
    # cream text and the dark content card remain readable on top of it.
    Save-Bmp24 -Image $frameImage -Path (Join-Path $OutDir 'frame.bmp') -DarkenAlpha 110

    # Dark parchment card used by all custom pages.
    $cardWidth = 900
    $cardHeight = 520
    $card = New-Object System.Drawing.Bitmap($cardWidth, $cardHeight, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $cardGraphics = [System.Drawing.Graphics]::FromImage($card)
    try {
        $cardGraphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $cardGraphics.Clear([System.Drawing.Color]::FromArgb(255, 16, 12, 8))

        $outerPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 122, 90, 46)), 3
        $cardGraphics.DrawRectangle($outerPen, 1, 1, $cardWidth - 3, $cardHeight - 3)
        $outerPen.Dispose()

        $innerPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 58, 44, 26)), 1
        $cardGraphics.DrawRectangle($innerPen, 8, 8, $cardWidth - 17, $cardHeight - 17)
        $innerPen.Dispose()
    }
    finally {
        $cardGraphics.Dispose()
    }
    $card.Save((Join-Path $OutDir 'card.bmp'), [System.Drawing.Imaging.ImageFormat]::Bmp)
    $card.Dispose()

    # The "installation in progress" console from the design; the native NSIS
    # progress bar is positioned in its horizontal slot.
    Save-Bmp24 -Image $panelImage -Path (Join-Path $OutDir 'panel.bmp')

    # Wide cinematic strip from the banner for the finish page.
    Save-Bmp24 -Image $bannerImage -Path (Join-Path $OutDir 'banner-strip.bmp') `
        -Width 900 -Height 180 `
        -SourceRect (New-Object System.Drawing.Rectangle(0, 300, 1672, 335))

    # Progress-bar palette sampled from the owner's progress art; used by the
    # report and by the NSIS script comments (the native bar keeps Windows' own
    # rendering, the panel art defines the slot geometry).
    $progressBitmap = New-Object System.Drawing.Bitmap($progressImage.Width, $progressImage.Height, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $progressGraphics = [System.Drawing.Graphics]::FromImage($progressBitmap)
    $progressGraphics.DrawImage($progressImage, 0, 0, $progressImage.Width, $progressImage.Height)
    $progressGraphics.Dispose()
    $sample = $progressBitmap.GetPixel([int]($progressBitmap.Width / 2), [int]($progressBitmap.Height / 2))
    $progressBitmap.Dispose()
    Set-Content -Path (Join-Path $OutDir 'progress-color.txt') -Encoding ascii `
        -Value ("#{0:X2}{1:X2}{2:X2}" -f $sample.R, $sample.G, $sample.B)

    # Live progress-bar fill: the native NSIS progress bar is hidden and this
    # bitmap is clipped from the left by BrhProgress as the install advances.
    Save-Bmp24 -Image $progressImage -Path (Join-Path $OutDir 'bar-fill.bmp') -Width 225 -Height 16
}
finally {
    $frameImage.Dispose()
    $bannerImage.Dispose()
    $panelImage.Dispose()
    $progressImage.Dispose()
}

Copy-Item $icon (Join-Path $OutDir 'game-icon.ico') -Force

Get-ChildItem $OutDir | Sort-Object Name | ForEach-Object {
    '{0,-24} {1,10} bytes' -f $_.Name, $_.Length
}
