# UI test of a single game: main menu -> New Game -> Options/Planet/Players -> a match with AI.
$ErrorActionPreference = 'Stop'
$root = 'D:\dev\max-port-debug'
$out = "$root\build\ui-test-sp"
New-Item -ItemType Directory -Force $out | Out-Null

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
    [Win32.Mouse]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)   # down
    Start-Sleep -Milliseconds 60
    [Win32.Mouse]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)   # up
    Start-Sleep -Milliseconds 250
}

function RightClick-Client([IntPtr]$hwnd, [int]$cx, [int]$cy) {
    $p = New-Object Win32.Mouse+POINT; $p.x = $cx; $p.y = $cy
    [Win32.Mouse]::ClientToScreen($hwnd, [ref]$p) | Out-Null
    [Win32.Mouse]::SetCursorPos($p.x, $p.y) | Out-Null
    Start-Sleep -Milliseconds 120
    [Win32.Mouse]::mouse_event(8, 0, 0, 0, [UIntPtr]::Zero)   # right down
    Start-Sleep -Milliseconds 60
    [Win32.Mouse]::mouse_event(16, 0, 0, 0, [UIntPtr]::Zero)  # right up
    Start-Sleep -Milliseconds 250
}

function Shot([string]$name) {
    $proc = Get-Process max-brh -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $proc) { Write-Host "shot ${name}: no window"; return }
    # The screenshot is taken from the screen: the window must be on top, otherwise the frame
    # catches the browser/terminal and the clicks go outside the game.
    [Win32.Mouse]::ShowWindow($proc.MainWindowHandle, 9) | Out-Null
    [Win32.Mouse]::SetForegroundWindow($proc.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 250
    $r = New-Object Win32.Mouse+RECT
    [Win32.Mouse]::GetClientRect($proc.MainWindowHandle, [ref]$r) | Out-Null
    $w = $r.right - $r.left; $h = $r.bottom - $r.top
    $p = New-Object Win32.Mouse+POINT; $p.x = 0; $p.y = 0
    [Win32.Mouse]::ClientToScreen($proc.MainWindowHandle, [ref]$p) | Out-Null
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($p.x, $p.y, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $bmp.Save("$out\$name.png", [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Host "shot $name ($w x $h)"
}

Get-Process max-brh -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
# Without --ui: the client opens the GUI (SDL3 window) by default.
Start-Process -FilePath "$root\release\max_app-1.0.0-windows-x86_64-ui\max-brh.exe" -ArgumentList '--scale','1','--data-dir',"$root\max-original",'--assets-dir',"$root\assets" -WorkingDirectory $root
Start-Sleep -Seconds 6
$proc = Get-Process max-brh | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $proc) { Write-Host 'window not found'; exit 1 }
$hwnd = $proc.MainWindowHandle
[Win32.Mouse]::ShowWindow($hwnd, 9) | Out-Null
[Win32.Mouse]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Seconds 1

Shot '01-main-menu'
Click-Client $hwnd 485 189    # New Game
Start-Sleep -Seconds 2
Shot '02-options'
Click-Client $hwnd 568 458    # Done
Start-Sleep -Seconds 2
Shot '03-planet'
Click-Client $hwnd 568 458    # Done
Start-Sleep -Seconds 2
Shot '04-players'
Click-Client $hwnd 312 194    # row 1 (Green) -> Computer
Start-Sleep -Seconds 1
Shot '05-players-green-computer'
Click-Client $hwnd 568 458    # Done -> start
Start-Sleep -Seconds 4
Shot '06-game-start'
# Select the Surveyor (the top-right mobile unit) and order it to move down.
# Coordinates as in the build/ui-test-sp/06-game-start.png frame (640×480).
Click-Client $hwnd 532 114
Start-Sleep -Seconds 1
Shot '07-selected'
RightClick-Client $hwnd 540 350    # RMB: move
Start-Sleep -Seconds 1
Shot '08-move-order'
# End Turn -> AI turn -> the player's turn again
Click-Client $hwnd 426 12
Start-Sleep -Seconds 6
Shot '09-after-ai-turn'
Write-Host 'DONE'
