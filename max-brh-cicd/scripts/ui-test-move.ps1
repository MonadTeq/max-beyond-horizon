# UI test of unit movement in the LIVE window (SDL3): the "unit does not move" regression.
#
#   powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\ui-test-move.ps1
#
# The same mouse path as the player's: New Game → Options Done → Planet Done →
# Players (Green = Computer) Done → mobilization Done → landing Enter → in game:
# click your own Surveyor (the portrait in the panel), RMB on a neighboring cell
# (green preview), End Turn → the unit is in the new cell.
#
# Coordinates are for a 640×480 frame (window --scale 1). After the camera lock
# (`GameManager_GetFirstRelevantUnit` → the mine (22,22) in the center of the map area)
# the Surveyor stands at (23,21) → pixel (468,178), the neighboring cell (24,21) →
# (532,178), the End Turn button is near (426,12).
#
# Screenshots: build/ui-test-move/*.png. The script exits with code 1 if
# the portrait is not filled after the click or the preview is not drawn.

param(
    [string]$Exe = "",
    [string]$Out = ""
)

$ErrorActionPreference = 'Stop'

# max-brh-cicd\scripts -> <repo>
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Out) { $Out = Join-Path $root "build\ui-test-move" }
New-Item -ItemType Directory -Force $Out | Out-Null

if (-not $Exe) {
    $bundle = Get-ChildItem (Join-Path $root "release") -Directory -Filter "max_app-*-windows-x86_64-ui" |
        Sort-Object Name -Descending | Select-Object -First 1
    if (-not $bundle) { Write-Host "ui-test-move: нет bin/release/max_app-*-windows-x86_64-ui"; exit 1 }
    $Exe = Join-Path $bundle.FullName "max-brh.exe"
}
if (-not (Test-Path $Exe)) { Write-Host "ui-test-move: нет $Exe"; exit 1 }

Add-Type -Namespace Win32 -Name Mouse -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, ref POINT p);
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT r);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
public struct POINT { public int x; public int y; }
public struct RECT { public int left; public int top; public int right; public int bottom; }
'@

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

function Click-Client([IntPtr]$hwnd, [int]$cx, [int]$cy) {
    $p = New-Object Win32.Mouse+POINT; $p.x = $cx; $p.y = $cy
    [Win32.Mouse]::ClientToScreen($hwnd, [ref]$p) | Out-Null
    [Win32.Mouse]::SetCursorPos($p.x, $p.y) | Out-Null
    Start-Sleep -Milliseconds 120
    [Win32.Mouse]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [Win32.Mouse]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 300
}

function RightClick-Client([IntPtr]$hwnd, [int]$cx, [int]$cy) {
    $p = New-Object Win32.Mouse+POINT; $p.x = $cx; $p.y = $cy
    [Win32.Mouse]::ClientToScreen($hwnd, [ref]$p) | Out-Null
    [Win32.Mouse]::SetCursorPos($p.x, $p.y) | Out-Null
    Start-Sleep -Milliseconds 120
    [Win32.Mouse]::mouse_event(8, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [Win32.Mouse]::mouse_event(16, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 300
}

function Get-Window([string]$title) {
    $proc = Get-Process max-brh -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $proc) { return $null }
    [Win32.Mouse]::ShowWindow($proc.MainWindowHandle, 9) | Out-Null
    [Win32.Mouse]::SetForegroundWindow($proc.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 250
    return $proc
}

function Shot([string]$name) {
    $proc = Get-Window 'max-brh'
    if (-not $proc) { Write-Host "shot ${name}: нет окна"; return $null }
    $r = New-Object Win32.Mouse+RECT
    [Win32.Mouse]::GetClientRect($proc.MainWindowHandle, [ref]$r) | Out-Null
    $w = $r.right - $r.left; $h = $r.bottom - $r.top
    $p = New-Object Win32.Mouse+POINT; $p.x = 0; $p.y = 0
    [Win32.Mouse]::ClientToScreen($proc.MainWindowHandle, [ref]$p) | Out-Null
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($p.x, $p.y, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $path = Join-Path $Out "$name.png"
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Host "shot $name ($w x $h)"
    return $path
}

# Counts "non-empty" pixels in a rectangle (brightness > 24) — portrait/panel.
function Count-Lit([string]$path, [int]$x, [int]$y, [int]$w, [int]$h) {
    $bmp = New-Object System.Drawing.Bitmap($path)
    $lit = 0
    for ($j = $y; $j -lt [Math]::Min($y + $h, $bmp.Height); $j += 2) {
        for ($i = $x; $i -lt [Math]::Min($x + $w, $bmp.Width); $i += 2) {
            $c = $bmp.GetPixel($i, $j)
            if (($c.R + $c.G + $c.B) -gt 72) { ++$lit }
        }
    }
    $bmp.Dispose()
    return $lit
}

# Green route preview (HUD PREVIEW_COLOR = 120,255,152).
function Count-Preview([string]$path) {
    $bmp = New-Object System.Drawing.Bitmap($path)
    $count = 0
    for ($j = 0; $j -lt $bmp.Height; ++$j) {
        for ($i = 0; $i -lt $bmp.Width; ++$i) {
            $c = $bmp.GetPixel($i, $j)
            if ([Math]::Abs($c.R - 120) -le 6 -and $c.G -ge 240 -and [Math]::Abs($c.B - 152) -le 6) {
                ++$count
            }
        }
    }
    $bmp.Dispose()
    return $count
}

Get-Process max-brh -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Process -FilePath $Exe -ArgumentList '--scale','1','--data-dir',"$root\max-original",'--assets-dir',"$root\assets" -WorkingDirectory $root
Start-Sleep -Seconds 6
$proc = Get-Window 'max-brh'
if (-not $proc) { Write-Host 'ui-test-move: окно не появилось'; exit 1 }
$hwnd = $proc.MainWindowHandle

Shot '01-main-menu' | Out-Null
Click-Client $hwnd 485 189        # New Game
Start-Sleep -Seconds 2
Shot '02-options' | Out-Null
Click-Client $hwnd 568 458        # Options: Done
Start-Sleep -Seconds 2
Shot '03-planet' | Out-Null
Click-Client $hwnd 568 458        # Planet: Done
Start-Sleep -Seconds 2
Click-Client $hwnd 312 194        # Players: GREEN = Computer
Start-Sleep -Milliseconds 400
Shot '04-players' | Out-Null
Click-Client $hwnd 568 458        # Players: Done -> Mobilize
Start-Sleep -Seconds 2
Shot '05-mobilize' | Out-Null
Click-Client $hwnd 474 463        # Mobilize: Done (447,452,54x23)
Start-Sleep -Seconds 2
Shot '06-landing' | Out-Null

# Landing: Enter confirms the default point (cursor = (22,22)).
[Win32.Mouse]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 300
[System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
Start-Sleep -Seconds 2
$start = Shot '07-game-start'

$portraitBefore = Count-Lit $start 10 29 127 127

# Click your own Surveyor (23,21) -> (468,178).
Click-Client $hwnd 468 178
Start-Sleep -Milliseconds 700
$selected = Shot '08-selected'
$portraitAfter = Count-Lit $selected 10 29 127 127

if ($portraitAfter -le ($portraitBefore + 40)) {
    Write-Host "ui-test-move: ПРОВАЛ — портрет не заполнился (было $portraitBefore, стало $portraitAfter)"
    exit 1
}

# RMB on the neighboring cell (24,21) -> (532,178): a green route.
RightClick-Client $hwnd 532 178
Start-Sleep -Milliseconds 500
$preview = Shot '09-move-preview'
$green = Count-Preview $preview
if ($green -lt 20) {
    Write-Host "ui-test-move: ПРОВАЛ — нет зелёного предпросмотра ($green пикселей)"
    exit 1
}

# End Turn -> AI turn -> a new round.
Click-Client $hwnd 426 12
Start-Sleep -Seconds 5
Shot '10-after-turn' | Out-Null

# The unit moved to (24,21): we select it on the new cell.
Click-Client $hwnd 532 178
Start-Sleep -Milliseconds 700
$moved = Shot '11-unit-moved'
$portraitMoved = Count-Lit $moved 10 29 127 127
if ($portraitMoved -le 40) {
    Write-Host "ui-test-move: ПРОВАЛ — юнит не выделяется в новой клетке ($portraitMoved)"
    exit 1
}

Write-Host "ui-test-move: OK (портрет $portraitBefore -> $portraitAfter, предпросмотр $green px, после хода $portraitMoved)"
exit 0
