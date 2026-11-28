# Builds MAX-BRH-0.1.28a-setup.exe: UI bitmaps -> staging -> makensis.
#
# Staging and generated UI assets live under .tmp/ (outside git). The installer
# artifact is written to max-brh-installer/dist/.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
    [string]$StageDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path '.tmp\max-brh-installer\stage\game'),
    [string]$UIDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path '.tmp\max-brh-installer\ui'),
    [string]$OutputDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..')).Path 'dist'),
    [string]$Makensis = '',
    [switch]$SkipStage,
    [switch]$SkipHash,
    [switch]$SkipUi
)

$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path $RepoRoot).Path
$Version = '0.1.28a'
$SetupName = "MAX-BRH-$Version-setup.exe"
$Nsi = Join-Path $RepoRoot 'max-brh-installer\installer.nsi'

if (-not $Makensis) {
    $candidates = @(
        (Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe'),
        (Join-Path $env:ProgramFiles 'NSIS\makensis.exe')
    ) | Where-Object { $_ -and (Test-Path $_) }
    if (-not $candidates) {
        throw 'makensis.exe not found; pass -Makensis <path>'
    }
    $Makensis = @($candidates)[0]
}

if (-not (Test-Path $Nsi)) { throw "NSIS script not found: $Nsi" }

if (-not $SkipUi) {
    & (Join-Path $PSScriptRoot 'prepare-ui.ps1') -RepoRoot $RepoRoot -OutDir $UIDir
}

if (-not $SkipStage) {
    & (Join-Path $PSScriptRoot 'stage.ps1') -RepoRoot $RepoRoot -OutDir $StageDir -SkipHash:$SkipHash
}

foreach ($required in @(
    (Join-Path $StageDir 'game.exe'),
    (Join-Path $StageDir 'data\MAX.RES'),
    (Join-Path $StageDir 'remaster\remaster-manifest.json'),
    (Join-Path $UIDir 'frame.bmp'),
    (Join-Path $UIDir 'panel.bmp'),
    (Join-Path $UIDir 'bar-fill.bmp'),
    (Join-Path $UIDir 'game-icon.ico')
)) {
    if (-not (Test-Path $required)) {
        throw "missing build input: $required"
    }
}

# Payload list and progress steps for the install page. The whole File sequence is
# generated here (base files, data, remaster) and every ~8 MiB of copied bytes is
# followed by a BrhProgress step whose percentage is the share of payload bytes
# already copied - the bar follows real copying instead of an arbitrary scale.
# The last generated step stays at 99%: installer.nsi reports 100% only after the
# last installation step (uninstaller + shortcut) has really completed.
$dataGroups = Join-Path $UIDir 'data-groups.nsh'

$baseFiles = @(
    'game.exe',
    'SDL3.dll',
    'LICENSE',
    'THIRD_PARTY_LICENSES.txt',
    'README.txt',
    'CHANGELOG.md'
)
$settingsFile = 'settings.jsonc'
$dataRoot = Join-Path $StageDir 'data'
$remasterRoot = Join-Path $StageDir 'remaster'

foreach ($name in ($baseFiles + $settingsFile)) {
    $path = Join-Path $StageDir $name
    if (-not (Test-Path -LiteralPath $path)) { throw "missing staged payload file: $path" }
}

function Get-PayloadFiles([string]$root) {
    if (-not (Test-Path -LiteralPath $root)) { throw "missing staged payload folder: $root" }
    return @(Get-ChildItem -LiteralPath $root -File -Recurse | Sort-Object FullName)
}

$dataFiles = Get-PayloadFiles $dataRoot
if ($dataFiles.Count -eq 0) { throw "no staged data files in $dataRoot" }
$remasterFiles = Get-PayloadFiles $remasterRoot
if ($remasterFiles.Count -eq 0) { throw "no staged remaster files in $remasterRoot" }

$totalBytes = 0.0
foreach ($name in ($baseFiles + $settingsFile)) {
    $totalBytes += [double](Get-Item -LiteralPath (Join-Path $StageDir $name)).Length
}
$totalBytes += [double](($dataFiles | Measure-Object -Property Length -Sum).Sum)
$totalBytes += [double](($remasterFiles | Measure-Object -Property Length -Sum).Sum)
if ($totalBytes -le 0) { throw "staged payload is empty: $StageDir" }

$lines = New-Object System.Collections.Generic.List[string]
$state = @{ Copied = 0.0; Chunk = 0.0 }

function Add-ProgressStep {
    $percent = [int][math]::Floor(99.0 * $state.Copied / $totalBytes)
    if ($percent -lt 1) { $percent = 1 }
    if ($percent -gt 99) { $percent = 99 }
    $lines.Add("StrCpy `$0 $percent")
    $lines.Add('Call BrhProgress')
}

function Add-FileStep([string]$fullPath, [string]$nsisPath) {
    $length = [double](Get-Item -LiteralPath $fullPath).Length
    $lines.Add('File "' + $nsisPath + '"')
    $state.Copied += $length
    $state.Chunk += $length
    if ($state.Chunk -ge 8MB) {
        Add-ProgressStep
        $state.Chunk = 0.0
    }
}

# Emits SetOutPath + File for a payload folder and its subfolders, so the generated
# list stays equivalent to the "File /r" fallback of installer.nsi.
function Add-PayloadTree([string]$root, [string]$outPath, [string]$stagePath) {
    $root = $root.TrimEnd('\')
    $dirs = New-Object System.Collections.Generic.List[string]
    $dirs.Add($root)
    foreach ($dir in (Get-ChildItem -LiteralPath $root -Directory -Recurse | Sort-Object FullName)) {
        $dirs.Add($dir.FullName)
    }

    foreach ($dir in $dirs) {
        $rel = ''
        if ($dir.Length -gt $root.Length) { $rel = $dir.Substring($root.Length).TrimStart('\') }
        $stageDir = $stagePath
        if ($rel) {
            $lines.Add('SetOutPath "' + $outPath + '\' + $rel + '"')
            $stageDir = $stagePath + '\' + $rel
        } else {
            $lines.Add('SetOutPath "' + $outPath + '"')
        }
        foreach ($file in (Get-ChildItem -LiteralPath $dir -File | Sort-Object Name)) {
            Add-FileStep $file.FullName ($stageDir + '\' + $file.Name)
        }
    }
}

$lines.Add('; Generated by scripts/build-installer.ps1 from the staged payload - do not edit.')
$lines.Add('; Every StrCpy/Call pair reports the share of payload bytes copied so far.')
$lines.Add('; installer.nsi reports the final 100% after the last installation step.')

foreach ($name in $baseFiles) {
    Add-FileStep (Join-Path $StageDir $name) ('${STAGE}\' + $name)
}

# Same guard as the installer fallback: an upgrade never overwrites settings.
$lines.Add('IfFileExists "$INSTDIR\settings.jsonc" brh_keep_settings')
Add-FileStep (Join-Path $StageDir $settingsFile) ('${STAGE}\' + $settingsFile)
$lines.Add('brh_keep_settings:')

Add-PayloadTree $dataRoot '$INSTDIR\data' '${STAGE}\data'
Add-PayloadTree $remasterRoot '$INSTDIR\remaster' '${STAGE}\remaster'
Add-ProgressStep
Set-Content -LiteralPath $dataGroups -Value $lines -Encoding UTF8

New-Item -ItemType Directory -Force $OutputDir | Out-Null
$output = Join-Path $OutputDir $SetupName
if (Test-Path $output) { Remove-Item $output -Force }

$arguments = @(
    '/V3',
    '/INPUTCHARSET',
    'UTF8',
    "/DSTAGE=$StageDir",
    "/DUIDIR=$UIDir",
    "/DDATAGROUPS=$dataGroups",
    "/DOUTFILE=$output",
    $Nsi
)

$timer = [System.Diagnostics.Stopwatch]::StartNew()
& $Makensis @arguments
$exit = $LASTEXITCODE
$timer.Stop()

if ($exit -ne 0 -or -not (Test-Path $output)) {
    throw "makensis failed (exit $exit)"
}

$file = Get-Item $output
$hash = (Get-FileHash $output -Algorithm SHA256).Hash.ToLowerInvariant()
$sizeMb = [math]::Round($file.Length / 1MB, 2)
$elapsed = [math]::Round($timer.Elapsed.TotalSeconds, 1)

"installer: $($file.FullName)"
"size: $sizeMb MiB"
"sha256: $hash"
"build time: $elapsed s"

$buildInfo = [pscustomobject]@{
    version = $Version
    installer = $file.Name
    bytes = $file.Length
    sha256 = $hash
    built_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    build_seconds = $elapsed
    stage = $StageDir
    ui = $UIDir
}
$buildInfo | ConvertTo-Json | Set-Content (Join-Path $OutputDir 'build-info.json') -Encoding UTF8
