# SDL window play-through for Stage D DoD: menu -> options -> planet -> players ->
# mobilize -> landing -> match -> End Turn, driven by real user32 mouse/keyboard
# events in the actual SDL window, with a screenshot of every step.
$ErrorActionPreference = 'Stop'
$repo = 'D:\dev\max-port-debug'
$out = "$repo\.tmp\sdl"
New-Item -ItemType Directory -Force $out | Out-Null

Add-Type -Namespace Win32 -Name SdlPlay -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, ref POINT p);
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT r);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
public struct POINT { public int x; public int y; }
public struct RECT { public int left; public int top; public int right; public int bottom; }
'@

Add-Type -AssemblyName System.Drawing

function Get-GameWindow {
    Get-Process max-brh -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
}

function Shot([string]$name) {
    $proc = Get-GameWindow
    if (-not $proc) { Write-Host "shot ${name}: no window"; return $false }
    [Win32.SdlPlay]::ShowWindow($proc.MainWindowHandle, 9) | Out-Null
    [Win32.SdlPlay]::SetForegroundWindow($proc.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 300
    $r = New-Object Win32.SdlPlay+RECT
    [Win32.SdlPlay]::GetClientRect($proc.MainWindowHandle, [ref]$r) | Out-Null
    $w = $r.right - $r.left; $h = $r.bottom - $r.top
    $p = New-Object Win32.SdlPlay+POINT; $p.x = 0; $p.y = 0
    [Win32.SdlPlay]::ClientToScreen($proc.MainWindowHandle, [ref]$p) | Out-Null
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($p.x, $p.y, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $bmp.Save("$out\$name.png", [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Host "shot ${name}: ${w}x${h}"
    return $true
}

function Click([int]$cx, [int]$cy) {
    $proc = Get-GameWindow
    if (-not $proc) { return }
    $p = New-Object Win32.SdlPlay+POINT; $p.x = $cx; $p.y = $cy
    [Win32.SdlPlay]::ClientToScreen($proc.MainWindowHandle, [ref]$p) | Out-Null
    [Win32.SdlPlay]::SetCursorPos($p.x, $p.y) | Out-Null
    Start-Sleep -Milliseconds 150
    [Win32.SdlPlay]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [Win32.SdlPlay]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 500
}

function PressEnter {
    [Win32.SdlPlay]::keybd_event(0x0D, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [Win32.SdlPlay]::keybd_event(0x0D, 0, 2, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
}

Get-Process max-brh -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Process -FilePath "$repo\max-brh\target\debug\max-brh.exe" `
    -ArgumentList '--scale', '1', '--data-dir', "$repo\max-original", '--assets-dir', "$repo\max-cpp-port\assets" `
    -WorkingDirectory $repo -RedirectStandardError "$out\stderr.txt" -RedirectStandardOutput "$out\stdout.txt"
Start-Sleep -Seconds 10

$proc = Get-GameWindow
if (-not $proc) { Write-Host 'SDL-PLAY: NO_WINDOW'; exit 1 }
[Win32.SdlPlay]::ShowWindow($proc.MainWindowHandle, 9) | Out-Null
[Win32.SdlPlay]::SetForegroundWindow($proc.MainWindowHandle) | Out-Null
Start-Sleep -Milliseconds 500

Shot '10-main-menu' | Out-Null
Click 485 189            # New Game
Start-Sleep -Seconds 2
Shot '11-options' | Out-Null
Click 568 458            # Done -> Planet
Start-Sleep -Seconds 2
Shot '12-planet' | Out-Null
Click 568 458            # Done -> Players
Start-Sleep -Seconds 2
Shot '13-players' | Out-Null
Click 312 194            # row GREEN -> Computer (idempotent if already Computer)
Start-Sleep -Milliseconds 600
Shot '14-players-green' | Out-Null
Click 568 458            # Done -> Mobilize (CargoMenu)
Start-Sleep -Seconds 3
Shot '15-mobilize' | Out-Null
Click 474 463            # CargoMenu 'Done' (mobilize::DONE_BUTTON centre) -> Landing
Start-Sleep -Seconds 3
Shot '16-landing' | Out-Null
# The landing map area is MAIN_MAP (180,18,448,448) with a 4 px cell: the
# standard RED start (22,22) is (180 + 22*4, 18 + 22*4) = (268,106). A click on a
# valid 4x4 land cell confirms the site (landing.rs::click -> Confirm), exactly as
# in visual-screens.qa (`landing grid 22 22` + `landing done`).
Click 268 106
Start-Sleep -Seconds 4
Shot '17-in-game' | Out-Null
Click 468 178            # select the Surveyor (move-input.qa coordinates)
Start-Sleep -Milliseconds 800
Shot '18-unit-selected' | Out-Null
$p = New-Object Win32.SdlPlay+POINT; $p.x = 532; $p.y = 178
$proc = Get-GameWindow
[Win32.SdlPlay]::ClientToScreen($proc.MainWindowHandle, [ref]$p) | Out-Null
[Win32.SdlPlay]::SetCursorPos($p.x, $p.y) | Out-Null
Start-Sleep -Milliseconds 200
[Win32.SdlPlay]::mouse_event(8, 0, 0, 0, [UIntPtr]::Zero)   # right down: move order
Start-Sleep -Milliseconds 80
[Win32.SdlPlay]::mouse_event(16, 0, 0, 0, [UIntPtr]::Zero)  # right up
Start-Sleep -Milliseconds 800
Shot '19-move-order' | Out-Null
Click 426 12             # End Turn (END_TURN_BUTTON 390,2,72,21 -> centre 426,12)
Start-Sleep -Seconds 8
Shot '20-after-end-turn' | Out-Null
Click 426 12             # End Turn again (turn 2 -> player's turn 3)
Start-Sleep -Seconds 8
Shot '21-turn-3' | Out-Null

Get-Process max-brh -ErrorAction SilentlyContinue | Stop-Process -Force
Write-Host 'SDL-PLAY: DONE'
