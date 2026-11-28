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

# Chunked file list for a live progress bar made from the owner's art: ~8 MiB
# per group, each group reports a percentage to BrhProgress in the NSIS script.
$dataGroups = Join-Path $UIDir 'data-groups.nsh'
$dataFiles = @(Get-ChildItem (Join-Path $StageDir 'data') -File | Sort-Object Name)
if ($dataFiles.Count -eq 0) { throw "no staged data files in $StageDir\data" }
$dataBytes = [double](($dataFiles | Measure-Object -Property Length -Sum).Sum)
$lines = New-Object System.Collections.Generic.List[string]
$lines.Add('SetOutPath "$INSTDIR\data"')
$processed = 0.0
$chunk = 0.0
for ($i = 0; $i -lt $dataFiles.Count; $i++) {
    $file = $dataFiles[$i]
    $lines.Add('File "${STAGE}\data\' + $file.Name + '"')
    $chunk += [double]$file.Length
    $processed += [double]$file.Length
    if ($chunk -ge 8MB -or $i -eq ($dataFiles.Count - 1)) {
        $percent = [int][math]::Floor(10 + 80.0 * $processed / $dataBytes)
        if ($percent -gt 90) { $percent = 90 }
        $lines.Add("StrCpy `$0 $percent")
        $lines.Add("Call BrhProgress")
        $chunk = 0.0
    }
}
$lines.Add('SetOutPath "$INSTDIR\remaster"')
foreach ($file in (Get-ChildItem (Join-Path $StageDir 'remaster') -File | Sort-Object Name)) {
    $lines.Add('File "${STAGE}\remaster\' + $file.Name + '"')
}
$lines.Add('StrCpy $0 98')
$lines.Add('Call BrhProgress')
Set-Content -LiteralPath $dataGroups -Value $lines -Encoding UTF8

New-Item -ItemType Directory -Force $OutputDir | Out-Null
$output = Join-Path $OutputDir $SetupName
if (Test-Path $output) { Remove-Item $output -Force }

$arguments = @(
    '/V3',
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
