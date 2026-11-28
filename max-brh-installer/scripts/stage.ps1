# Stages the portable game catalog for the NSIS installer.
#
# Layout (distro-installers.md §B):
#   <OutDir>/game.exe          renamed release binary
#   <OutDir>/SDL3.dll
#   <OutDir>/data/             509 verified files from data-inventory.json
#   <OutDir>/remaster/         mandatory HD remaster (target/remaster)
#   <OutDir>/saves/            empty, created here for the installer
#   <OutDir>/missions/         empty, created here for the installer
#   <OutDir>/settings.jsonc    template (the installer keeps an existing file)
#   <OutDir>/LICENSE, THIRD_PARTY_LICENSES.txt, README.txt, CHANGELOG.md
#
# The script only reads the oracles (max-original/, max-cpp-port/). Staging lives
# outside git by default (.tmp/) and every copied data file is checked against the
# inventory size and sha256.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
    [string]$OutDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path '.tmp\max-brh-installer\stage\game'),
    [string]$Inventory = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..')).Path 'data-inventory.json'),
    [string]$ReleaseDir = '',
    [string]$RemasterDir = '',
    [switch]$SkipHash,
    [switch]$KeepTimestamps
)

$ErrorActionPreference = 'Stop'

function Resolve-Required([string]$Path, [string]$What) {
    if (-not (Test-Path $Path)) {
        throw "$What not found: $Path"
    }
    return (Resolve-Path $Path).Path
}

$RepoRoot = Resolve-Required $RepoRoot 'repository root'
$Inventory = Resolve-Required $Inventory 'data inventory'
if (-not $ReleaseDir) { $ReleaseDir = Join-Path $RepoRoot 'max-brh\target\release' }
if (-not $RemasterDir) { $RemasterDir = Join-Path $RepoRoot 'target\remaster' }
$ReleaseDir = Resolve-Required $ReleaseDir 'release binary directory'
$RemasterDir = Resolve-Required $RemasterDir 'remaster directory'

$gameBinary = Resolve-Required (Join-Path $ReleaseDir 'max-brh.exe') 'release binary (max-brh.exe)'
$sdlDll = Resolve-Required (Join-Path $ReleaseDir 'SDL3.dll') 'SDL3 runtime (SDL3.dll)'
$remasterManifest = Resolve-Required (Join-Path $RemasterDir 'remaster-manifest.json') 'remaster manifest'
$remasterAssets = @(Get-ChildItem $RemasterDir -File -Filter '*.ktx2')
if ($remasterAssets.Count -eq 0) {
    throw "no .ktx2 files in remaster directory: $RemasterDir"
}

$inventoryJson = Get-Content $Inventory -Raw | ConvertFrom-Json
$files = @($inventoryJson.files)
$expectedCount = [int]$inventoryJson.totals.files
$expectedBytes = [long]$inventoryJson.totals.bytes

if ($files.Count -ne $expectedCount) {
    throw "inventory lists $($files.Count) files, totals say $expectedCount"
}

# Source roots, in priority order. PATCHES.RES and maxint.json live in the C++
# port assets; the original game files live in max-original (the Interplay oracle).
$sourceRoots = @(
    (Join-Path $RepoRoot 'max-original'),
    (Join-Path $RepoRoot 'max-cpp-port\assets'),
    (Join-Path $RepoRoot 'assets'),
    (Join-Path $RepoRoot 'bin\linux-server-template')
) | Where-Object { Test-Path $_ }

if (Test-Path $OutDir) {
    Remove-Item $OutDir -Recurse -Force
}
New-Item -ItemType Directory -Force $OutDir | Out-Null
$dataDir = Join-Path $OutDir 'data'
$remasterOut = Join-Path $OutDir 'remaster'
New-Item -ItemType Directory -Force $dataDir, $remasterOut, (Join-Path $OutDir 'saves'), (Join-Path $OutDir 'missions') | Out-Null

$staged = New-Object System.Collections.Generic.List[object]
$copiedBytes = 0L
$index = 0

foreach ($entry in ($files | Sort-Object path)) {
    $index++
    $relative = [string]$entry.path
    $source = $null
    foreach ($root in $sourceRoots) {
        $candidate = Join-Path $root $relative
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            $source = $candidate
            break
        }
    }
    if (-not $source) {
        throw "source file not found for '$relative' (tried: $($sourceRoots -join ', '))"
    }

    $item = Get-Item -LiteralPath $source
    if ([long]$item.Length -ne [long]$entry.size) {
        throw "size mismatch for '$relative': $($item.Length) != $($entry.size)"
    }

    if (-not $SkipHash) {
        $hash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($hash -ne [string]$entry.sha256) {
            throw "sha256 mismatch for '$relative': $hash != $($entry.sha256)"
        }
    }

    $destination = Join-Path $dataDir $relative
    $destinationParent = Split-Path -Parent $destination
    if (-not (Test-Path $destinationParent)) {
        New-Item -ItemType Directory -Force $destinationParent | Out-Null
    }
    Copy-Item -LiteralPath $source -Destination $destination -Force

    if (-not $KeepTimestamps) {
        [System.IO.File]::SetLastWriteTimeUtc($destination, [datetime]'2026-11-28T00:00:00Z')
    }

    $copiedBytes += [long]$entry.size
    $staged.Add([pscustomobject]@{ path = $relative; size = [long]$entry.size; class = [string]$entry.class })
}

if ($staged.Count -ne $expectedCount) {
    throw "staged $($staged.Count) data files, expected $expectedCount"
}
if ($copiedBytes -ne $expectedBytes) {
    throw "staged $copiedBytes data bytes, expected $expectedBytes"
}

# Mandatory remaster.
foreach ($asset in (Get-ChildItem $RemasterDir -File)) {
    $destination = Join-Path $remasterOut $asset.Name
    Copy-Item -LiteralPath $asset.FullName -Destination $destination -Force
    if (-not $KeepTimestamps) {
        [System.IO.File]::SetLastWriteTimeUtc($destination, [datetime]'2026-11-28T00:00:00Z')
    }
}

# Binaries: the package ships game.exe, not max-brh.exe.
$gameDestination = Join-Path $OutDir 'game.exe'
Copy-Item -LiteralPath $gameBinary -Destination $gameDestination -Force
Copy-Item -LiteralPath $sdlDll -Destination (Join-Path $OutDir 'SDL3.dll') -Force

# Documentation and license notices.
Copy-Item -LiteralPath (Resolve-Required (Join-Path $RepoRoot 'LICENSE') 'LICENSE') -Destination (Join-Path $OutDir 'LICENSE') -Force
Copy-Item -LiteralPath (Resolve-Required (Join-Path $RepoRoot 'CHANGELOG.md') 'CHANGELOG.md') -Destination (Join-Path $OutDir 'CHANGELOG.md') -Force
Copy-Item -LiteralPath (Resolve-Required (Join-Path $RepoRoot 'README.md') 'README.md') -Destination (Join-Path $OutDir 'README.txt') -Force

$sdlLicense = Resolve-Required (Join-Path $RepoRoot 'max-brh-vendor\SDL\LICENSE.txt') 'SDL3 license'
$enetLicense = Resolve-Required (Join-Path $RepoRoot 'max-brh\crates\max_net\vendor\enet\LICENSE') 'ENet license'
$thirdParty = @"
M.A.X.: Beyond Rust Horizon 0.1.28a - third-party licenses
===============================================================================

This package contains the following third-party software. The full license
texts are reproduced below or in the distribution's source archive.

1. SDL3 (Simple DirectMedia Layer 3) - zlib license
   Copyright (C) 1997-2025 Sam Lantinga and SDL contributors.
   Project: https://www.libsdl.org/

2. ENet 1.3.18 - MIT license
   Copyright (C) 2002-2024 Lee Salzman and ENet contributors.
   Project: https://github.com/lsalzman/enet

3. Rust crates used by the client (MIT and/or Apache-2.0), including
   anyhow, serde, serde_json, tokio, tracing, prost, pentagram, egui (if
   enabled by the build), rodio/cpal (sound) and windows-sys. The complete
   license texts are part of the distribution's source archive and the
   crates' published packages.

4. Game data
   M.A.X.: Mechanized Assault & Exploration is a trademark of Interplay
   Entertainment Corp. The original game assets (c) Interplay 1996. This
   project is a free, non-commercial fan remake, is not affiliated with or
   endorsed by the rights holder, and redistributes the assets without
   commercial intent.

--------------------------------------------------------------------------------
SDL3 LICENSE
--------------------------------------------------------------------------------
$((Get-Content $sdlLicense -Raw).Trim())

--------------------------------------------------------------------------------
ENet LICENSE
--------------------------------------------------------------------------------
$((Get-Content $enetLicense -Raw).Trim())
"@
Set-Content -LiteralPath (Join-Path $OutDir 'THIRD_PARTY_LICENSES.txt') -Value $thirdParty -Encoding UTF8

$settingsTemplate = Join-Path $RepoRoot 'settings.jsonc'
if (Test-Path $settingsTemplate) {
    Copy-Item -LiteralPath $settingsTemplate -Destination (Join-Path $OutDir 'settings.jsonc') -Force
}

if (-not $KeepTimestamps) {
    Get-ChildItem $OutDir -Recurse -File | ForEach-Object {
        [System.IO.File]::SetLastWriteTimeUtc($_.FullName, [datetime]'2026-11-28T00:00:00Z')
    }
}

$stagingReport = [pscustomobject]@{
    schema = 1
    version = '0.1.28a'
    generated_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    inventory = [pscustomobject]@{
        files = $expectedCount
        bytes = $expectedBytes
        hash_verified = (-not $SkipHash)
    }
    data_files = $staged.Count
    data_bytes = $copiedBytes
    classes = @($staged | Group-Object class | Sort-Object Name | ForEach-Object {
        [pscustomobject]@{ class = $_.Name; files = $_.Count; bytes = [long](($_.Group | Measure-Object -Property size -Sum).Sum) }
    })
    remaster_files = @(Get-ChildItem $remasterOut -File).Count
    game_exe = [pscustomobject]@{
        path = 'game.exe'
        bytes = (Get-Item $gameDestination).Length
        sha256 = (Get-FileHash -LiteralPath $gameDestination -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    sdl3_dll_bytes = (Get-Item (Join-Path $OutDir 'SDL3.dll')).Length
    source_binary = $gameBinary
    git_head = (& git -C $RepoRoot rev-parse HEAD 2>$null)
}
$reportPath = Join-Path (Split-Path -Parent $OutDir) 'staging-report.json'
$stagingReport | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $reportPath -Encoding UTF8

"staging: $OutDir"
"data: $($staged.Count) files, $('{0:N1}' -f ($copiedBytes / 1MB)) MiB, hash_verified=$(-not $SkipHash)"
"remaster: $($stagingReport.remaster_files) files"
"game.exe: $([math]::Round($stagingReport.game_exe.bytes / 1MB, 2)) MiB sha256=$($stagingReport.game_exe.sha256.Substring(0, 16))..."
"report: $reportPath"
