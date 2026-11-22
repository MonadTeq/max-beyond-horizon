# Cleanup of working artifacts (builds/logs/screenshots). Repeatable.
#   powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\workspace-clean.ps1 [-DryRun]
param([switch]$DryRun)
$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $root

function DirSize($p) {
    if (-not (Test-Path $p)) { return 0 }
    return (Get-ChildItem $p -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
}
function Remove-Target([string]$rel) {
    $full = Join-Path $root $rel
    if (-not (Test-Path $full)) { return }
    $size = DirSize $full
    Write-Host ("{0,-45} {1,8:N1} GB" -f $rel, ($size / 1GB))
    if (-not $DryRun) { Remove-Item -LiteralPath $full -Recurse -Force -ErrorAction Continue }
}

$before = (Get-ChildItem $root -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
Write-Host ("== workspace: {0:N1} GB ==" -f ($before / 1GB))

# --- cargo builds -----------------------------------------------------------
Remove-Target 'max-brh\target'
Remove-Target 'max-brh-server\target'
Remove-Target 'max-brh-qa-tools\max-resources-viewer\target'
# Windows builds with native MSVC: UI (build-windows-ui.ps1) and headless
# (build-windows-console.ps1) keep their target separate from max-brh\target so that
# MSVC artifacts do not mix with the Linux/Docker builds.
Remove-Target 'bin\target'
Remove-Target 'bin\target-console'
# CARGO_HOME for the Linux/Docker builds (ci.sh, release.sh); the Windows builds do not
# use it — they take ~\.cargo.
Remove-Target 'build\cargo-home'

# --- legacy/cross builds and temporary build\ directories ------------------
$buildDirs = @(
    'bin\release',
    'build\msvc-console',
    'build\cross-winxp-x64-debug', 'build\cross-winxp-x64-server-debug',
    'build\linux-x64-debug', 'build\linux-x64-server-static',
    'build\m52', 'build\run', 'build\run2', 'build\recovered',
    'build\ui-probe-build', 'build\ui-check', 'build\ui-preview',
    'build\ui-test-allocate', 'build\ui-test-construction', 'build\ui-test-construction-popup',
    'build\ui-test-move', 'build\ui-test-move2', 'build\ui-test-popup', 'build\ui-test-prefs',
    'build\ui-test-release', 'build\ui-test-sp',
    'build\sendinput-probe', 'build\sendinput-probe2',
    'build\server-run', 'build\session-log', 'build\smoke-logs', 'build\tmp',
    'build\inspect', 'build\game-data-smoke', 'build\docker-sessions',
    'build\horizon-sessions', 'build\capture', '__pycache__', '.session-log'
)
foreach ($d in $buildDirs) { Remove-Target $d }

# --- DOS experiment: delete the captures, keep proof/ and the scripts ------
Remove-Target 'build\anim-dos\captures'
Remove-Target 'build\anim-dos\game'
if (Test-Path 'build\anim-dos\dosextract') { Remove-Target 'build\anim-dos\dosextract' }

# --- ad-hoc logs/screenshots/one-off scripts in the build\ root -----------
# Reusable scripts were moved to max-brh-cicd/scripts/ (git-tracked); artifacts go
# to bin/ (bin/linux-server-template, bin/sdl3-3.4.20, bin/release, bin/target).
# Only transient logs/caches remain in the build/ root.
Get-ChildItem 'build' -File -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host ("build\{0}" -f $_.Name)
    if (-not $DryRun) { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction Continue }
}

$after = (Get-ChildItem $root -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
Write-Host ("== workspace: {0:N1} GB (freed {1:N1} GB) ==" -f ($after / 1GB), (($before - $after) / 1GB))
