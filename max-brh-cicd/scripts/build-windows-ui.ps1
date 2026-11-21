#requires -version 5.1
<#
.SYNOPSIS
  Нативная Windows-сборка M.A.X. Rust-клиента (max_app) с окном SDL3 и звуком,
  упаковка в bin/bin/release/max_app-<ver>-windows-x86_64-ui/.

.DESCRIPTION
  Нативный двойник build-windows-console.ps1 (тот собирает headless на MSVC,
  без SDL3 и звука): этот скрипт собирает ИНТЕРАКТИВНЫЙ клиент на Windows-хосте —
  SDL3 берётся из vcpkg и линкуется MSVC, звук — rodio/cpal (WASAPI, отдельных
  DLL не нужно).

  Требуется (всё ставится без прав администратора, кроме самого Visual Studio):
    * Visual Studio (Build Tools или Community) с workload "Desktop development
      with C++": MSVC x64, Windows SDK, CMake — ищется через vswhere;
    * Rust 1.94.0 (rustup, %USERPROFILE%\.cargo) — target x86_64-pc-windows-msvc;
    * vcpkg с установленным sdl3:x64-windows
      (git clone https://github.com/microsoft/vcpkg D:\dev\toolchain\vcpkg;
       D:\dev\toolchain\vcpkg\bootstrap-vcpkg.bat;
       D:\dev\toolchain\vcpkg\vcpkg.exe install sdl3:x64-windows).

  Сборка не засоряет max-brh/target (там живут Linux/Docker-артефакты):
  используется CARGO_TARGET_DIR=<repo>\bin\target.

.PARAMETER VcpkgRoot
  Каталог vcpkg. По умолчанию $env:VCPKG_ROOT, затем D:\dev\toolchain\vcpkg,
  затем C:\vcpkg.

.PARAMETER Out
  Каталог артефактов. По умолчанию <repo>\bin\release.

.PARAMETER Features
  Набор features max_app. По умолчанию "sdl-vcpkg,sound". Скрипт собирает с
  --no-default-features: иначе к явным фичам добавился бы default
  `sdl-from-source` (сборка SDL3 через CMake) вместо vcpkg.

.PARAMETER NoPackage
  Только собрать (и проверить --help), не упаковывать.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-ui.ps1

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-ui.ps1 -NoPackage
#>
param(
    [string]$VcpkgRoot = "",
    [string]$Out = "",
    [string]$Features = "sdl-vcpkg,sound",
    [switch]$NoPackage
)

$ErrorActionPreference = "Stop"

function Fail([string]$message) {
    Write-Host "build-windows-ui: $message" -ForegroundColor Red
    exit 1
}

# <repo>\max-brh-cicd\scripts -> <repo>
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Out) { $Out = Join-Path $root "bin\release" }

# --- версия workspace -------------------------------------------------------
$manifest = Join-Path $root "max-brh\Cargo.toml"
if (-not (Test-Path $manifest)) { Fail "нет $manifest" }
$version = (Select-String -Path $manifest -Pattern '^version = "([0-9]+\.[0-9]+\.[0-9]+)"').Matches.Groups[1].Value
if (-not $version) { Fail "не удалось прочитать версию из $manifest" }

# --- Visual Studio (MSVC x64 + Windows SDK + CMake) -------------------------
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) { Fail "нет vswhere.exe — Visual Studio не установлена" }
$vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsPath) { Fail "не найдена VS с workload 'Desktop development with C++' (VC.Tools.x86.x64)" }
$vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) { Fail "нет $vcvars" }

# --- vcpkg с SDL3 -----------------------------------------------------------
$candidates = @()
if ($VcpkgRoot) { $candidates += $VcpkgRoot }
elseif ($env:VCPKG_ROOT) { $candidates += $env:VCPKG_ROOT }
$candidates += "D:\dev\toolchain\vcpkg", "C:\vcpkg"
$vcpkg = $null
foreach ($candidate in $candidates) {
    if ($candidate -and (Test-Path (Join-Path $candidate "installed\x64-windows\lib\SDL3.lib"))) {
        $vcpkg = $candidate
        break
    }
}
if (-not $vcpkg) {
    Fail ("не найден vcpkg с sdl3:x64-windows (искал: " + ($candidates -join ", ") + "). " +
          "Установка: git clone https://github.com/microsoft/vcpkg D:\dev\toolchain\vcpkg; " +
          "D:\dev\toolchain\vcpkg\bootstrap-vcpkg.bat; " +
          "D:\dev\toolchain\vcpkg\vcpkg.exe install sdl3:x64-windows")
}
$sdlLib = Join-Path $vcpkg "installed\x64-windows\lib"
$sdlBin = Join-Path $vcpkg "installed\x64-windows\bin"
$sdlDll = Join-Path $sdlBin "SDL3.dll"
if (-not (Test-Path $sdlDll)) { Fail "нет $sdlDll" }

# --- cargo ------------------------------------------------------------------
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path $cargo)) {
    $cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargoCmd) { Fail "cargo не найден: поставь Rust 1.94.0 (https://win.rustup.rs)" }
    $cargo = $cargoCmd.Source
}
$cargoBin = Split-Path -Parent $cargo

$targetDir = Join-Path $root "bin\target"
$exePath = Join-Path $targetDir "release\max-brh.exe"
$bundleName = "max_app-$version-windows-x86_64-ui"
$bundle = Join-Path $Out $bundleName
$zip = Join-Path $Out "$bundleName.zip"

Write-Host "build-windows-ui: M.A.X. Rust client $version → Windows x86_64 (UI: SDL3 + звук)"
Write-Host "  Visual Studio: $vsPath"
Write-Host "  vcpkg:         $vcpkg"
Write-Host "  cargo:         $cargo"
Write-Host "  features:      $Features"
Write-Host "  target dir:    $targetDir"

# vcvars64 нужен линкеру (link.exe) и Windows SDK; переменные окружения ставим
# через .bat: в cmd `set VAR=value && ...` захватывает пробел в значение,
# поэтому только `set "VAR=value"`.
$bat = Join-Path $root "build\win-msvc-build.bat"
$batLines = @(
    '@echo off',
    "call `"$vcvars`" >nul",
    "set `"PATH=$cargoBin;%PATH%`"",
    "set `"VCPKG_ROOT=$vcpkg`"",
    "set `"LIB=%LIB%;$sdlLib`"",
    "set `"CARGO_TARGET_DIR=$targetDir`"",
    "cd /d `"$root\max-brh`"",
    "`"$cargo`" build --release --locked -p max_app --no-default-features --features $Features",
    "exit /b %ERRORLEVEL%"
)
Set-Content -Path $bat -Value $batLines -Encoding ASCII
Write-Host "  батник:        $bat"

Write-Host ""
Write-Host "== cargo build --release -p max_app --no-default-features --features $Features =="
& cmd.exe /c "`"$bat`""
$buildRc = $LASTEXITCODE
if ($buildRc -ne 0) { Fail "cargo build завершился с кодом $buildRc (лог выше)" }
if (-not (Test-Path $exePath)) { Fail "после сборки нет $exePath" }

# --- DLL рядом с exe, подсистема PE и проверка --help ----------------------
$exeDir = Split-Path -Parent $exePath
Copy-Item $sdlDll $exeDir -Force
$exeSize = (Get-Item $exePath).Length
Write-Host ("exe: {0} ({1:N0} байт), SDL3.dll рядом" -f $exePath, $exeSize)

# Gигровая сборка обязана иметь подсистему windows (Subsystem=2), иначе двойной клик
# откроет консоль. Проверяем PE-заголовок напрямую (без dumpbin/vcvars).
$peScript = Join-Path $PSScriptRoot "pe-subsystem.ps1"
& powershell -NoProfile -ExecutionPolicy Bypass -File $peScript -Path $exePath -Expected 2
if ($LASTEXITCODE -ne 0) { Fail "PE-подсистема $exePath не GUI (ожидалось Subsystem=2)" }

# Подсистема windows не подключается к консоли: `& $exe --help` в PowerShell не
# дождётся процесса и не соберёт вывод. cmd ждёт дочерний процесс, а
# перенаправление `>` всё равно отдаёт GUI-exe stdout — читаем справку из файла.
$helpFile = Join-Path $targetDir "max-brh-help.txt"
if (Test-Path $helpFile) { Remove-Item $helpFile -Force }
& cmd.exe /c "`"$exePath`" --help > `"$helpFile`" 2>&1"
$helpRc = $LASTEXITCODE
$help = if (Test-Path $helpFile) { Get-Content $helpFile -Raw -Encoding UTF8 } else { "" }
if ($helpRc -ne 0 -or $help -notmatch "ИСПОЛЬЗОВАНИЕ") {
    Fail "max-brh.exe --help не отработал (rc=$helpRc, Subsystem=2):`n$help"
}
Write-Host "проверка: PE Subsystem=2 (GUI), max-brh.exe --help — OK (GUI по умолчанию, --ui — alias)"

if ($NoPackage) {
    Write-Host ""
    Write-Host "Готово (без упаковки): $exePath"
    Write-Host "Запуск: `"$exePath`" (без флагов — GUI)"
    exit 0
}

# --- упаковка ---------------------------------------------------------------
Write-Host ""
Write-Host "== упаковка $bundleName =="
New-Item -ItemType Directory -Force -Path $Out | Out-Null
if (Test-Path $bundle) { Remove-Item $bundle -Recurse -Force }
New-Item -ItemType Directory -Path $bundle | Out-Null
Copy-Item $exePath $bundle
Copy-Item $sdlDll $bundle
Copy-Item (Join-Path $root "LICENSE") $bundle
Copy-Item (Join-Path $root "CHANGELOG.md") $bundle

$readme = @"
M.A.X. Rust client $version — Windows x86_64 (UI: SDL3 + звук)
==============================================================

Собран на Windows-хосте (MSVC + SDL3 из vcpkg):
  cargo build --release -p max_app --no-default-features --features $Features
Рядом с max-brh.exe лежит SDL3.dll — она обязательна, ставить ничего не нужно.
Звук — rodio/cpal (WASAPI), отдельных DLL не требует.

Окно без консоли
----------------
Это Gигровая сборка: подсистема Windows (Subsystem=2 в PE-заголовке), поэтому
двойной клик по max-brh.exe открывает только окно SDL3 — окно cmd не
появляется. Плата за это: stdout этой сборки не виден в терминале.
Для справки и headless-режимов есть Console-сборка
max_app-$version-windows-x86_64.zip; при необходимости вывод перенаправляется:
  max-brh.exe --help > help.txt

Запуск
------
  max-brh.exe                          # GUI: меню, создать/выбрать игру, партия
  max-brh.exe --ui                     # то же (совместимый alias)
  max-brh.exe --connect HOST:8082 --join GAME_ID --sdl
  max-brh.exe --help

Управление в игре: мышь (LMB — выбор/атака, RMB — движение/отмена),
стрелки/Enter — меню, End Turn — конец хода, Esc — выход.

Данные игры
-----------
Клиент не содержит данных игры: нужны MAX.RES, PATCHES.RES, COLOR.PAL,
*.WRL и *.SPW из оригинальной M.A.X. (собственность Interplay Productions).
Пути задаются флагами:
  --data-dir <каталог>     MAX.RES, COLOR.PAL, *.WRL, *.SPW, сейвы
  --assets-dir <каталог>   PATCHES.RES
  --save / --demo <файл>   конкретный сейв (*.CAM)
Если рядом с exe есть каталоги max-original\ и assets\, клиент поднимется до
них сам (как в дереве репозитория).

Сервер
------
Для партии нужен Horizon (gRPC, порт 8082) и headless-сервер сессии:
  docker compose up -d --build
Меню "Create game" создаёт игру и входит в неё.

Headless-вариант (без SDL/звука) — отдельный артефакт
bin/release/max_app-$version-windows-x86_64.zip.

Лицензия
--------
Код клиента — MIT (см. LICENSE). M.A.X. (C) 1996 Interplay Productions;
данные игры лицензией клиента не покрываются.
"@
Set-Content -Path (Join-Path $bundle "README.txt") -Value $readme -Encoding UTF8

Push-Location $bundle
try {
    $files = @("max-brh.exe", "SDL3.dll", "README.txt", "LICENSE", "CHANGELOG.md")
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

$exeHash = (Get-FileHash -Algorithm SHA256 (Join-Path $bundle "max-brh.exe")).Hash.ToLower()
Write-Host ""
Write-Host "Готово:"
Write-Host "  $bundle\"
Write-Host ("    max-brh.exe  {0:N0} байт, sha256 {1}" -f $exeSize, $exeHash)
Write-Host "    SDL3.dll, README.txt, LICENSE, CHANGELOG.md, SHA256SUMS"
Write-Host "  $zip"
Write-Host ("    sha256 {0}" -f $zipHash)
Write-Host ""
Write-Host "Запуск: `"$(Join-Path $bundle 'max-brh.exe')`" (без флагов — GUI)"
