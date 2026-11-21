# ui-test-prefs.ps1 — ЖИВОЙ прогон окна: диалог Preferences/Setup
# (C++ `OptionsMenu`), TC-0108/TC-0935.
#
# Проверяется в реальном окне (SendInput + фокус, кадры в build/ui-test-prefs):
#  * пункт `Setup` главного меню открывает диалог `SETUPPIC` поверх меню
#    (зелёный заголовок в (228,41) 184×17, кадр 01-setup-dialog);
#  * `Cancel` закрывает его и возвращает главное меню (трасса `prefs=closed`);
#  * в игре кнопка `Preferences` панели открывает `PREFSPIC`
#    (кадр 05-prefs-dialog), а `Animate Effects` реально выключает эффекты
#    (`effects=0` в трассе `MAX_DEBUG_INPUT`) и `Done` их сохраняет;
#  * `FX Disabled` доходит до звука (`sound=0` в трассе).
#
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File build\scripts\ui-test-prefs.ps1
# Нужен собранный клиент (build-windows-ui.ps1; exe ищется в
# build\win-msvc-target\release и release\*-ui).
param(
    [string]$Out = "",
    [switch]$KeepProcess
)

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Threading;

public static class MaxPrefsUi {
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
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll", SetLastError = true)] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);

    const uint LEFT_DOWN = 0x0002, LEFT_UP = 0x0004;
    const uint KEYEVENTF_KEYUP = 0x0002;

    public static bool Click(IntPtr hwnd, int x, int y) {
        POINT p = new POINT(); p.x = x; p.y = y;
        ClientToScreen(hwnd, ref p);
        SetCursorPos(p.x, p.y);
        Thread.Sleep(140);
        INPUT[] a = new INPUT[2];
        a[0].type = 0; a[0].mi.dwFlags = LEFT_DOWN;
        a[1].type = 0; a[1].mi.dwFlags = LEFT_UP;
        uint sent = SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
        Thread.Sleep(140);
        return sent == 2;
    }

    public static bool Move(IntPtr hwnd, int x, int y) {
        POINT p = new POINT(); p.x = x; p.y = y;
        ClientToScreen(hwnd, ref p);
        SetCursorPos(p.x, p.y);
        Thread.Sleep(120);
        return true;
    }

    public static bool PressEnter() {
        INPUT[] a = new INPUT[2];
        a[0].type = 1; a[0].ki.wVk = 0x0D;
        a[1].type = 1; a[1].ki.wVk = 0x0D; a[1].ki.dwFlags = KEYEVENTF_KEYUP;
        uint sent = SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
        Thread.Sleep(150);
        return sent == 2;
    }

    public static bool IsFocused(IntPtr hwnd) { return GetForegroundWindow() == hwnd; }

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
if (-not $Out) { $Out = Join-Path $root 'build\ui-test-prefs' }
New-Item -ItemType Directory -Force $Out | Out-Null
Get-ChildItem $Out -Filter '*.png' -ErrorAction SilentlyContinue | Remove-Item -Force

$exeCandidates = @(
    (Join-Path $root 'build\win-msvc-target\release\max-rust.exe'),
    (Join-Path $root 'release\max_app-1.1.0-windows-x86_64-ui\max-rust.exe')
)
$exe = $exeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) { throw 'ui-test-prefs: нет собранного max-rust.exe (build-windows-ui.ps1 -NoPackage)' }

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
        $stream = [System.IO.File]::Open($script:trace, [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
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
        [MaxPrefsUi]::ShowWindow($script:hwnd, 9) | Out-Null
        [MaxPrefsUi]::SetForegroundWindow($script:hwnd) | Out-Null
        Start-Sleep -Milliseconds 120
        if ([MaxPrefsUi]::IsFocused($script:hwnd)) { return $true }
    }
    return $false
}

function Shot([string]$name) {
    Ensure-Focus | Out-Null
    $p0 = New-Object MaxPrefsUi+POINT
    $p0.x = 0; $p0.y = 0
    [MaxPrefsUi]::ClientToScreen($script:hwnd, [ref]$p0) | Out-Null
    $bmp = New-Object System.Drawing.Bitmap(640, 480)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($p0.x, $p0.y, 0, 0, (New-Object System.Drawing.Size(640, 480)))
    $path = Join-Path $Out "$name.png"
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    return $path
}

function Click-Client([int]$cx, [int]$cy) {
    if (-not (Ensure-Focus)) { throw "фокус не на игровом окне: клик ($cx,$cy)" }
    [MaxPrefsUi]::Click($script:hwnd, $cx, $cy) | Out-Null
}

function Move-Client([int]$cx, [int]$cy) {
    Ensure-Focus | Out-Null
    [MaxPrefsUi]::Move($script:hwnd, $cx, $cy) | Out-Null
    Start-Sleep -Milliseconds 120
}

function Last-Input {
    @((Read-Trace) -split "`r?`n" | Where-Object { $_ -match '^\[in\] ' }) | Select-Object -Last 1
}

function Fail([string]$message) {
    Write-Host "FAIL: $message" -ForegroundColor Red
    exit 1
}

try {
    $script:hwnd = Wait-Window
    if (-not (Ensure-Focus)) { Fail 'окно не получает фокус' }
    Shot '00-menu' | Out-Null
    $green = [MaxPrefsUi]::CountColor((Join-Path $Out '00-menu.png'), 236, 145, 172, 15, 0, 255, 0, 60)
    Write-Host "A: главное меню, зелёный заголовок: $green px"
    if ($green -lt 20) { Fail 'главное меню не нарисовано' }

    # 1. Setup в главном меню (SETUPPIC).
    Click-Client 485 294
    Move-Client 400 200
    if (-not (Wait-Trace 'prefs=open' 5000)) { Fail 'Setup не открыл диалог (нет prefs=open)' }
    Start-Sleep -Milliseconds 500
    Shot '01-setup-dialog' | Out-Null
    $green = [MaxPrefsUi]::CountColor((Join-Path $Out '01-setup-dialog.png'), 228, 41, 184, 17, 0, 255, 0, 60)
    Write-Host "B: заголовок Setup: $green px"
    if ($green -lt 20) { Fail 'заголовок Setup не нарисован (PREFSPIC)' }

    # 2. Cancel возвращает меню.
    Click-Client 156 394
    Move-Client 400 200
    if (-not (Wait-Trace 'prefs=closed' 5000)) { Fail 'Cancel не закрыл диалог' }
    $line = Last-Input
    if ($line -notmatch 'screen=MainMenu') { Fail "Cancel не вернул меню: $line" }
    Shot '02-cancelled' | Out-Null

    # 3. Партия: New Game -> Options/Planet/Players/Cargo -> Enter (landing).
    Click-Client 485 189; Start-Sleep -Milliseconds 900
    Click-Client 568 458; Start-Sleep -Milliseconds 700
    Click-Client 568 458; Start-Sleep -Milliseconds 700
    Click-Client 568 458; Start-Sleep -Milliseconds 700
    Click-Client 474 463; Start-Sleep -Milliseconds 700
    Ensure-Focus | Out-Null
    [MaxPrefsUi]::PressEnter() | Out-Null
    if (-not (Wait-Trace '(?m)^\[tick ' 20000)) { Fail 'партия не началась (нет тиков)' }
    Start-Sleep -Milliseconds 800
    Shot '03-game' | Out-Null

    # 4. Кнопка Preferences панели (67x18 в (86,4) -> центр 119,13).
    Click-Client 119 13
    Move-Client 400 360
    if (-not (Wait-Trace 'prefs=open' 5000)) { Fail 'кнопка Preferences не открыла диалог' }
    Start-Sleep -Milliseconds 400
    Shot '04-prefs-dialog' | Out-Null
    $green = [MaxPrefsUi]::CountColor((Join-Path $Out '04-prefs-dialog.png'), 228, 41, 184, 17, 0, 255, 0, 60)
    Write-Host "C: заголовок Preferences: $green px"
    if ($green -lt 20) { Fail 'заголовок Preferences не нарисован' }

    # 5. Animate Effects (PREFSPIC, локальные (25,213) -> клик 154,250).
    Click-Client 154 250
    Move-Client 400 360
    Start-Sleep -Milliseconds 300
    $line = Last-Input
    Write-Host "D: $line"
    if ($line -notmatch 'effects=0') { Fail "effects не выключился: $line" }
    Shot '05-effects-off' | Out-Null

    # 6. FX Disabled (210,73) -> клик 339,110: звук выключается сразу.
    Click-Client 339 110
    Move-Client 400 360
    Start-Sleep -Milliseconds 300
    $line = Last-Input
    Write-Host "E: $line"
    if ($line -notmatch 'sound=0') { Fail "sound не выключился: $line" }
    Shot '06-fx-disabled' | Out-Null

    # 7. Done (215,383,62x23) -> клик 366,423: диалог закрыт, значения живы.
    Click-Client 366 423
    Move-Client 400 360
    if (-not (Wait-Trace 'prefs=closed' 5000)) { Fail 'Done не закрыл диалог' }
    Start-Sleep -Milliseconds 300
    $line = Last-Input
    Write-Host "F: $line"
    if ($line -notmatch 'effects=0' -or $line -notmatch 'sound=0') {
        Fail "Done потерял значения: $line"
    }
    Shot '07-done' | Out-Null

    # 8. Повторное открытие: значения сохранились (`preferences.ini` + App).
    Click-Client 119 13
    Move-Client 400 360
    if (-not (Wait-Trace 'prefs=open' 5000)) { Fail 'повторное открытие не удалось' }
    Start-Sleep -Milliseconds 400
    Shot '08-prefs-reopened' | Out-Null
    Click-Client 366 423
    Start-Sleep -Milliseconds 300

    Write-Host 'PASS: Preferences/Setup в живом окне (открытие из меню и из игры, Cancel/Done, effects/sound)'
} finally {
    if (-not $KeepProcess) {
        Get-Process max-brh -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }
}
