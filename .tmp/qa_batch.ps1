# QA Stage D batch driver: qa_run for every .qa script + qa_golden_check over every
# golden set, mirroring max-brh-cicd/ci.sh stages 3/4. Writes a CSV summary and a
# per-step log under .tmp/.
$ErrorActionPreference = 'Continue'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$repo = 'D:\dev\max-port-debug'
Set-Location "$repo\max-brh"

$logDir = Join-Path $repo '.tmp\qa_logs'
if (Test-Path $logDir) { Remove-Item -Recurse -Force $logDir }
New-Item -ItemType Directory -Path $logDir | Out-Null

# A clean output tree, exactly like ci.sh ("rm -rf qa-out"): stale frames must not
# mask a regression.
if (Test-Path qa-out) { Remove-Item -Recurse -Force qa-out }

# name -> golden dir (relative to max-brh-qa); empty = no golden comparison (the
# script is still a gate through its asserts).
$cases = @(
    @('sp-full', ''),
    @('hs-2', ''),
    @('visual-screens', 'golden'),
    @('victory', ''),
    @('production', 'golden/production'),
    @('save-load', ''),
    @('upgrade', 'golden/upgrade'),
    @('research', 'golden/research'),
    @('edge-cases', ''),
    @('cargo-full', 'golden/cargo'),
    @('move-input', 'golden/move-input'),
    @('research-help', 'golden/research-help'),
    @('unit-menu', 'golden/unit-menu'),
    @('allocate', 'golden/allocate'),
    @('preferences', 'golden/preferences'),
    @('xfer', 'golden/xfer'),
    @('ai-turn', 'golden/ai-turn'),
    @('sp-tb', 'golden/sp-tb'),
    @('combat-tb', 'golden/combat-tb'),
    @('missile-line', 'golden/missile-line'),
    @('build-cancel', 'golden/build-cancel'),
    @('panels', 'golden/panels'),
    @('soak-100', '')
)

$rows = @()
foreach ($case in $cases) {
    $name = $case[0]
    $golden = $case[1]

    $scriptLog = Join-Path $logDir "$name.qa.log"
    & cargo run -q -p max_app --no-default-features --example qa_run -- `
        "../max-brh-qa/scripts/$name.qa" "qa-out/$name" *> $scriptLog
    $scriptExit = $LASTEXITCODE

    $goldenExit = 'n/a'
    $goldenNote = ''
    if ($golden -ne '') {
        $goldenLog = Join-Path $logDir "$name.golden.log"
        & cargo run -q -p max_app --no-default-features --example qa_golden_check -- `
            "qa-out/$name" "../max-brh-qa/$golden" *> $goldenLog
        $goldenExit = $LASTEXITCODE
        $goldenNote = (Select-String -Path $goldenLog -Pattern 'qa_golden_check: сверено' |
            Select-Object -First 1).Line
        if (-not $goldenNote) {
            $goldenNote = (Select-String -Path $goldenLog -Pattern 'ДРЕЙФ|ВИЗУАЛЬНЫЙ|нет кадров|нет эталона' |
                Select-Object -First 1).Line
        }
    }

    $rows += [pscustomobject]@{
        case        = $name
        script_exit = $scriptExit
        golden_exit = $goldenExit
        golden      = $goldenNote
    }
    Write-Host ("{0,-16} script={1} golden={2} {3}" -f $name, $scriptExit, $goldenExit, $goldenNote)
}

$rows | Export-Csv -Path (Join-Path $repo '.tmp\qa_results.csv') -NoTypeInformation -Encoding UTF8
Write-Host 'QA-BATCH-DONE'
