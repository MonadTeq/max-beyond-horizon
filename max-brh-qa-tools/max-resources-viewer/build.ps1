# Сборка max-resources-viewer (Windows) с автокопированием SDL3.dll.
# Ищет SDL3: vcpkg ($env:VCPKG_ROOT или D:\dev\toolchain\vcpkg / C:\vcpkg),
# затем локальную копию build\sdl3-3.4.20, затем бандл клиента.
# Использование: powershell -ExecutionPolicy Bypass -File build.ps1 [-Debug] [-NoPackage]
param(
    [switch]$Debug,
    [switch]$NoPackage
)

$ErrorActionPreference = 'Stop'
function Fail([string]$m) { Write-Host "ОШИБКА: $m" -ForegroundColor Red; exit 1 }

$root = Split-Path -Parent $PSScriptRoot            # tools/
$repo = Split-Path -Parent $root                    # корень репозитория
$here = Join-Path $root 'max-resources-viewer'
$profile = if ($Debug) { 'debug' } else { 'release' }
$flags = if ($Debug) { '' } else { '--release' }

# --- Visual Studio (MSVC x64) ------------------------------------------------
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) { Fail 'нет vswhere.exe — Visual Studio не установлена' }
$vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsPath) { Fail 'не найдена VS с workload Desktop development with C++' }
$vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) { Fail "нет $vcvars" }

# --- SDL3 (lib для линковки + dll для запуска) ------------------------------
$candidates = @()
if ($env:VCPKG_ROOT) { $candidates += $env:VCPKG_ROOT }
$candidates += "D:\dev\toolchain\vcpkg", "C:\vcpkg"
$vcpkg = $null
foreach ($candidate in $candidates) {
    if ($candidate -and (Test-Path (Join-Path $candidate 'installed\x64-windows\lib\SDL3.lib'))) { $vcpkg = $candidate; break }
}
$sdlLib = $null; $sdlDll = $null
if ($vcpkg) {
    $sdlLib = Join-Path $vcpkg 'installed\x64-windows\lib'
    $sdlDll = Join-Path $vcpkg 'installed\x64-windows\bin\SDL3.dll'
} else {
    $local = Join-Path $repo 'build\sdl3-3.4.20\SDL3-3.4.20\lib\x64'
    if (Test-Path (Join-Path $local 'SDL3.dll')) { $sdlLib = $local; $sdlDll = Join-Path $local 'SDL3.dll' }
    else { Write-Host 'WARN: SDL3 не найден (vcpkg/local) — линковка sdl-vcpkg может не сработать.' }
}

# --- cargo -------------------------------------------------------------------
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (-not (Test-Path $cargo)) {
    $cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargoCmd) { Fail 'cargo не найден — поставь Rust (https://win.rustup.rs)' }
    $cargo = $cargoCmd.Source
}
$cargoBin = Split-Path -Parent $cargo

Write-Host "build: max-resources-viewer → $profile"
Write-Host "  VS:    $vsPath"
Write-Host "  SDL3:  $(if ($sdlLib) { $sdlLib } else { 'НЕ НАЙДЕН' })"
Write-Host "  cargo: $cargo"

# vcvars64 через .bat (set "VAR=value" — без пробела в значении)
$bat = Join-Path $env:TEMP 'max-res-viewer-build.bat'
$libLine = if ($sdlLib) { "set `"LIB=%LIB%;$sdlLib`"" } else { 'rem no-sdl-lib' }
$vcpkgLine = if ($vcpkg) { "set `"VCPKG_ROOT=$vcpkg`"" } else { 'rem no-vcpkg' }
$batLines = @(
    '@echo off',
    "call `"$vcvars`" >nul",
    "set `"PATH=$cargoBin;%PATH%`"",
    $vcpkgLine,
    $libLine,
    "cd /d `"$here`"",
    "`"$cargo`" build $flags --features sdl-vcpkg",
    'exit /b %ERRORLEVEL%'
)
Set-Content -Path $bat -Value $batLines -Encoding ASCII
cmd /c "`"$bat`""
if ($LASTEXITCODE -ne 0) { Fail "cargo build: exit $LASTEXITCODE" }

# --- подсистема PE: GUI (2), иначе двойной клик по .exe откроет консоль ------
$exePath = Join-Path $here "target\$profile\max-resources-viewer.exe"
if (-not (Test-Path $exePath)) { Fail "после сборки нет $exePath" }
$peScript = Join-Path $repo 'build\scripts\pe-subsystem.ps1'
& powershell -NoProfile -ExecutionPolicy Bypass -File $peScript -Path $exePath -Expected 2
if ($LASTEXITCODE -ne 0) { Fail "PE-подсистема $exePath не GUI (ожидалось Subsystem=2)" }

# --- DLL рядом с exe + пакет -------------------------------------------------
$targetDir = Join-Path $here "target\$profile"
if ($sdlDll -and (Test-Path $sdlDll)) {
    Copy-Item $sdlDll (Join-Path $targetDir 'SDL3.dll') -Force
    Write-Host "SDL3.dll → $targetDir"
} else {
    $bundle = Join-Path $repo 'release\max_app-1.0.0-windows-x86_64-ui\SDL3.dll'
    if (Test-Path $bundle) { Copy-Item $bundle (Join-Path $targetDir 'SDL3.dll') -Force; Write-Host "SDL3.dll (из бандла клиента) → $targetDir" }
    else { Write-Host 'WARN: SDL3.dll не найден — положите рядом с exe вручную.' }
}

if (-not $NoPackage) {
    $dist = Join-Path $here 'dist'
    New-Item -ItemType Directory -Force $dist | Out-Null
    Copy-Item (Join-Path $targetDir 'max-resources-viewer.exe') $dist -Force
    if (Test-Path (Join-Path $targetDir 'SDL3.dll')) { Copy-Item (Join-Path $targetDir 'SDL3.dll') $dist -Force }
    Write-Host "пакет: $dist"
}
Write-Host 'OK'
