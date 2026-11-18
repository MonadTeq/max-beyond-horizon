# ui-test-construction.ps1 — ЖИВОЙ прогон окна M.A.X. Rust-клиента:
# стройплощадка на карте после заказа здания (C++ `DeployConstructionSiteUtilities`
# + `PrepareConstructionSite`).
#
# Что проверяется в реальном окне (клики/клавиши через SendInput, фокус
# проверяется):
#  * новый старт (меню → Options → Planet → Players → мобилизация → высадка);
#  * конструктор находится по трассе `MAX_DEBUG_INPUT` (тип `49`/`CONSTRCT`),
#    камера наводится на него кликом по миникарте (1 px = 1 клетка);
#  * `B` открывает BuildMenu, стрелки вниз выбирают Training Hall, Enter
#    заказывает стройку;
#  * на следующем кадре `ORDER_BUILD` на карте появляются настоящие декали
#    `LRGTAPE` (тип 15), `LRGSLAB` (17), `LRGCONES` (19) — видно и в трассе, и
#    на скриншоте (жёлто-чёрная лента/плита у конструктора);
#  * кадры `00-menu` … `03-construction-site` — в `build/ui-test-construction/`.
#
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File build\scripts\ui-test-construction.ps1
#
# Нужен собранный клиент:
#   powershell -ExecutionPolicy Bypass -File build\scripts\build-windows-ui.ps1 -NoPackage
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

public static class MaxUiBuild {
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

    public static bool PressKey(ushort vk) {
        INPUT[] a = new INPUT[2];
        a[0].type = 1; a[0].ki.wVk = vk;
        a[1].type = 1; a[1].ki.wVk = vk; a[1].ki.dwFlags = KEYEVENTF_KEYUP;
        uint sent = SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
        Thread.Sleep(90);
        return sent == 2;
    }

    public static bool IsFocused(IntPtr hwnd) {
        return GetForegroundWindow() == hwnd;
    }

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
if (-not $Out) { $Out = Join-Path $root 'build\ui-test-construction' }
New-Item -ItemType Directory -Force $Out | Out-Null
Get-ChildItem $Out -Filter '*.png' -ErrorAction SilentlyContinue | Remove-Item -Force

$exeCandidates = @(
    (Join-Path $root 'build\win-msvc-target\release\max-rust.exe'),
    (Join-Path $root 'release\max_app-1.1.0-windows-x86_64-ui\max-rust.exe')
)
$exe = $exeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) { throw "ui-test-construction: нет собранного max-rust.exe (build-windows-ui.ps1 -NoPackage)" }

$env:MAX_DEBUG_INPUT = '1'
$script:trace = Join-Path $Out 'trace.log'

Get-Process max-rust -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

$script:process = Start-Process -FilePath $exe `
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
        $script:process.Refresh()
        if ($script:process.HasExited) { throw "окно: процесс завершился (exit=$($script:process.ExitCode))" }
        if ($script:process.MainWindowHandle -ne 0) { return $script:process.MainWindowHandle }
        Start-Sleep -Milliseconds 200
    }
    throw 'окно не появилось за 20 с'
}

function Ensure-Focus {
    for ($i = 0; $i -lt 12; $i++) {
        [MaxUiBuild]::ShowWindow($script:hwnd, 9) | Out-Null
        [MaxUiBuild]::SetForegroundWindow($script:hwnd) | Out-Null
        Start-Sleep -Milliseconds 120
        if ([MaxUiBuild]::IsFocused($script:hwnd)) { return $true }
    }
    return $false
}

function Shot([string]$name) {
    Ensure-Focus | Out-Null
    $p0 = New-Object MaxUiBuild+POINT
    $p0.x = 0; $p0.y = 0
    [MaxUiBuild]::ClientToScreen($script:hwnd, [ref]$p0) | Out-Null
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
    [MaxUiBuild]::Click($script:hwnd, $cx, $cy, $right) | Out-Null
}

function Press-Key([int]$vk) {
    if (-not (Ensure-Focus)) { throw 'фокус не на игровом окне: клавиша не будет доставлена' }
    [MaxUiBuild]::PressKey([uint16]$vk) | Out-Null
}

function Find-Line([string]$text, [string]$pattern) {
    @($text -split "`r?`n" | Where-Object { $_ -match $pattern }) | Select-Object -Last 1
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
    Press-Key 0x0D                                       # Enter — высадка
    Start-Sleep -Seconds 1

    if (-not (Wait-Trace '(?m)^\[tick ' 20000)) {
        Fail 'партия не началась: нет тиков InSession'
        throw 'меню не пройдено'
    }
    Shot '01-game' | Out-Null

    Step 'конструктор по трассе -> камера на него'
    $tick = Find-Line (Read-Trace) '^\[tick .* units='
    if (-not $tick) { throw 'в трассе нет строки tick' }
    if ($tick -notmatch '(?:^|\s)49@\((\d+),(\d+)\)') {
        Fail "в трассе нет конструктора (тип 49): $tick"
        throw 'нет конструктора'
    }
    $cx = [int]$Matches[1]; $cy = [int]$Matches[2]
    Write-Host "constructor at ($cx,$cy)"
    # Миникарта 112×112 в (15,356): 1 px = 1 клетка мира.
    Click-Client (15 + $cx) (356 + $cy)
    Start-Sleep -Milliseconds 500
    Click-Client 404 242
    Start-Sleep -Milliseconds 500
    Shot '02-constructor-selected' | Out-Null

    $clickLine = Find-Line (Read-Trace) '^\[click\]'
    Write-Host "A: $clickLine"
    if (-not $clickLine) { Fail 'клик по конструктору не дошёл до окна' }

    Step 'B -> BuildMenu -> Training Hall (13 × «вниз») -> Done'
    Press-Key 0x42
    Start-Sleep -Milliseconds 700
    # Стрелка вниз списка `BLDDWN_U` — `LIST_DOWN` (491,441,18,17), центр
    # (500,449); клик идёт в меню, как у игрока.
    for ($i = 0; $i -lt 13; $i++) {
        Click-Client 500 449
        Start-Sleep -Milliseconds 90
    }
    Shot '02-buildmenu' | Out-Null
    Click-Client 424 463   # `Done` (397,452,54,23) — заказ
    Start-Sleep -Milliseconds 900

    Step 'стройплощадка на карте: LRGTAPE/LRGSLAB/LRGCONES'
    Start-Sleep -Seconds 1
    $after = Read-Trace
    $tape = Find-Line $after '(?:^|[^0-9])15@\(\d+,\d+\)'
    $slab = Find-Line $after '(?:^|[^0-9])17@\(\d+,\d+\)'
    $cones = Find-Line $after '(?:^|[^0-9])19@\(\d+,\d+\)'
    Write-Host "B: tape=$tape"
    Write-Host "C: slab=$slab"
    Write-Host "D: cones=$cones"
    if (-not $tape) { Fail 'LRGTAPE (15) не появился в состоянии после заказа' }
    if (-not $slab) { Fail 'LRGSLAB (17) не появился в состоянии после заказа' }
    if (-not $cones) { Fail 'LRGCONES (19) не появился в состоянии после заказа' }
    Shot '03-construction-site' | Out-Null

    # Контраст: на кадре плато/лента занимает заметную площадь (жёлтый/чёрный
    # узор), проверяем непустую заливку центра карты цветом, отличным от снега.
    $sitePixels = [MaxUiBuild]::CountColor((Join-Path $Out '03-construction-site.png'), 250, 150, 300, 200, 0, 0, 0, 90)
    Write-Host "E: dark decoration pixels: $sitePixels"
    if ($sitePixels -lt 500) { Fail "стройплощадка почти не видна (тёмных пикселей: $sitePixels)" }

    Write-Host ''
    Write-Host '--- evidence ------------------------------------------------'
    Write-Host "A: $clickLine"
    Write-Host "B: $tape"
    Write-Host "C: $slab"
    Write-Host "D: $cones"
    Write-Host "E: dark decoration pixels=$sitePixels"

    if ($failures.Count -gt 0) {
        Write-Host ''
        Write-Host "ui-test-construction: FAIL ($($failures.Count))" -ForegroundColor Red
        foreach ($failure in $failures) { Write-Host " - $failure" -ForegroundColor Red }
        Write-Host "trace: $script:trace"
        exit 1
    }

    Write-Host ''
    Write-Host 'ui-test-construction: PASS' -ForegroundColor Green
    Write-Host "trace: $script:trace"
    Write-Host "shots: $Out"
    exit 0
}
catch {
    Write-Host "ui-test-construction: ERROR: $_" -ForegroundColor Red
    $tail = @((Read-Trace) -split "`r?`n" | Select-Object -Last 25)
    Write-Host '--- trace tail ---'
    $tail | ForEach-Object { Write-Host $_ }
    exit 1
}
finally {
    if (-not $KeepProcess) {
        if ($script:process -and -not $script:process.HasExited) {
            Stop-Process -Id $script:process.Id -Force -ErrorAction SilentlyContinue
        }
        Get-Process max-rust -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }
}
