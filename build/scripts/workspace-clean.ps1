# Очистка рабочих артефактов (сборки/логи/скриншоты). Повторяемая.
#   powershell -ExecutionPolicy Bypass -File build\scripts\workspace-clean.ps1 [-DryRun]
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

# --- cargo-сборки -----------------------------------------------------------
Remove-Target 'max-rust\target'
Remove-Target 'max-horizon-server\target'
Remove-Target 'max-brh-qa-tools\max-resource-editor\target'
Remove-Target 'build\cargo-home'

# --- устаревшие/кросс-сборки и временные каталоги build\ -------------------
$buildDirs = @(
    'build\win-msvc-target', 'build\win-msvc-target-headless',
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

# --- DOS-эксперимент: удаляем захваты, оставляем proof/ и скрипты ----------
Remove-Target 'build\anim-dos\captures'
Remove-Target 'build\anim-dos\game'
if (Test-Path 'build\anim-dos\dosextract') { Remove-Target 'build\anim-dos\dosextract' }

# --- ad-hoc логи/скриншоты/одноразовые скрипты в корне build\ -------------
$keep = @('scripts', 'linux-server-template', 'Dockerfile', 'docker-build-linux-server.sh',
    'docker-build-server.sh', 'docker-build.sh', 'prepare-linux-template.sh',
    'run-linux-server-with-data.sh', 'verify-linux-template.sh', 'capture-ef.ps1',
    'make-run.ps1', 'make-server-run.ps1', 'ui-lib.ps1', 'anim-dos')
Get-ChildItem 'build' -File -ErrorAction SilentlyContinue | ForEach-Object {
    if ($keep -contains $_.Name) { return }
    Write-Host ("build\{0}" -f $_.Name)
    if (-not $DryRun) { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction Continue }
}

$after = (Get-ChildItem $root -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
Write-Host ("== workspace: {0:N1} GB (freed {1:N1} GB) ==" -f ($after / 1GB), (($before - $after) / 1GB))
