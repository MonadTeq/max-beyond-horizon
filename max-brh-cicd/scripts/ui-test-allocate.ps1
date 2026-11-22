# ui-test-allocate.ps1 — LIVE run of the M.A.X. Rust client window:
# mine output allocation (C++ `AllocMenu`, window `ALLOCFRM`), TC-0627–0629.
#
# What is checked in the real window (clicks via SendInput, focus is verified):
#  * the mine command menu's `allocate` item is **enabled** and opens the dialog
#    (`alloc=open` in the `MAX_DEBUG_INPUT` trace);
#  * the dialog is full-screen: the window frame shows the green `Allocation Menu` title
#    and green section labels (`COLOR_GREEN` = 0x00FF00);
#  * modality: a click on the map neither closes the dialog nor issues an order;
#  * the `RTARO_OF` arrow changes the value, `Done` closes the window (`alloc=closed`)
#    and returns to the game;
#  * frames `00-menu` … `06-alloc-done` go to `build/ui-test-allocate/`.
#
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File max-brh-cicd\scripts\ui-test-allocate.ps1
#
# A built client is required:
#   powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-ui.ps1 -NoPackage
param(
    [string]$Out = "",
    [switch]$KeepProcess
)

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Threading;

public static class MaxUi {
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x; public int y; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT {
        public int dx; public int dy; public uint mouseData; public uint dwFlags;
        public uint time; public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT {
        public ushort wVk; public ushort wScan; public uint dwFlags;
        public uint time; public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Explicit)] public struct INPUT {
        [FieldOffset(0)] public uint type;
        [FieldOffset(8)] public MOUSEINPUT mi;
        [FieldOffset(8)] public KEYBDINPUT ki;
    }

    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, ref POINT p);
    [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr hWnd, ref POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll", SetLastError = true)] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);

    const uint LEFT_DOWN = 0x0002, LEFT_UP = 0x0004, RIGHT_DOWN = 0x0008, RIGHT_UP = 0x0010;
    const uint KEYEVENTF_KEYUP = 0x0002;

    public static bool Click(IntPtr hwnd, int x, int y, bool right) {
        POINT p = new POINT(); p.x = x; p.y = y;
        ClientToScreen(hwnd, ref p);
        SetCursorPos(p.x, p.y);
        Thread.Sleep(150);

        INPUT[] a = new INPUT[2];
        a[0].type = 0;
        a[0].mi.dwFlags = right ? RIGHT_DOWN : LEFT_DOWN;
        a[1].type = 0;
        a[1].mi.dwFlags = right ? RIGHT_UP : LEFT_UP;
        uint sent = SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
        Thread.Sleep(120);
        return sent == 2;
    }

    public static bool PressEnter() {
        INPUT[] a = new INPUT[2];
        a[0].type = 1; a[0].ki.wVk = 0x0D;
        a[1].type = 1; a[1].ki.wVk = 0x0D; a[1].ki.dwFlags = KEYEVENTF_KEYUP;
        uint sent = SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
        Thread.Sleep(120);
        return sent == 2;
    }

    public static bool IsFocused(IntPtr hwnd) {
        return GetForegroundWindow() == hwnd;
    }

    // Количество пикселей заданного цвета в прямоугольнике. Пиксели PNG
    // читаются как BGRA.
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

# --- scenario: mine output allocation (AllocMenu, ALLOCFRM) -----------------
$ErrorActionPreference = 'Stop'
$root = 'D:\dev\max-port-debug'
if (-not $Out) { $Out = Join-Path $root 'build\ui-test-allocate' }
New-Item -ItemType Directory -Force $Out | Out-Null
Get-ChildItem $Out -Filter '*.png' -ErrorAction SilentlyContinue | Remove-Item -Force

# Color of one frame pixel — to tell "the bar is filled" from "the bar is empty".
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System.Drawing;
public static class MaxUiPixel {
    public static int At(string path, int x, int y) {
        using (Bitmap bmp = new Bitmap(path)) {
            Color c = bmp.GetPixel(x, y);
            return (c.R << 16) | (c.G << 8) | c.B;
        }
    }
}
'@

$exeCandidates = @(
    (Join-Path $root 'build\win-msvc-target\release\max-brh.exe'),
    (Join-Path $root 'release\max_app-1.1.0-windows-x86_64-ui\max-brh.exe')
)
$exe = $exeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) { throw "ui-test-allocate: нет собранного max-brh.exe (build-windows-ui.ps1 -NoPackage)" }

$env:MAX_DEBUG_INPUT = '1'
$script:trace = Join-Path $Out 'trace.log'

Get-Process max-brh -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

$process = Start-Process -FilePath $exe `
    -ArgumentList '--scale', '1', '--data-dir', (Join-Path $root 'max-original'), `
        '--assets-dir', (Join-Path $root 'assets') `
    -WorkingDirectory $root -RedirectStandardError $script:trace -PassThru

function Read-Trace {
    try {
        if (-not (Test-Path $script:trace)) { return '' }
        $stream = [System.IO.File]::Open(
            $script:trace,
            [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::Read,
            [System.IO.FileShare]::ReadWrite)
        try {
            $reader = New-Object System.IO.StreamReader($stream, [System.Text.Encoding]::UTF8)
            try { return $reader.ReadToEnd() } finally { $reader.Dispose() }
        } finally { $stream.Dispose() }
    } catch { return '' }
}

function Wait-Trace([string]$pattern, [int]$timeoutMs = 8000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        if ((Read-Trace) -match $pattern) { return $true }
        Start-Sleep -Milliseconds 120
    }
    return $false
}

function Wait-Window([int]$timeoutMs = 20000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        $process.Refresh()
        if ($process.HasExited) { throw "окно: процесс завершился (exit=$($process.ExitCode))" }
        if ($process.MainWindowHandle -ne 0) { return $process.MainWindowHandle }
        Start-Sleep -Milliseconds 200
    }
    throw 'окно не появилось за 20 с'
}

function Ensure-Focus {
    for ($i = 0; $i -lt 12; $i++) {
        [MaxUi]::ShowWindow($script:hwnd, 9) | Out-Null
        [MaxUi]::SetForegroundWindow($script:hwnd) | Out-Null
        Start-Sleep -Milliseconds 120
        if ([MaxUi]::IsFocused($script:hwnd)) { return $true }
    }
    return $false
}

function Shot([string]$name) {
    Ensure-Focus | Out-Null
    $p0 = New-Object MaxUi+POINT
    $p0.x = 0; $p0.y = 0
    [MaxUi]::ClientToScreen($script:hwnd, [ref]$p0) | Out-Null
    $bmp = New-Object System.Drawing.Bitmap(640, 480)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($p0.x, $p0.y, 0, 0, (New-Object System.Drawing.Size(640, 480)))
    $path = Join-Path $Out "$name.png"
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    return $path
}

function Click-Client([int]$cx, [int]$cy, [bool]$right = $false) {
    if (-not (Ensure-Focus)) {
        throw "фокус не на игровом окне: клик ($cx,$cy) не будет доставлен"
    }
    [MaxUi]::Click($script:hwnd, $cx, $cy, $right) | Out-Null
}

function Find-Line([string]$text, [string]$pattern) {
    @($text -split "`r?`n" | Where-Object { $_ -match $pattern }) | Select-Object -Last 1
}

# Moves the cursor without a click: the app receives `MouseMove`, and the trace
# prints the modal window state at the moment of the event. So `alloc=open`/
# `alloc=closed` becomes visible right after the action (the `[in]` line is written BEFORE the
# input is applied, so right after the click it still shows the previous state).
function Move-Client([int]$cx, [int]$cy) {
    Ensure-Focus | Out-Null
    $p = New-Object MaxUi+POINT
    $p.x = $cx; $p.y = $cy
    [MaxUi]::ClientToScreen($script:hwnd, [ref]$p) | Out-Null
    [MaxUi]::SetCursorPos($p.x, $p.y) | Out-Null
    Start-Sleep -Milliseconds 350
}

function Get-LastTick([string]$text) {
    $lines = @($text -split "`r?`n" | Where-Object { $_ -match '^\[tick ' })
    if ($lines.Count -eq 0) { return $null }
    return $lines[-1]
}

$failures = New-Object System.Collections.Generic.List[string]
function Fail([string]$message) {
    $failures.Add($message) | Out-Null
    Write-Host "FAIL: $message" -ForegroundColor Red
}
function Step([string]$message) { Write-Host "== $message ==" -ForegroundColor Cyan }

try {
    Step 'окно и меню -> партия'
    $script:hwnd = Wait-Window
    if (-not (Ensure-Focus)) { Fail 'окно не получает фокус перед стартом' }
    Shot '00-menu' | Out-Null

    Click-Client 485 189; Start-Sleep -Seconds 1        # New Game
    Click-Client 568 458; Start-Sleep -Milliseconds 800 # Done (Options)
    Click-Client 568 458; Start-Sleep -Milliseconds 800 # Done (Planet)
    Click-Client 568 458; Start-Sleep -Milliseconds 800 # Done (Players)
    Click-Client 474 463; Start-Sleep -Milliseconds 800 # Done (CargoMenu)

    Ensure-Focus | Out-Null
    [MaxUi]::PressEnter() | Out-Null
    Start-Sleep -Seconds 1

    if (-not (Wait-Trace '(?m)^\[tick ' 20000)) {
        Fail 'партия не началась: нет тиков InSession'
        throw 'меню не пройдено'
    }
    Shot '01-game' | Out-Null

    # After the landing the camera is centered on the mine (`focus_player`), so
    # the screen coordinates of the cell/menu are constant (the same way
    # `App::open_unit_menu` computes them: anchor `unit->x + 32`, items 24 px apart).
    $centerX = 404; $centerY = 242

    Step 'LMB x2: выделение шахты и меню команд'
    Click-Client $centerX $centerY
    Start-Sleep -Milliseconds 600
    Click-Client $centerX $centerY
    Start-Sleep -Milliseconds 600
    Shot '02-unit-menu' | Out-Null

    $clickLine = Find-Line (Read-Trace) '^\[click\]'
    if (-not $clickLine) { Fail 'клик по шахте не дошёл до окна' }
    Write-Host "A: $clickLine"

    Step 'пункт allocate -> диалог ALLOCFRM'
    # 4 items (`allocate`, `x-fer`, `stop`, `remove`), window 48x99, allocate
    # first (position character '1'): button center (460, 206).
    Click-Client 460 206
    Start-Sleep -Milliseconds 900
    Shot '03-alloc-menu' | Out-Null

    $greenTitle = [MaxUi]::CountColor((Join-Path $Out '03-alloc-menu.png'), 230, 6, 180, 20, 0, 255, 0, 40)
    Write-Host "B: green title pixels: $greenTitle"
    if ($greenTitle -lt 30) { Fail "заголовок Allocation Menu не нарисован (зелёных пикселей: $greenTitle)" }

    Step 'модальность: клик по карте не закрывает диалог'
    Click-Client 320 300
    Start-Sleep -Milliseconds 500
    Move-Client 400 400
    $lastInput = Find-Line (Read-Trace) '^\[in\] '
    Write-Host "C: $lastInput"
    if ($lastInput -notmatch 'alloc=open') { Fail "клик по карте закрыл диалог: $lastInput" }
    Shot '04-alloc-modal' | Out-Null

    Step 'LFARO_OF: -1 единица сырья (заливка полосы короче)'
    Shot '05-alloc-before' | Out-Null
    $probeBefore = [MaxUiPixel]::At((Join-Path $Out '05-alloc-before.png'), 410, 85)

    Click-Client 153 84
    Start-Sleep -Milliseconds 600
    Move-Client 400 420
    Shot '06-alloc-minus' | Out-Null
    $probeAfter = [MaxUiPixel]::At((Join-Path $Out '06-alloc-minus.png'), 410, 85)
    Write-Host ("D: probe before=0x{0:X6} after-minus=0x{1:X6}" -f $probeBefore, $probeAfter)
    if ($probeBefore -eq $probeAfter) {
        Fail 'левая стрелка LFARO_OF не изменила заливку полосы Raw'
    }
    $lastInput = Find-Line (Read-Trace) '^\[in\] '
    if ($lastInput -notmatch 'alloc=open') { Fail "после клика по стрелке диалог закрылся: $lastInput" }

    $greenLabels = [MaxUi]::CountColor((Join-Path $Out '06-alloc-minus.png'), 42, 70, 78, 100, 0, 200, 0, 60)
    Write-Host "D: green label pixels: $greenLabels"
    if ($greenLabels -lt 30) { Fail "подписи секций не нарисованы (зелёных пикселей: $greenLabels)" }

    # Return the value with the right arrow (RTARO_OF): the bar is full again.
    Click-Client 435 84
    Start-Sleep -Milliseconds 600
    Move-Client 400 430
    Shot '07-alloc-restored' | Out-Null
    $probeRestored = [MaxUiPixel]::At((Join-Path $Out '07-alloc-restored.png'), 410, 85)
    Write-Host ("D: probe restored=0x{0:X6}" -f $probeRestored)
    if ($probeRestored -ne $probeBefore) {
        Fail 'правая стрелка RTARO_OF не восстановила заливку полосы Raw'
    }

    Step 'Done: закрыть диалог'
    Click-Client 568 458
    Start-Sleep -Milliseconds 900
    Move-Client 400 440
    Shot '07-alloc-done' | Out-Null

    $lastInput = Find-Line (Read-Trace) '^\[in\] '
    if ($lastInput -notmatch 'alloc=closed') {
        Fail "Done не закрыл диалог: последняя строка ввода «$lastInput»"
    }
    if (-not (Wait-Trace '(?m)^\[tick ' 5000)) { Fail 'после Done игра не идёт (нет тиков)' }

    $greenAfter = [MaxUi]::CountColor((Join-Path $Out '07-alloc-done.png'), 230, 6, 180, 20, 0, 255, 0, 40)
    Write-Host "E: green title pixels after Done: $greenAfter"

    Write-Host ''
    Write-Host '--- evidence ------------------------------------------------'
    Write-Host "A: $clickLine"
    Write-Host "B: alloc=open, title green=$greenTitle"
    Write-Host "C: $lastInput"
    Write-Host "D: labels green=$greenLabels"
    Write-Host "E: title green after Done=$greenAfter"

    if ($failures.Count -gt 0) {
        Write-Host ''
        Write-Host "ui-test-allocate: FAIL ($($failures.Count))" -ForegroundColor Red
        foreach ($failure in $failures) { Write-Host " - $failure" -ForegroundColor Red }
        Write-Host "trace: $script:trace"
        exit 1
    }

    Write-Host ''
    Write-Host 'ui-test-allocate: PASS' -ForegroundColor Green
    Write-Host "trace: $script:trace"
    Write-Host "shots: $Out"
    exit 0
}
catch {
    Write-Host "ui-test-allocate: ERROR: $_" -ForegroundColor Red
    $tail = @((Read-Trace) -split "`r?`n" | Select-Object -Last 25)
    Write-Host '--- trace tail ---'
    $tail | ForEach-Object { Write-Host $_ }
    exit 1
}
finally {
    if (-not $KeepProcess) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
        Get-Process max-brh -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }
}
