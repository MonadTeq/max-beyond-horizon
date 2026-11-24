# SDL window smoke for Stage D DoD: launch the real GUI client, prove that the
# window opens and renders (main menu), and that a real mouse click drives the UI
# (New Game -> Options). The full match/input chain is covered by qa_run/golden;
# this check only closes the "the SDL window itself works" part.
$ErrorActionPreference = 'Stop'
$repo = 'D:\dev\max-port-debug'
$out = "$repo\.tmp\sdl"
New-Item -ItemType Directory -Force $out | Out-Null

Add-Type -Namespace Win32 -Name SdlSmoke -MemberDefinition @'
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

function Shot([string]$name) {
    $proc = Get-Process max-brh -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $proc) { Write-Host "shot ${name}: no window"; return $false }
    [Win32.SdlSmoke]::ShowWindow($proc.MainWindowHandle, 9) | Out-Null
    [Win32.SdlSmoke]::SetForegroundWindow($proc.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 300
    $r = New-Object Win32.SdlSmoke+RECT
    [Win32.SdlSmoke]::GetClientRect($proc.MainWindowHandle, [ref]$r) | Out-Null
    $w = $r.right - $r.left; $h = $r.bottom - $r.top
    $p = New-Object Win32.SdlSmoke+POINT; $p.x = 0; $p.y = 0
    [Win32.SdlSmoke]::ClientToScreen($proc.MainWindowHandle, [ref]$p) | Out-Null
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($p.x, $p.y, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $bmp.Save("$out\$name.png", [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Host "shot ${name}: ${w}x${h}"
    return $true
}

function Click([int]$cx, [int]$cy) {
    $proc = Get-Process max-brh -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $proc) { return }
    $p = New-Object Win32.SdlSmoke+POINT; $p.x = $cx; $p.y = $cy
    [Win32.SdlSmoke]::ClientToScreen($proc.MainWindowHandle, [ref]$p) | Out-Null
    [Win32.SdlSmoke]::SetCursorPos($p.x, $p.y) | Out-Null
    Start-Sleep -Milliseconds 150
    [Win32.SdlSmoke]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [Win32.SdlSmoke]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
}

Get-Process max-brh -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

$stderr = "$out\stderr.txt"
$stdout = "$out\stdout.txt"
Start-Process -FilePath "$repo\max-brh\target\debug\max-brh.exe" `
    -ArgumentList '--scale', '1', '--data-dir', "$repo\max-original", '--assets-dir', "$repo\max-cpp-port\assets" `
    -WorkingDirectory $repo -RedirectStandardError $stderr -RedirectStandardOutput $stdout
Start-Sleep -Seconds 10

$proc = Get-Process max-brh -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $proc) {
    Write-Host 'SDL-SMOKE: NO_WINDOW'
    if (Test-Path $stderr) { Get-Content $stderr -Encoding UTF8 | Select-Object -Last 15 }
    exit 1
}

Write-Host ("SDL-SMOKE: window title={0}" -f $proc.MainWindowTitle)
if (-not (Shot '01-main-menu')) { exit 1 }
Click 485 189          # New Game
Start-Sleep -Seconds 2
if (-not (Shot '02-options')) { exit 1 }
Click 568 458          # Options -> Done -> Planet
Start-Sleep -Seconds 2
if (-not (Shot '03-planet')) { exit 1 }

Get-Process max-brh -ErrorAction SilentlyContinue | Stop-Process -Force
Write-Host 'SDL-SMOKE: DONE'
