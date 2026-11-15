# UI-тест одиночной игры: главное меню -> New Game -> Options/Planet/Players -> партия с ИИ.
$ErrorActionPreference = 'Stop'
$root = 'D:\dev\max-port-debug'
$out = "$root\build\ui-test-sp"
New-Item -ItemType Directory -Force $out | Out-Null

Add-Type -Namespace Win32 -Name Mouse -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, ref POINT p);
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT r);
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

function Shot([string]$name) {
    $proc = Get-Process max_app -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $proc) { Write-Host "shot ${name}: no window"; return }
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

Get-Process max_app -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
Start-Process -FilePath "$root\release\max_app-1.0.0-windows-x86_64-ui\max_app.exe" -ArgumentList '--ui','--scale','1','--data-dir',"$root\max-original",'--assets-dir',"$root\assets" -WorkingDirectory $root
Start-Sleep -Seconds 6
$proc = Get-Process max_app | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $proc) { Write-Host 'window not found'; exit 1 }
$hwnd = $proc.MainWindowHandle

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
# Выделение юнита около центра карты и приказ движения
Click-Client $hwnd 404 242
Start-Sleep -Seconds 1
Shot '07-selected'
Click-Client $hwnd 468 242    # RMB: move
Start-Sleep -Seconds 1
Shot '08-move-order'
# End Turn -> ход ИИ -> снова ход игрока
Click-Client $hwnd 426 12
Start-Sleep -Seconds 6
Shot '09-after-ai-turn'
Write-Host 'DONE'
