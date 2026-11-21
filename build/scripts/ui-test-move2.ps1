# ui-test-move2.ps1 — ЖИВОЙ прогон окна M.A.X. Rust-клиента:
# LMB-выбор юнита, RMB-приказ (SendInput!), зелёный предпросмотр пути,
# End Turn и проверка, что юнит реально перешёл в целевую клетку.
#
# Отличия от старой версии:
#  * кнопки мыши посылаются через SendInput (MOUSEEVENTF_LEFT/RIGHTDOWN/UP), а
#    не через устаревший mouse_event;
#  * перед каждым кликом проверяется фокус (GetForegroundWindow == hwnd);
#    при потере фокуса окно поднимается (ShowWindow/SW_RESTORE + SetForegroundWindow);
#  * точка высадки подтверждается клавишей Enter (как QA `key ENTER`), поэтому
#    база всегда в START_POSITIONS, а не там, куда случайно попал клик по карте;
#  * базовые клетки шахты/генератора/юнитов берутся из трассы MAX_DEBUG_INPUT,
#    а не из жёстких пикселей: сценарий работает при любой выбранной точке;
#  * проверяется, что зелёный предпросмотр (hud::PREVIEW_COLOR) реально
#    нарисован в кадре окна, и что после End Turn `grid` юнита = цель, а
#    визуальная позиция `vX.Y` прошла маршрут.
#
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File build\scripts\ui-test-move2.ps1
param(
    [string]$Out = "",
    [switch]$KeepProcess
)

$ErrorActionPreference = 'Stop'
$root = 'D:\dev\max-port-debug'
if (-not $Out) { $Out = Join-Path $root 'build\ui-test-move2' }
New-Item -ItemType Directory -Force $Out | Out-Null
Get-ChildItem $Out -Filter '*.png' -ErrorAction SilentlyContinue | Remove-Item -Force

$exeCandidates = @(
    (Join-Path $root 'build\win-msvc-target\release\max-brh.exe'),
    (Join-Path $root 'release\max_app-1.1.0-windows-x86_64-ui\max-brh.exe')
)
$exe = $exeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) { throw "ui-test-move2: нет собранного max-brh.exe (build-windows-ui.ps1 -NoPackage)" }

$srcTime = (Get-ChildItem (Join-Path $root 'max-brh\crates\max_app\src') -Recurse -File |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1).LastWriteTime
$exeTime = (Get-Item $exe).LastWriteTime
if ($exeTime -lt $srcTime) {
    Write-Warning "ui-test-move2: exe ($exeTime) старше исходников ($srcTime) — пересоберите build-windows-ui.ps1"
}

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

# --- окружение процесса -----------------------------------------------------
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
        # Файл пишет дочерний процесс: открываем с FileShare.ReadWrite, иначе
        # ReadAllText падает и трасса выглядит пустой во время прогона.
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
        [MaxUi]::ShowWindow($script:hwnd, 9) | Out-Null   # SW_RESTORE
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

# --- разбор трассы ----------------------------------------------------------
function Get-LastTick([string]$text) {
    $lines = @($text -split "`r?`n" | Where-Object { $_ -match '^\[tick ' })
    if ($lines.Count -eq 0) { return $null }
    return $lines[-1]
}

function Get-Units([string]$tickLine) {
    if (-not $tickLine) { return @() }
    $units = @()
    foreach ($m in [regex]::Matches($tickLine, '(\d+)@\((-?\d+),(-?\d+)\)v(-?\d+\.\d),(-?\d+\.\d)')) {
        $units += [pscustomobject]@{
            Type = [int]$m.Groups[1].Value
            GX   = [int]$m.Groups[2].Value
            GY   = [int]$m.Groups[3].Value
            VX   = [double]$m.Groups[4].Value
            VY   = [double]$m.Groups[5].Value
        }
    }
    return $units
}

function Find-Line([string]$text, [string]$pattern) {
    @($text -split "`r?`n" | Where-Object { $_ -match $pattern }) | Select-Object -Last 1
}

function Tick-Selection([string]$text) {
    $line = Get-LastTick $text
    if (-not $line) { return $null }
    $match = [regex]::Match($line, 'sel=(\d+)')
    if (-not $match.Success) { return $null }
    return [int]$match.Groups[1].Value
}

function Tick-Turn([string]$text) {
    $line = Get-LastTick $text
    if (-not $line) { return -1 }
    $match = [regex]::Match($line, 'turn=(\d+)')
    if (-not $match.Success) { return -1 }
    return [int]$match.Groups[1].Value
}

$failures = New-Object System.Collections.Generic.List[string]
function Fail([string]$message) {
    $failures.Add($message) | Out-Null
    Write-Host "FAIL: $message" -ForegroundColor Red
}
function Step([string]$message) {
    Write-Host "== $message ==" -ForegroundColor Cyan
}

try {
    Step 'окно и меню -> партия'
    $script:hwnd = Wait-Window
    if (-not (Ensure-Focus)) { Fail 'окно не получает фокус перед стартом' }
    Shot '01-menu' | Out-Null

    # MainMenu -> Options -> Done; Planet -> Done; Players -> Done; Mobilize -> Done.
    Click-Client 485 189; Start-Sleep -Seconds 1        # New Game
    Shot '02-options' | Out-Null
    Click-Client 568 458; Start-Sleep -Milliseconds 800  # Done (Options)
    Shot '03-planet' | Out-Null
    Click-Client 568 458; Start-Sleep -Milliseconds 800  # Done (Planet)
    Shot '04-players' | Out-Null
    Click-Client 312 194; Start-Sleep -Milliseconds 400  # слот 1: Human (уже)
    Click-Client 568 458; Start-Sleep -Milliseconds 800  # Done (Players)
    Shot '05-mobilize' | Out-Null
    Click-Client 474 463; Start-Sleep -Milliseconds 800  # Done (CargoMenu)
    Shot '06-landing' | Out-Null

    # Точка высадки: Enter подтверждает точку по умолчанию (START_POSITIONS +
    # FindValidStartingPosition), как `key ENTER` в move-input.qa. Клик по карте
    # здесь выбрал бы точку под курсором и сломал бы абсолютные координаты.
    Ensure-Focus | Out-Null
    [MaxUi]::PressEnter() | Out-Null
    Start-Sleep -Seconds 1

    # В окне сессия начинается без ввода: признак — первые тики локальной партии
    # (`[tick ...]` печатается только при `screen = InSession`).
    if (-not (Wait-Trace '(?m)^\[tick ' 20000)) {
        Fail 'партия не началась: нет тиков InSession'
        throw 'меню не пройдено'
    }

    $deadline = [DateTime]::Now.AddSeconds(10)
    $units = @()
    $mine = $null
    $surveyor = $null
    while ([DateTime]::Now -lt $deadline) {
        $units = Get-Units (Get-LastTick (Read-Trace))
        $mine = $units | Where-Object { $_.Type -eq 40 } | Select-Object -First 1
        $surveyor = $units | Where-Object { $_.Type -eq 57 } | Select-Object -First 1
        if ($mine -and $surveyor) { break }
        Start-Sleep -Milliseconds 200
    }
    if (-not $mine) { throw 'в трассе нет шахты (тип 40)' }
    if (-not $surveyor) { throw 'в трассе нет Surveyor (тип 57)' }

    $generator = $units | Where-Object { $_.Type -eq 2 } | Select-Object -First 1
    $largeSlab = $units | Where-Object { $_.Type -eq 17 } | Select-Object -First 1
    if (-not $generator) { throw 'в трассе нет генератора (тип 2)' }
    if (-not $largeSlab) { throw 'в трассе нет LRGSLAB (тип 17)' }

    Shot '07-game' | Out-Null
    Write-Host ("A: mining=40@({0},{1})  generator=2@({2},{3})  large-slab=17@({4},{5})  surveyor=57@({6},{7})" -f `
        $mine.GX, $mine.GY, $generator.GX, $generator.GY, $largeSlab.GX, $largeSlab.GY, $surveyor.GX, $surveyor.GY)

    # (A) Генератор не на клетке шахты; на клетке высадки — шахта и плита.
    if ($generator.GX -eq $mine.GX -and $generator.GY -eq $mine.GY) {
        Fail "генератор стоит на клетке шахты ($($mine.GX),$($mine.GY))"
    }
    $atBase = @($units | Where-Object { $_.GX -eq $mine.GX -and $_.GY -eq $mine.GY } |
        ForEach-Object { $_.Type } | Sort-Object)
    Write-Host "A: units at mining cell: $($atBase -join ',')"
    if (-not ($atBase -contains 40)) { Fail 'на клетке высадки нет шахты' }
    if (-not ($atBase -contains 17)) { Fail 'на клетке шахты нет LRGSLAB (как C++ DeployUnit)' }
    if ($atBase -contains 2) { Fail 'генератор делит клетку с шахтой' }

    # --- LMB: выбрать Surveyor ------------------------------------------------
    Step 'LMB: выбор Surveyor'
    $centerX = 404; $centerY = 242   # центр MAIN_MAP; камера на шахте
    $sx = $centerX + 64 * ($surveyor.GX - $mine.GX)
    $sy = $centerY + 64 * ($surveyor.GY - $mine.GY)
    Click-Client $sx $sy
    Start-Sleep -Milliseconds 700
    Shot '08-selected' | Out-Null

    $clickLog = Find-Line (Read-Trace) '^\[click\]'
    if (-not $clickLog -or $clickLog -notmatch "grid=\($($surveyor.GX),$($surveyor.GY)\)") {
        Fail "LMB не попал в клетку Surveyor'а ($($surveyor.GX),$($surveyor.GY)): $clickLog"
    }
    $selId = Tick-Selection (Read-Trace)
    if ($null -eq $selId) { Fail 'LMB не выделил юнита (в трассе нет sel=ID)' }

    # --- RMB: приказ на соседнюю клетку --------------------------------------
    Step 'RMB: приказ движения (SendInput)'
    # Сначала цели в 2 клетках: маршрут длиннее одной клетки, поэтому в трассе
    # видно несколько промежуточных `vX.Y` (визуальный проход маршрута).
    $destinations = @(
        @(2, 0), @(0, 2), @(-2, 0), @(0, -2),
        @(2, -2), @(2, 2), @(-2, 2), @(-2, -2),
        @(1, 0), @(0, 1), @(-1, 0), @(0, -1)
    )
    $moved = $false
    $destGX = 0; $destGY = 0
    foreach ($d in $destinations) {
        $gx = $surveyor.GX + $d[0]
        $gy = $surveyor.GY + $d[1]
        $cx = $centerX + 64 * ($gx - $mine.GX)
        $cy = $centerY + 64 * ($gy - $mine.GY)
        if ($cx -lt 181 -or $cx -gt 626 -or $cy -lt 19 -or $cy -gt 465) { continue }

        for ($attempt = 0; $attempt -lt 2; $attempt++) {
            # Повторный LMB: после неудачного RMB (Cancel/No path) выделение
            # могло сброситься, а без выделения RMB отвечает no-selection.
            Click-Client $sx $sy
            Start-Sleep -Milliseconds 350
            $before = ([regex]::Matches((Read-Trace), '\[rclick\] result=')).Count
            Click-Client $cx $cy $true
            Start-Sleep -Milliseconds 500

            $lines = @((Read-Trace) -split "`r?`n" | Where-Object { $_ -match '^\[rclick\] result=' })
            $result = if ($lines.Count -gt $before) { $lines[-1] } else { '' }
            Write-Host "RMB ($gx,$gy) attempt $($attempt + 1): $result"
            if ($result -match 'result=move') {
                $moved = $true
                $destGX = $gx; $destGY = $gy
                break
            }
            if ($result) { break }   # событие дошло, но приказ не дал движения — другая цель
        }

        if ($moved) { break }
    }
    if (-not $moved) { throw 'RMB не дал приказ движения ни на одну соседнюю клетку' }

    if (-not (Find-Line (Read-Trace) '\[in\] screen=InSession event=RightClick')) {
        Fail 'RightClick не дошёл до окна как InputEvent::RightClick'
    }
    $previewLine = Find-Line (Read-Trace) '\[tick .* preview=[1-9]'
    if (-not $previewLine) { Fail 'предпросмотр пути не появился (preview=0)' }

    Shot '09-move-preview' | Out-Null
    $green = [MaxUi]::CountColor((Join-Path $Out '09-move-preview.png'), 180, 18, 448, 448, 120, 255, 152, 2)
    Write-Host "B: green preview pixels in map area: $green"
    if ($green -lt 8) { Fail "зелёный предпросмотр не нарисован в chrome-кадре (пикселей: $green)" }

    $rclickLine = Find-Line (Read-Trace) '^\[rclick\] result=move'
    Write-Host "B: $rclickLine"
    Write-Host "B: $previewLine"

    # --- End Turn -------------------------------------------------------------
    Step 'End Turn и фактическое перемещение'
    $turnBefore = Tick-Turn (Read-Trace)
    Ensure-Focus | Out-Null
    Click-Client 426 12
    Start-Sleep -Seconds 6
    Shot '10-after-turn' | Out-Null

    $text = Read-Trace
    $surveyorTicks = @()
    foreach ($m in [regex]::Matches($text, '57@\((-?\d+),(-?\d+)\)v(-?\d+\.\d),(-?\d+\.\d)')) {
        $surveyorTicks += [pscustomobject]@{
            GX = [int]$m.Groups[1].Value; GY = [int]$m.Groups[2].Value
            VX = [double]$m.Groups[3].Value; VY = [double]$m.Groups[4].Value
        }
    }
    $atDest = @($surveyorTicks | Where-Object { $_.GX -eq $destGX -and $_.GY -eq $destGY })
    if ($atDest.Count -eq 0) {
        $last = $surveyorTicks | Select-Object -Last 1
        Fail "после End Turn Surveyor не на целевой клетке ($destGX,$destGY); последняя: $($last.GX),$($last.GY)"
    }

    $visuals = @($surveyorTicks | ForEach-Object { "{0},{1}" -f $_.VX, $_.VY } | Select-Object -Unique)
    Write-Host "C: surveyor grid/vis trace: $($visuals -join ' -> ')"
    if ($visuals.Count -lt 3) {
        Fail "визуальная позиция не проходила маршрут (кадров: $($visuals.Count))"
    }
    $finalVisual = "{0},{1}" -f $destGX, $destGY
    if ($visuals[-1] -ne $finalVisual) {
        Fail "визуальная позиция не дошла до цели: $($visuals[-1]) != $finalVisual"
    }

    # --- Выбор юнита уже в новой клетке (следующий ход) ----------------------
    Step 'LMB по целевой клетке после смены хода'
    $dx = $centerX + 64 * ($destGX - $mine.GX)
    $dy = $centerY + 64 * ($destGY - $mine.GY)
    $selectedAtDest = $false
    for ($i = 0; $i -lt 25; $i++) {
        $line = Get-LastTick (Read-Trace)
        if ($line -match 'phase=Your turn' -and (Tick-Turn (Read-Trace)) -gt $turnBefore) { break }
        Start-Sleep -Seconds 1
    }
    for ($i = 0; $i -lt 10; $i++) {
        Click-Client $dx $dy
        Start-Sleep -Milliseconds 600
        $lastClick = Find-Line (Read-Trace) '^\[click\]'
        if ($lastClick -match "grid=\($destGX,$destGY\)" -and
            ((Tick-Selection (Read-Trace)) -eq $selId)) {
            $selectedAtDest = $true
            break
        }
    }
    Shot '11-after-select' | Out-Null
    if (-not $selectedAtDest) {
        Fail "после End Turn юнит не выделяется в целевой клетке ($destGX,$destGY)"
    }

    Write-Host ''
    Write-Host '--- evidence ------------------------------------------------'
    Write-Host ("A: mining 40@({0},{1}) vs generator 2@({2},{3}); base cell units={4}" -f `
        $mine.GX, $mine.GY, $generator.GX, $generator.GY, ($atBase -join ','))
    Write-Host "B: $rclickLine"
    Write-Host "B: preview pixels=$green"
    Write-Host ("C: 57@({0},{1}) -> ({2},{3}); vis {4}" -f `
        $surveyor.GX, $surveyor.GY, $destGX, $destGY, ($visuals -join ' -> '))

    if ($failures.Count -gt 0) {
        Write-Host ''
        Write-Host "ui-test-move2: FAIL ($($failures.Count))" -ForegroundColor Red
        foreach ($failure in $failures) { Write-Host " - $failure" -ForegroundColor Red }
        Write-Host "trace: $script:trace"
        exit 1
    }

    Write-Host ''
    Write-Host 'ui-test-move2: PASS' -ForegroundColor Green
    Write-Host "trace: $script:trace"
    Write-Host "shots: $Out"
    exit 0
}
catch {
    Write-Host "ui-test-move2: ERROR: $_" -ForegroundColor Red
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
