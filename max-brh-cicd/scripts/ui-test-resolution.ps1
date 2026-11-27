#requires -version 5.1
# ui-test-resolution.ps1 - LIVE window run: Settings -> Resolutions really changes
# the render scale and rebuilds the window (640x480 / 1280x960 / 2560x1920).
#
# Regression pinned here: `WindowPresentation::apply_resolution` used to rebuild the
# window through `SdlWindow::new_with_render_scale` while the running window was
# still alive. SDL3 keeps only one `EventPump` per process, so the constructor
# failed with "an `EventPump` instance is already alive", the driver logged the
# error and the selected resolution was silently ignored - the window never
# changed. The fix re-opens the window in place
# (`SdlWindow::reopen_with_render_scale`), keeping the SDL context and the event
# pump while the OS window is rebuilt at the new render frame.
#
# Checked in the real window (screenshots in build/ui-test-resolution):
#  * launch at --scale 1: the client area is 640x496 (640x480 frame + 16 px bar),
#    the menu bar is drawn and the main menu is on screen;
#  * Settings -> Resolutions -> 1280x960 (x2): the window is rebuilt (a NEW HWND
#    while the old one is still alive), the client area is 1280x992 and the log
#    reports the hd-2x switch with a 1280x960 render frame;
#  * Settings -> Resolutions -> 640x480 (1:1): the window is rebuilt again, the
#    client area is back to 640x496 and the log reports the pixel-perfect switch
#    with a 640x480 frame - the pixel-perfect presentation is restored (byte
#    equality of the 1:1 frame itself is covered by the golden suite; this test
#    proves that the window switch is really applied).
#
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File max-brh-cicd\scripts\ui-test-resolution.ps1
# Requires a built UI client (build-windows-ui.ps1 -NoPackage, or
# cargo build --release -p max_app --no-default-features --features sdl-vcpkg,sound)
# and an interactive Windows desktop (the screenshots use CopyFromScreen).
param(
    [string]$Exe = "",
    [string]$Out = ""
)

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

public static class MaxResolutionUi {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);

    public static bool IsFocused(IntPtr h) { return GetForegroundWindow() == h; }

    public static int CountColor(string path, int x0, int y0, int w, int h, int r, int g, int b, int tolerance) {
        using (Bitmap bmp = new Bitmap(path)) {
            Rectangle rect = new Rectangle(0, 0, bmp.Width, bmp.Height);
            BitmapData data = bmp.LockBits(rect, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            try {
                byte[] bytes = new byte[data.Stride * bmp.Height];
                Marshal.Copy(data.Scan0, bytes, 0, bytes.Length);
                int count = 0;
                int maxY = Math.Min(y0 + h, bmp.Height);
                int maxX = Math.Min(x0 + w, bmp.Width);
                for (int y = Math.Max(0, y0); y < maxY; y++) {
                    for (int x = Math.Max(0, x0); x < maxX; x++) {
                        int i = y * data.Stride + x * 4;
                        if (Math.Abs(bytes[i] - b) <= tolerance &&
                            Math.Abs(bytes[i + 1] - g) <= tolerance &&
                            Math.Abs(bytes[i + 2] - r) <= tolerance) {
                            count++;
                        }
                    }
                }
                return count;
            } finally { bmp.UnlockBits(data); }
        }
    }
}
'@

$ErrorActionPreference = 'Stop'
$root = 'D:\dev\max-port-debug'
if (-not $Out) { $Out = Join-Path $root 'build\ui-test-resolution' }
New-Item -ItemType Directory -Force $Out | Out-Null
Get-ChildItem $Out -Filter '*.png' -ErrorAction SilentlyContinue | Remove-Item -Force

if (-not $Exe) {
    $candidates = @(
        (Join-Path $root 'build\win-msvc-target\release\max-brh.exe'),
        (Join-Path $root 'bin\target\release\max-brh.exe'),
        (Join-Path $root 'release\max_app-1.1.0-windows-x86_64-ui\max-brh.exe'),
        (Join-Path $root 'bin\release\max_app-1.1.0-windows-x86_64-ui\max-brh.exe')
    )
    $Exe = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $Exe -or -not (Test-Path $Exe)) {
    throw 'ui-test-resolution: no built max-brh.exe (run build-windows-ui.ps1 -NoPackage)'
}

# Menu chrome colors (max_app::window_menu): dropdown/flyout background rgb(28,34,44),
# bar background rgb(18,22,30).
$MENU_BG = @(28, 34, 44)
$BAR_BG = @(18, 22, 30)

$script:stdout = Join-Path $Out 'stdout.log'
$script:stderr = Join-Path $Out 'stderr.log'
Get-Process max-brh -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

$env:MAX_DEBUG_INPUT = '1'
$script:process = Start-Process -FilePath $Exe `
    -ArgumentList '--ui', '--scale', '1', '--data-dir', (Join-Path $root 'max-original') `
    -WorkingDirectory $root -RedirectStandardOutput $script:stdout -RedirectStandardError $script:stderr -PassThru
Remove-Item Env:\MAX_DEBUG_INPUT

function Fail([string]$message) {
    Write-Host "FAIL: $message" -ForegroundColor Red
    exit 1
}

function Read-Text([string]$path) {
    try {
        if (-not (Test-Path $path)) { return '' }
        $stream = [System.IO.File]::Open($path, [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
        try {
            $reader = New-Object System.IO.StreamReader($stream, [System.Text.Encoding]::UTF8)
            try { return $reader.ReadToEnd() } finally { $reader.Dispose() }
        } finally { $stream.Dispose() }
    } catch { return '' }
}

function Wait-Stdout([string]$pattern, [int]$timeoutMs = 8000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        if ((Read-Text $script:stdout) -match $pattern) { return $true }
        Start-Sleep -Milliseconds 120
    }
    return $false
}

function Wait-Window([int]$timeoutMs = 20000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        $script:process.Refresh()
        if ($script:process.HasExited) {
            throw "ui-test-resolution: the client exited (exit=$($script:process.ExitCode)): $(Read-Text $script:stderr)"
        }
        if ($script:process.MainWindowHandle -ne [IntPtr]::Zero) { return $script:process.MainWindowHandle }
        Start-Sleep -Milliseconds 200
    }
    throw 'ui-test-resolution: the window did not appear in 20 s'
}

function Get-Client([IntPtr]$h) {
    $rc = New-Object MaxResolutionUi+RECT
    [void][MaxResolutionUi]::GetClientRect($h, [ref]$rc)
    $origin = New-Object MaxResolutionUi+POINT
    [void][MaxResolutionUi]::ClientToScreen($h, [ref]$origin)
    [pscustomobject]@{ W = $rc.R - $rc.L; H = $rc.B - $rc.T; X = $origin.X; Y = $origin.Y }
}

function Get-Scale([IntPtr]$h) {
    $client = Get-Client $h
    if ($client.H -le 0 -or ($client.H % 496) -ne 0) { return 0 }
    return [int]($client.H / 496)
}

function Ensure-Focus {
    for ($i = 0; $i -lt 12; $i++) {
        [void][MaxResolutionUi]::ShowWindow($script:hwnd, 9)          # SW_RESTORE
        [void][MaxResolutionUi]::SetForegroundWindow($script:hwnd)
        Start-Sleep -Milliseconds 120
        if ([MaxResolutionUi]::IsFocused($script:hwnd)) { return $true }
    }
    return $false
}

# Puts the pointer on a client point right before the click: the desktop must not
# steal the pointer (and the click) between SetCursorPos and mouse_event.
function Set-Pointer([int]$screenX, [int]$screenY) {
    for ($attempt = 0; $attempt -lt 4; $attempt++) {
        [void][MaxResolutionUi]::SetCursorPos($screenX, $screenY)
        Start-Sleep -Milliseconds 60
        $point = New-Object MaxResolutionUi+POINT
        [void][MaxResolutionUi]::GetCursorPos([ref]$point)
        if ([math]::Abs($point.X - $screenX) -le 1 -and [math]::Abs($point.Y - $screenY) -le 1) { return $true }
    }
    return $false
}

# Client coordinates of a logical 640x496 point: the window is opened with --scale 1,
# so at S the client area is exactly 640*S x 496*S and a click is (x*S, y*S).
function Click-Logical([int]$x, [int]$y) {
    if (-not (Ensure-Focus)) { Fail "the game window does not own the focus (click $x,$y)" }
    $scale = Get-Scale $script:hwnd
    if ($scale -lt 1) { Fail "unexpected client area height: $((Get-Client $script:hwnd).H)" }
    $client = Get-Client $script:hwnd
    $cx = $x * $scale; $cy = $y * $scale
    if (-not (Set-Pointer ($client.X + $cx) ($client.Y + $cy))) {
        Fail "the desktop keeps moving the pointer away from ($cx,$cy)"
    }
    [MaxResolutionUi]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)   # LEFTDOWN
    Start-Sleep -Milliseconds 60
    [MaxResolutionUi]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)   # LEFTUP
    Start-Sleep -Milliseconds 500
    Write-Host "click logical=($x,$y) S=$scale client=($cx,$cy)"
}

function Move-Logical([int]$x, [int]$y) {
    Ensure-Focus | Out-Null
    $scale = Get-Scale $script:hwnd
    if ($scale -lt 1) { Fail 'unexpected client area height' }
    $client = Get-Client $script:hwnd
    if (-not (Set-Pointer ($client.X + $x * $scale) ($client.Y + $y * $scale))) {
        Fail "the desktop keeps moving the pointer away from ($x,$y)"
    }
    Start-Sleep -Milliseconds 300
}

function Save-Shot([string]$name) {
    Ensure-Focus | Out-Null
    $client = Get-Client $script:hwnd
    if ($client.W -le 0 -or $client.H -le 0) { Fail "no client area for screenshot $name" }
    $bmp = New-Object System.Drawing.Bitmap $client.W, $client.H
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($client.X, $client.Y, 0, 0, $bmp.Size)
    $g.Dispose()
    $path = Join-Path $Out "$name.png"
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "shot $name ($($client.W)x$($client.H))"
    return $path
}

# The main menu draws its green "Main Menu" caption at frame (236,145) 172x15;
# the menu bar shifts the frame down by BAR_HEIGHT (16 logical pixels).
function Get-MainMenuGreen([string]$shot) {
    return [MaxResolutionUi]::CountColor($shot, 236, 145 + 16, 172, 15, 0, 255, 0, 60)
}

function Get-MenuPanelPixels([string]$shot, [int]$x, [int]$y, [int]$w, [int]$h) {
    return [MaxResolutionUi]::CountColor($shot, $x, $y, $w, $h, $MENU_BG[0], $MENU_BG[1], $MENU_BG[2], 2)
}

# Waits for the rebuilt window: the HWND must change (the new window is created
# before the old one is destroyed, so the handle cannot be reused), and the client
# area must be the new render frame plus the menu bar.
function Wait-RebuiltWindow([IntPtr]$oldHandle, [int]$width, [int]$height, [int]$timeoutMs = 8000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        $script:process.Refresh()
        $handle = $script:process.MainWindowHandle
        if ($handle -ne [IntPtr]::Zero) {
            $client = Get-Client $handle
            if ($handle -ne $oldHandle -and $client.W -eq $width -and $client.H -eq $height) {
                return $handle
            }
        }
        Start-Sleep -Milliseconds 120
    }
    return [IntPtr]::Zero
}

function Save-Logs {
    Copy-Item $script:stdout (Join-Path $Out 'stdout-run.log') -Force -ErrorAction SilentlyContinue
}

try {
    $script:hwnd = Wait-Window
    Start-Sleep -Milliseconds 800
    $launchShot = Save-Shot '00-launch'

    # The client starts on the intro movie when maxint.mve is present. Esc returns
    # to the main menu; the screenshot detects whether the intro is still playing.
    if ((Get-MainMenuGreen $launchShot) -lt 40) {
        if (-not (Ensure-Focus)) { Fail 'the game window does not own the focus' }
        [MaxResolutionUi]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero)        # VK_ESCAPE down
        Start-Sleep -Milliseconds 60
        [MaxResolutionUi]::keybd_event(0x1B, 2, 0, [UIntPtr]::Zero)        # VK_ESCAPE up
        Start-Sleep -Milliseconds 1500
    }

    $client0 = Get-Client $script:hwnd
    if ($client0.W -ne 640 -or $client0.H -ne 496) {
        Fail "the launch window must be 640x496 (640x480 frame + 16 px bar), got $($client0.W)x$($client0.H)"
    }
    $shot0 = Save-Shot '01-main-menu-pixelperfect'
    $green = Get-MainMenuGreen $shot0
    Write-Host "A: pixel-perfect start, client 640x496, main-menu green pixels: $green"
    if ($green -lt 40) { Fail 'the main menu is not on screen (no green Main Menu caption)' }

    $bar = [MaxResolutionUi]::CountColor($shot0, 0, 0, 640, 16, $BAR_BG[0], $BAR_BG[1], $BAR_BG[2], 2)
    Write-Host "B: menu bar background pixels: $bar"
    if ($bar -lt 1000) { Fail 'the menu bar is not drawn (no BAR_BACKGROUND in the top 16 px)' }

    # ---- Settings -> Resolutions -> 1280x960 (x2), pixel-perfect ----
    # The desktop may move the pointer between frames: every stage is verified on a
    # screenshot and retried instead of trusting a single click.
    $opened = $false
    for ($attempt = 0; $attempt -lt 3 -and -not $opened; $attempt++) {
        Click-Logical 69 8
        $shot = Save-Shot '02-settings-dropdown'
        $dropdown = Get-MenuPanelPixels $shot 36 16 124 16
        Write-Host "C: Settings dropdown panel pixels: $dropdown"
        $opened = $dropdown -gt 500
    }
    if (-not $opened) { Fail 'Settings -> the dropdown did not open' }

    $flyout = $false
    for ($attempt = 0; $attempt -lt 3 -and -not $flyout; $attempt++) {
        Move-Logical 98 24
        $shot = Save-Shot '03-flyout-pixelperfect'
        $panel = Get-MenuPanelPixels $shot 160 17 156 44
        Write-Host "D: Resolutions flyout panel pixels: $panel"
        $flyout = $panel -gt 1000
    }
    if (-not $flyout) { Fail 'Settings -> Resolutions -> the flyout did not open' }

    $oldHandle = $script:hwnd
    Click-Logical 238 39
    $newHandle = Wait-RebuiltWindow $oldHandle 1280 992
    if ($newHandle -eq [IntPtr]::Zero) {
        Fail "1280x960 did not rebuild the window to 1280x992 (client $((Get-Client $script:hwnd).W)x$((Get-Client $script:hwnd).H))"
    }
    $script:hwnd = $newHandle
    Write-Host "E: window rebuilt: hwnd $oldHandle -> $newHandle, client 1280x992"
    if (-not (Wait-Stdout 'hd-2x' 3000)) { Fail 'the log has no hd-2x resolution switch' }
    if (-not (Wait-Stdout '1280x960' 3000)) { Fail 'the log has no 1280x960 render frame' }
    $shot1 = Save-Shot '04-hd2-1280x960'
    $bar2 = [MaxResolutionUi]::CountColor($shot1, 0, 0, 1280, 32, $BAR_BG[0], $BAR_BG[1], $BAR_BG[2], 2)
    Write-Host "F: x2 menu bar background pixels: $bar2"
    if ($bar2 -lt 4000) { Fail 'the menu bar was not redrawn at x2' }

    # ---- Settings -> Resolutions -> 640x480 (1:1), back from x2 ----
    $opened = $false
    for ($attempt = 0; $attempt -lt 3 -and -not $opened; $attempt++) {
        Click-Logical 69 8
        $shot = Save-Shot '05-settings-dropdown-hd2'
        $dropdown = Get-MenuPanelPixels $shot 72 32 248 32
        Write-Host "G: x2 Settings dropdown panel pixels: $dropdown"
        $opened = $dropdown -gt 2000
    }
    if (-not $opened) { Fail 'Settings at x2 -> the dropdown did not open' }

    $flyout = $false
    for ($attempt = 0; $attempt -lt 3 -and -not $flyout; $attempt++) {
        Move-Logical 98 24
        $shot = Save-Shot '06-flyout-hd2'
        $panel = Get-MenuPanelPixels $shot 320 34 312 88
        Write-Host "H: x2 Resolutions flyout panel pixels: $panel"
        $flyout = $panel -gt 4000
    }
    if (-not $flyout) { Fail 'Settings -> Resolutions at x2 -> the flyout did not open' }

    $oldHandle = $script:hwnd
    Click-Logical 238 25
    $newHandle = Wait-RebuiltWindow $oldHandle 640 496
    if ($newHandle -eq [IntPtr]::Zero) {
        Fail "640x480 did not rebuild the window back to 640x496 (client $((Get-Client $script:hwnd).W)x$((Get-Client $script:hwnd).H))"
    }
    $script:hwnd = $newHandle
    Write-Host "I: window rebuilt back: hwnd $oldHandle -> $newHandle, client 640x496"
    if (-not (Wait-Stdout 'pixel-perfect' 3000)) { Fail 'the log has no pixel-perfect resolution switch' }
    $shot2 = Save-Shot '07-back-to-pixelperfect'
    $barAgain = [MaxResolutionUi]::CountColor($shot2, 0, 0, 640, 16, $BAR_BG[0], $BAR_BG[1], $BAR_BG[2], 2)
    $greenAgain = Get-MainMenuGreen $shot2
    Write-Host "J: back at 1:1, menu bar background pixels: $barAgain, main-menu green pixels: $greenAgain"
    if ($barAgain -lt 1000) { Fail 'the menu bar did not return to 1:1' }
    if ($greenAgain -lt 40) { Fail 'the main menu is not rendered after returning to 1:1' }

    Save-Logs
    Write-Host 'PASS: Settings -> Resolutions switches 640x496 -> 1280x992 (frame 1280x960) -> 640x496'
}
finally {
    Save-Logs
    Get-ChildItem $Out -Filter 'tmp-*.png' -ErrorAction SilentlyContinue | Remove-Item -Force
    if (-not $script:process.HasExited) { Stop-Process -Id $script:process.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 300
    Write-Host "EXITED: $($script:process.HasExited)"
}
