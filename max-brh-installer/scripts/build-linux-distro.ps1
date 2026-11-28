# Builds the portable Linux distribution (stage 4, distro-installers.md §D/§F)
# in the max-brh-installer/linux Docker image and runs the G4 gate over it.
#
#   powershell -ExecutionPolicy Bypass -File max-brh-installer\scripts\build-linux-distro.ps1
#   ... -SkipBuild      # reuse the cargo cache / an already built Linux binary
#   ... -RebuildImage   # rebuild the Docker image (max-brh-linux-dist:<version>)
#   ... -GateOnly       # only the G4 gate over the artifacts already in .tmp/linux-dist/out
#   ... -KeepStaging    # keep the staging tree and the extracted AppDir under .tmp
#
# Artifacts (gitignored, outside git):
#   max-brh-installer/dist/linux/max-brh-<version>-linux.tar.gz  (or .tar.xz: the smaller)
#   max-brh-installer/dist/linux/MAX-BRH-<version>-x86_64.AppImage
#   max-brh-installer/dist/linux/SHA256SUMS
#   max-brh-installer/dist/linux/staging-report.json
#   max-brh-installer/dist/linux/g4-summary.json, g4.log
#
# Requirements: Docker Desktop with Linux containers; network on the first run
# (apt/rustup/crates.io/appimagetool). The image is built once from
# max-brh-installer/linux/Dockerfile; the build itself runs from /src inside it.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
    [string]$WorkDir = '',
    [string]$DistDir = '',
    [string]$Image = 'max-brh-linux-dist:0.1.28a',
    [string]$Dockerfile = '',
    [string]$Features = 'sdl-from-source,sound',
    [int]$Jobs = 0,
    [switch]$RebuildImage,
    [switch]$SkipBuild,
    [switch]$GateOnly,
    [switch]$KeepStaging
)

$ErrorActionPreference = 'Stop'

function Invoke-Step([string]$What, [scriptblock]$Body) {
    Write-Host ''
    Write-Host "== $What =="
    & $Body
    if ($LASTEXITCODE -ne 0) {
        throw "$What failed with exit code $LASTEXITCODE"
    }
}

$RepoRoot = (Resolve-Path $RepoRoot).Path
if (-not $WorkDir) { $WorkDir = Join-Path $RepoRoot '.tmp\linux-dist' }
if (-not $DistDir) { $DistDir = Join-Path $RepoRoot 'max-brh-installer\dist\linux' }
if (-not $Dockerfile) { $Dockerfile = Join-Path $RepoRoot 'max-brh-installer\linux\Dockerfile' }
$notFoundError = 'docker: command not found'
if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { throw $notFoundError }

New-Item -ItemType Directory -Force $WorkDir, $DistDir | Out-Null
$OutDir = Join-Path $WorkDir 'out'

$inner = if ($GateOnly) {
    @('bash', '/src/max-brh-installer/linux/gate-g4.sh')
} else {
    @('bash', '/src/max-brh-installer/linux/build-distro.sh')
}

# The image carries the Rust toolchain, the packaging tools and Xvfb.
docker image inspect $Image *> $null
if ($LASTEXITCODE -ne 0 -or $RebuildImage) {
    Invoke-Step "docker build $Image" {
        docker build --progress=plain -t $Image -f $Dockerfile (Split-Path -Parent $Dockerfile)
    }
} else {
    Write-Host "image: $Image (cached; -RebuildImage to rebuild)"
}

$runArgs = @(
    'run', '--rm',
    '-v', "${RepoRoot}:/src",
    '-v', "${WorkDir}:/work",
    '-e', "FEATURES=$Features"
)
if ($Jobs -gt 0) { $runArgs += @('-e', "JOBS=$Jobs") }
if ($SkipBuild) { $runArgs += @('-e', 'SKIP_BUILD=1') }
if ($KeepStaging) { $runArgs += @('-e', 'KEEP_TAR=1') }
$runArgs += @($Image) + $inner

if (-not $GateOnly) {
    Invoke-Step "build + package (Docker: $Image)" { docker @runArgs }
}

if (-not $GateOnly -or (Test-Path $OutDir)) {
    Invoke-Step "G4 gate (Docker: $Image)" {
        docker run --rm -v "${RepoRoot}:/src" -v "${WorkDir}:/work" $Image `
            bash /src/max-brh-installer/linux/gate-g4.sh
    }
}

# Collect the release artifacts and both reports next to the Windows installer.
# Only the deliverables are copied: the staging catalog stays in .tmp.
if (Test-Path $OutDir) {
    Get-ChildItem $DistDir -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^(max-brh-.*-linux\.tar\.(gz|xz)|MAX-BRH-.*\.AppImage|SHA256SUMS|staging-report\.json|g4-.*)$' } |
        Remove-Item -Force
    foreach ($pattern in @('max-brh-*-linux.tar.gz', 'max-brh-*-linux.tar.xz', 'MAX-BRH-*-x86_64.AppImage', 'SHA256SUMS', 'staging-report.json')) {
        Get-ChildItem (Join-Path $OutDir $pattern) -File -ErrorAction SilentlyContinue |
            Copy-Item -Destination $DistDir -Force
    }
}
$g4Dir = Join-Path $WorkDir 'g4'
foreach ($report in @('g4-summary.json', 'g4.log')) {
    $source = Join-Path $g4Dir $report
    if (Test-Path $source) { Copy-Item $source $DistDir -Force }
}

Write-Host ''
Write-Host 'Artifacts:'
Get-ChildItem $DistDir -File |
    Sort-Object Name |
    ForEach-Object { Write-Host ('  {0,-45} {1,14:N0} bytes' -f $_.Name, $_.Length) }
if (Test-Path (Join-Path $DistDir 'SHA256SUMS')) {
    Write-Host ''
    Get-Content (Join-Path $DistDir 'SHA256SUMS') | ForEach-Object { Write-Host "  $_" }
}

# Every failure path above throws; reaching this point means build + gate passed.
# The explicit exit keeps the wrapper's exit code meaningful for CI (docker and
# appimagetool write progress to stderr, which Windows PowerShell may report as
# a non-terminating error record).
exit 0

