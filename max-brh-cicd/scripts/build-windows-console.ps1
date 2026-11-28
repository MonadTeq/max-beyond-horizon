#requires -version 5.1
<#
.SYNOPSIS
  Native Windows build of the M.A.X. Rust client console binary (max_app) with MSVC,
  without Docker and without mingw.

.DESCRIPTION
  Replaces the previous cross-build max-brh-cicd/scripts/build-windows.sh
  (Docker + mingw-w64, target x86_64-pc-windows-gnu). Here everything is built on
  the Windows host with the native toolchain:

    * MSVC x64 + Windows SDK are located via vswhere and activated by vcvars64.bat;
    * Rust 1.94.0 (rustup, %USERPROFILE%\.cargo), the default target is
      x86_64-pc-windows-msvc (rustup default on Windows);
    * vcpkg/SDL3 are NOT needed: the build uses --no-default-features, that is, without
      the sdl* features (SDL3 window) and without sound (rodio/cpal → WASAPI).

  Build command:

    cargo build --release --locked -p max_app --no-default-features

  The build does not pollute max-brh/target (where the Linux/Docker artifacts live):
  it uses CARGO_TARGET_DIR=<repo>\bin\target-console (the same as
  build-windows-ui.ps1, which builds into <repo>\bin\target).

  Post-build checks (native, no wine):
    * PE header: MZ, PE32+ x64, Subsystem=3 (Console) — the client's console binary
      must be a console one, otherwise --help/--demo print nothing to the terminal/CI;
    * imported DLL list (dumpbin /dependents): system ones + vcruntime140,
      no mingw runtime of any kind (libgcc_s_*/libwinpthread-1);
    * max-brh.exe --help prints the help (the "ИСПОЛЬЗОВАНИЕ" banner is searched for);
    * max-brh.exe --demo max-original\SAVE1.CAM --turns 1 runs through:
      the output contains ticks=1 and state_digest=0x….

  Packaging (the same as the previous build-windows.sh):
    bin\release\max_app-<ver>-windows-x86_64\            max-brh.exe, README.txt,
                                                         LICENSE, CHANGELOG.md,
                                                         SHA256SUMS
    bin\release\max_app-<ver>-windows-x86_64.zip          the directory archive
    bin\release\max_app-<ver>-windows-x86_64.zip.sha256   sha256 of the archive

.PARAMETER Out
  Artifact directory. Defaults to <repo>\bin\release.

.PARAMETER TargetDir
  CARGO_TARGET_DIR for the build. Defaults to <repo>\bin\target-console.

.PARAMETER NoPackage
  Only build and verify (--help/--demo), do not package.

.PARAMETER SkipSmoke
  Do not run the exe (--help/--demo) — build and PE checks only.
  Needed on a host without the original game data.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-console.ps1

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-console.ps1 -NoPackage
#>
param(
    [string]$Out = "",
    [string]$TargetDir = "",
    [switch]$NoPackage,
    [switch]$SkipSmoke
)

$ErrorActionPreference = "Stop"

function Fail([string]$message) {
    Write-Host "build-windows-console: $message" -ForegroundColor Red
    exit 1
}

# Native commands (cargo, cmd) write progress to stderr. With
# $ErrorActionPreference = "Stop" PowerShell 5.1 wraps stderr into
# NativeCommandError and kills the script even though the build is fine. While
# running native processes we switch to Continue and restore the exit code.
function Invoke-Native([scriptblock]$body) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $code = 0
    try {
        & $body
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return $code
}

# PE header parsing: MZ, the PE\0\0 signature, machine, PE32/PE32+ and Subsystem.
# Subsystem sits in the Optional Header at offset 0x44 for both PE32 and PE32+
# (PE32+ has no BaseOfData, but ImageBase is wider — the offset does not change), so
# the absolute offset is e_lfanew + 4 + 20 + 0x44 = e_lfanew + 0x5C.
# Parsing is inline rather than via pe-subsystem.ps1: the helper runs as a separate
# Windows PowerShell 5.1 process that reads .ps1 without a BOM as ANSI and
# chokes on Cyrillic strings.
function Get-PeInfo([string]$file) {
    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $file).Path)
    if ($bytes.Length -lt 0x40) { Fail "${file}: файл короче DOS-заголовка" }
    if ($bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) { Fail "${file}: нет MZ-заголовка" }
    $lfanew = [BitConverter]::ToInt32($bytes, 0x3C)
    $optional = $lfanew + 4 + 20
    if ($lfanew -lt 0x40 -or ($optional + 0x46) -gt $bytes.Length) {
        Fail "${file}: битый e_lfanew=$lfanew (файл $($bytes.Length) байт)"
    }
    if ($bytes[$lfanew] -ne 0x50 -or $bytes[$lfanew + 1] -ne 0x45 -or
        $bytes[$lfanew + 2] -ne 0 -or $bytes[$lfanew + 3] -ne 0) {
        Fail "${file}: нет сигнатуры PE\0\0 по смещению e_lfanew=$lfanew"
    }
    $machine = [int][BitConverter]::ToUInt16($bytes, $lfanew + 4)
    $magic = [int][BitConverter]::ToUInt16($bytes, $optional)
    [pscustomobject]@{
        Machine   = if ($machine -eq 0x8664) { "x64" } elseif ($machine -eq 0x014c) { "x86" } else { "0x{0:X4}" -f $machine }
        Magic     = if ($magic -eq 0x20B) { "PE32+" } elseif ($magic -eq 0x10B) { "PE32" } else { "0x{0:X4}" -f $magic }
        Subsystem = [int][BitConverter]::ToUInt16($bytes, $optional + 0x44)
    }
}

# <repo>\max-brh-cicd\scripts -> <repo>
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Out) { $Out = Join-Path $root "bin\release" }
elseif (-not [System.IO.Path]::IsPathRooted($Out)) { $Out = Join-Path (Get-Location).Path $Out }
if (-not $TargetDir) { $TargetDir = Join-Path $root "bin\target-console" }
elseif (-not [System.IO.Path]::IsPathRooted($TargetDir)) { $TargetDir = Join-Path (Get-Location).Path $TargetDir }

# --- workspace version ------------------------------------------------------
$manifest = Join-Path $root "max-brh\Cargo.toml"
if (-not (Test-Path $manifest)) { Fail "нет $manifest" }
$version = (Select-String -Path $manifest -Pattern '^version = "([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?)"').Matches.Groups[1].Value
if (-not $version) { Fail "не удалось прочитать версию из $manifest" }

$bundleName = "max_app-$version-windows-x86_64"
$bundle = Join-Path $Out $bundleName
$zip = Join-Path $Out "$bundleName.zip"

foreach ($required in @((Join-Path $root "LICENSE"), (Join-Path $root "CHANGELOG.md"))) {
    if (-not (Test-Path $required)) { Fail "отсутствует $required" }
}

# --- Visual Studio (MSVC x64 + Windows SDK) ---------------------------------
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) { Fail "нет vswhere.exe — Visual Studio не установлена" }
$vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsPath) { Fail "не найдена VS с workload 'Desktop development with C++' (VC.Tools.x86.x64)" }
$vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) { Fail "нет $vcvars" }

# --- cargo (rustup 1.94.0, x86_64-pc-windows-msvc target by default) --------
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path $cargo)) {
    $cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargoCmd) { Fail "cargo не найден: поставь Rust 1.94.0 (https://win.rustup.rs)" }
    $cargo = $cargoCmd.Source
}
$cargoBin = Split-Path -Parent $cargo

$exePath = Join-Path $TargetDir "release\max-brh.exe"
$workDir = Join-Path $root "build\msvc-console"
$buildLog = Join-Path $workDir "build.log"
$importsFile = Join-Path $workDir "imports.txt"
$helpFile = Join-Path $workDir "help.txt"
$demoLog = Join-Path $workDir "demo.log"
$smokeLog = Join-Path $root "build\smoke-logs\windows-msvc-console-smoke.log"
New-Item -ItemType Directory -Force -Path $workDir | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $smokeLog) | Out-Null

$buildCmd = "cargo build --release --locked -p max_app --no-default-features"

Write-Host "build-windows-console: M.A.X. Rust client $version → Windows x86_64 (консольная сборка, MSVC)"
Write-Host "  Visual Studio: $vsPath"
Write-Host "  cargo:         $cargo"
Write-Host "  target:        x86_64-pc-windows-msvc (native)"
Write-Host "  features:      <нет: --no-default-features, без sdl*, без sound>"
Write-Host "  target dir:    $TargetDir"
Write-Host "  каталог:       $bundle"
Write-Host "  архив:         $zip"

# vcvars64 is needed by the linker (link.exe), the Windows SDK and the C compiler for the vendored
# ENet 1.3.18 (cc → cl.exe). Environment variables are set via .bat: in cmd
# `set VAR=value && ...` captures a trailing space into the value, hence only
# `set "VAR=value"` (the same pitfall as in build-windows-ui.ps1).
$bat = Join-Path $root "build\msvc-console-build.bat"
$batLines = @(
    '@echo off',
    "call `"$vcvars`" >nul",
    "if errorlevel 1 exit /b 1",
    "set `"PATH=$cargoBin;%PATH%`"",
    "set `"CARGO_TARGET_DIR=$TargetDir`"",
    "cd /d `"$root\max-brh`"",
    "`"$cargo`" build --release --locked -p max_app --no-default-features",
    "if errorlevel 1 exit /b 1",
    "rem Импорты PE: dumpbin из состава MSVC (после vcvars он в PATH).",
    "if exist `"$importsFile`" del /q `"$importsFile`"",
    "dumpbin /nologo /dependents `"$exePath`" > `"$importsFile`" 2>nul",
    "exit /b 0"
)
Set-Content -Path $bat -Value $batLines -Encoding ASCII
Write-Host "  батник:        $bat"

Write-Host ""
Write-Host "== $buildCmd =="
if (Test-Path $buildLog) { Remove-Item $buildLog -Force }
# cargo stderr is merged into stdout at the cmd level: then PowerShell sees a regular
# stream and the exit code stays cmd's (and cargo's). We write the log ourselves (not via
# Tee-Object): Tee-Object in PowerShell 5.1 has no -Encoding, and the file would come out
# UTF-16.
[System.IO.File]::WriteAllText($buildLog, "")
$buildRc = Invoke-Native {
    & cmd.exe /c "`"$bat`" 2>&1" | ForEach-Object {
        $line = "$_"
        Write-Host $line
        [System.IO.File]::AppendAllText($buildLog, $line + [Environment]::NewLine)
    }
}
if ($buildRc -ne 0) {
    Fail "cargo build завершился с кодом $buildRc (лог: $buildLog)"
}
if (-not (Test-Path $exePath)) { Fail "после сборки нет $exePath" }

$exeSize = (Get-Item $exePath).Length
$exeHash = (Get-FileHash -Algorithm SHA256 $exePath).Hash.ToLower()
Write-Host ""
Write-Host ("exe: {0} ({1:N0} байт), sha256 {2}" -f $exePath, $exeSize, $exeHash)

# --- PE: Console (Subsystem=3) + imports -----------------------------------
Write-Host ""
Write-Host "== проверка PE =="
$pe = Get-PeInfo $exePath
$peName = switch ($pe.Subsystem) { 2 { "Windows GUI" } 3 { "Windows Console" } default { "?" } }
Write-Host ("PE: {0} — {1} {2}, Subsystem={3} ({4})" -f $exePath, $pe.Magic, $pe.Machine, $pe.Subsystem, $peName)
if ($pe.Machine -ne "x64") { Fail "ожидался PE x64, получено $($pe.Machine)" }
if ($pe.Magic -ne "PE32+") { Fail "ожидался PE32+, получено $($pe.Magic)" }
if ($pe.Subsystem -ne 3) {
    # The windows subsystem is enabled only by the sdl feature (main.rs), while the console
    # build of the client uses --no-default-features — otherwise --help/--demo
    # do not print to the terminal/CI.
    Fail "консольный бинарь клиента должен быть Console (Subsystem=3), получено $($pe.Subsystem) ($peName)"
}

$dlls = @()
if (Test-Path $importsFile) {
    $dlls = @(Get-Content $importsFile -Encoding UTF8 |
        Select-String -Pattern '^\s{4}([A-Za-z0-9_.\-]+\.dll)\s*$' -AllMatches |
        ForEach-Object { $_.Matches[0].Groups[1].Value } |
        Sort-Object -Unique)
}
if ($dlls.Count -eq 0) {
    Write-Host "импорты: dumpbin не дал списка (см. $importsFile)" -ForegroundColor Yellow
} else {
    Write-Host ("импорты: {0}" -f ($dlls -join ", "))
    $mingw = @($dlls | Where-Object { $_ -match 'libgcc|libwinpthread' })
    if ($mingw.Count -gt 0) {
        Fail ("mingw-рантайм в импортах: " + ($mingw -join ", "))
    }
    if (-not ($dlls | Where-Object { $_ -match '^(api-ms-win-crt|ucrtbase|vcruntime|msvcp)' })) {
        Write-Host "предупреждение: в импортах нет CRT (vcruntime140/ucrtbase) — проверь /MD-линковку" -ForegroundColor Yellow
    }
}

# --- native run: --help and --demo SAVE1.CAM --turns 1 ----------------------
$helpOk = $false
$demoOk = $false
$demoLine = ""
$save = Join-Path $root "max-original\SAVE1.CAM"

if ($SkipSmoke) {
    Write-Host ""
    Write-Host "прогон: пропущен (-SkipSmoke)"
} else {
    Write-Host ""
    Write-Host "== прогон нативно: --help =="
    if (Test-Path $helpFile) { Remove-Item $helpFile -Force }
    $helpRc = Invoke-Native { & cmd.exe /c "`"$exePath`" --help > `"$helpFile`" 2>&1" }
    $help = if (Test-Path $helpFile) { Get-Content $helpFile -Raw -Encoding UTF8 } else { "" }
    if ($helpRc -ne 0 -or $help -notmatch "ИСПОЛЬЗОВАНИЕ") {
        Fail "max-brh.exe --help не отработал (rc=$helpRc):`n$help"
    }
    $helpOk = $true
    Write-Host ("--help: OK (rc=0, {0} строк, «ИСПОЛЬЗОВАНИЕ» найдено)" -f ($help -split "`r?`n").Count)

    Write-Host ""
    Write-Host "== прогон нативно: --demo max-original\SAVE1.CAM --turns 1 =="
    if (-not (Test-Path $save)) { Fail "нет $save — нечего прогонять в --demo (нужен -SkipSmoke)" }
    if (Test-Path $demoLog) { Remove-Item $demoLog -Force }
    Push-Location $root
    try {
        $demoRc = Invoke-Native { & cmd.exe /c "`"$exePath`" --demo max-original\SAVE1.CAM --turns 1 > `"$demoLog`" 2>&1" }
    } finally {
        Pop-Location
    }
    $demoOut = if (Test-Path $demoLog) { Get-Content $demoLog -Raw -Encoding UTF8 } else { "" }
    $demoLine = (($demoOut -split "`r?`n") | Where-Object { $_ -match '^ticks=' } | Select-Object -First 1)
    if ($demoRc -ne 0) { Fail "max-brh.exe --demo завершился с кодом ${demoRc}:`n$demoOut" }
    if (-not ($demoOut -match '(?m)^ticks=1\s')) { Fail "--demo не напечатал ticks=1:`n$demoOut" }
    if (-not ($demoOut -match 'state_digest=0x[0-9A-Fa-f]{2,4}')) { Fail "--demo без state_digest:`n$demoOut" }
    $demoOk = $true
    Write-Host "demo: OK (rc=0)"
    Write-Host "  $demoLine"

    # Single run log (replaces build/smoke-logs/windows-wine-smoke.log).
    $smokeLines = @(
        "== M.A.X. Rust client $version — нативный MSVC (консольная сборка клиента) ==",
        "exe:    $exePath",
        "sha256: $exeHash",
        "target: x86_64-pc-windows-msvc (native, без wine/mingw)",
        "",
        "== 1/2: max-brh.exe --help ==",
        $help.TrimEnd(),
        "EXIT_HELP=$helpRc",
        "",
        "== 2/2: max-brh.exe --demo max-original\SAVE1.CAM --turns 1 ==",
        $demoOut.TrimEnd(),
        "EXIT_DEMO=$demoRc",
        "",
        "NATIVE: ok (--help + --demo 1 тик)"
    )
    Set-Content -Path $smokeLog -Value $smokeLines -Encoding UTF8
    Write-Host "  лог: $smokeLog"
}

if ($NoPackage) {
    Write-Host ""
    Write-Host "Готово (без упаковки): $exePath"
    exit 0
}

# --- packaging --------------------------------------------------------------
Write-Host ""
Write-Host "== упаковка $bundleName =="
New-Item -ItemType Directory -Force -Path $Out | Out-Null
if (Test-Path $bundle) { Remove-Item $bundle -Recurse -Force }
New-Item -ItemType Directory -Path $bundle | Out-Null
Copy-Item $exePath (Join-Path $bundle "max-brh.exe")
Copy-Item (Join-Path $root "LICENSE") $bundle
Copy-Item (Join-Path $root "CHANGELOG.md") $bundle

$readme = @"
M.A.X. Rust client $version — Windows x86_64 (консольная сборка клиента, native MSVC build)
==========================================================================

Собран нативно на Windows-хосте тулчейном MSVC (Visual Studio Build Tools,
x64 + Windows SDK), без Docker и без mingw:
  $buildCmd
target: x86_64-pc-windows-msvc
Без фич sdl* и sound: окно SDL3 и звук в этот бинарь не входят (см. ниже).
Клиент не содержит данных игры: нужны MAX.RES, PATCHES.RES, COLOR.PAL,
*.WRL и *.SPW из оригинальной M.A.X. (собственность Interplay Productions).

Запуск (консольная сборка)
-----------------
  max-brh.exe --help
  max-brh.exe --demo SAVE1.CAM --turns 5          # прогон сейва → digest
  max-brh.exe --new-game --world 12 --seed 42 --out frame.ppm
  max-brh.exe --connect HOST:8082 --join GAME_ID  # Horizon gRPC + ENet, без окна

Запуск без флагов (GUI) в этой консольной сборке недоступен: клиент скажет,
что нужна сборка с feature sdl (она входит в default-features max_app), и
предложит консольные флаги выше. Интерактивная сборка — отдельный артефакт
max_app-$version-windows-x86_64-ui.zip (build-windows-ui.ps1, SDL3 + звук).

Подсистема этого exe — Console (Subsystem=3), поэтому --help/--demo/--new-game
печатают в терминал (и в CI); окна у бинаря нет.

Данные игры указываются флагами:
  --data-dir <каталог>     MAX.RES и COLOR.PAL
  --assets-dir <каталог>   PATCHES.RES
  --world-dir <каталог>    *.WRL (если не рядом с MAX.RES)
  --max-res/--patches/--palette — точные пути

Зависимости
-----------
  vcruntime140.dll (Visual C++ Redistributable 2015-2022 x64) — CRT-рантайм
  MSVC, которым собран бинарь. На Windows 10/11 он почти всегда уже стоит;
  если система пишет «не найден VCRUNTIME140.dll» — поставь
  https://aka.ms/vs/17/release/vc_redist.x64.exe
  (это единственная не-системная зависимость: SDL3.dll и mingw-рантайма
  libgcc_s_seh-1.dll / libwinpthread-1.dll здесь нет).
  Остальные импорты — системные DLL Windows: kernel32, ws2_32, winmm,
  bcryptprimitives, ucrtbase, api-ms-win-crt-*.
  Horizon-эндпоинт — plaintext http://, TLS-стек в бинарь не входит.

Документация
------------
  README.txt (этот файл), CHANGELOG.md, LICENSE.
  Полный чек-лист релиза — max-brh-docs/development/release-checklist.md в
  репозитории (§6 — Windows x64: консольная сборка нативно на MSVC и игровая сборка).

Лицензия
--------
  Код клиента — MIT (см. LICENSE). M.A.X. (C) 1996 Interplay Productions;
  данные игры лицензией клиента не покрываются.
"@
Set-Content -Path (Join-Path $bundle "README.txt") -Value $readme -Encoding UTF8

Push-Location $bundle
try {
    $files = @("max-brh.exe", "README.txt", "LICENSE", "CHANGELOG.md")
    $sums = foreach ($file in $files) {
        $hash = (Get-FileHash -Algorithm SHA256 $file).Hash.ToLower()
        "$hash  $file"
    }
    Set-Content -Path "SHA256SUMS" -Value $sums -Encoding ASCII
} finally {
    Pop-Location
}

if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $bundle -DestinationPath $zip -CompressionLevel Optimal
$zipHash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
Set-Content -Path "$zip.sha256" -Value "$zipHash  $bundleName.zip" -Encoding ASCII

Write-Host ""
Write-Host "Готово:"
Write-Host "  $bundle\"
Write-Host ("    max-brh.exe  {0:N0} байт, sha256 {1}" -f $exeSize, $exeHash)
Write-Host "    README.txt, LICENSE, CHANGELOG.md, SHA256SUMS"
Write-Host "  $zip"
Write-Host ("    {0:N0} байт, sha256 {1}" -f (Get-Item $zip).Length, $zipHash)
Write-Host ""
if ($helpOk -and $demoOk) {
    Write-Host "Проверки: PE Subsystem=3 (console), --help OK, --demo --turns 1 OK ($(("$demoLine").Trim()))"
    Write-Host "Лог прогона: $smokeLog"
} else {
    Write-Host "Проверки: PE Subsystem=3 (console); прогон пропущен (-SkipSmoke)" -ForegroundColor Yellow
}
