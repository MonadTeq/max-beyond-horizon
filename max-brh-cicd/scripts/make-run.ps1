# Prepares a portable run directory for max-port.exe and points it at the original game data.
# Usage: pwsh -File build\make-run.ps1 [-TargetDir <dir>] [-HostPort <n>] [-HostAddress <ip>]
param(
    [string]$TargetDir = "build\run",
    [int]$HostPort = 31554,
    [string]$HostAddress = "127.0.0.1"
)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot | Split-Path -Parent
Set-Location $root

$exe = "build\cross-winxp-x64-debug\max-port.exe"
if (-not (Test-Path $exe)) { throw "Build output not found: $exe (build first)" }

$run = Join-Path $root $TargetDir
if (Test-Path $run) { Remove-Item $run -Recurse -Force }
New-Item -ItemType Directory -Path $run | Out-Null

Copy-Item $exe $run
Copy-Item "build\cross-winxp-x64-debug\max-port.debug" $run -ErrorAction SilentlyContinue
foreach ($f in @(".portable", "PATCHES.RES", "settings.json", "language.json", "help.json", "maxint.json", "LICENSE")) {
    Copy-Item (Join-Path $root "assets\$f") (Join-Path $run $f)
}

$settingsPath = Join-Path $run "settings.json"
$settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
$settings.settings.SETUP.game_data = "D:/dev/max-port-debug/original/MAX"
$settings.settings.SETUP.intro_movie = 0
$settings.settings.SETUP.disable_movie = 1
$settings.settings.SETUP.disable_music = 1
$settings.settings.SETUP.disable_voice = 1
$settings.settings.NETWORK_SETTINGS.host_port = $HostPort
$settings.settings.NETWORK_SETTINGS.host_address = $HostAddress
$settings | ConvertTo-Json -Depth 10 | Set-Content $settingsPath -Encoding UTF8

Write-Host "Run directory prepared: $run"
Write-Host "game_data -> $($settings.settings.SETUP.game_data)"
Write-Host "host_port -> $HostPort"
