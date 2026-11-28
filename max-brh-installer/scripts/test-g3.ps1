# Gate G3 tests for the portable NSIS installer.
#
# Covers: silent install into a chosen folder, catalog contents, game start
# (headless --new-game / --demo from the installed saves folder), optional Start
# Menu shortcut, upgrade over an existing installation, uninstall with saves
# kept, uninstall with /SAVES, and oracle cleanliness.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
    [string]$Setup = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..')).Path 'dist\MAX-BRH-0.1.28a-setup.exe'),
    [string]$WorkRoot = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path '.tmp\g3'),
    [switch]$KeepWork
)

$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path $RepoRoot).Path
$Setup = (Resolve-Path $Setup).Path
if (Test-Path $WorkRoot) { Remove-Item $WorkRoot -Recurse -Force }
New-Item -ItemType Directory -Force $WorkRoot | Out-Null
$logPath = Join-Path $WorkRoot 'g3-report.txt'
$script:Failures = New-Object System.Collections.Generic.List[string]
$script:Checks = New-Object System.Collections.Generic.List[string]

function Log([string]$Message) {
    $line = "[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $Message
    $line | Tee-Object -FilePath $logPath -Append | Out-Null
    Write-Host $line
}

function Check([string]$Name, [scriptblock]$Test) {
    try {
        $ok = & $Test
        if ($ok) {
            $script:Checks.Add($Name)
            Log "PASS  $Name"
        }
        else {
            $script:Failures.Add($Name)
            Log "FAIL  $Name"
        }
    }
    catch {
        $script:Failures.Add($Name)
        Log "FAIL  $Name ($($_.Exception.Message))"
    }
}

function Hash([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return '' }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Run([string]$File, [string[]]$Arguments, [string]$WorkingDirectory = $WorkRoot, [int]$TimeoutSeconds = 180) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -WorkingDirectory $WorkingDirectory -PassThru
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        $process.Kill()
        throw "timeout: $File $Arguments"
    }
    return $process.ExitCode
}

function WaitUntil([scriptblock]$Condition, [int]$TimeoutSeconds = 60) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (& $Condition) { return $true }
        Start-Sleep -Milliseconds 250
    }
    return $false
}

function Start-Uninstaller([string]$InstallDir, [string[]]$Arguments = @('/S')) {
    $uninstaller = Join-Path $InstallDir 'uninstall.exe'
    if (Test-Path $uninstaller) {
        Start-Process -FilePath $uninstaller -ArgumentList $Arguments -WorkingDirectory $InstallDir | Out-Null
    }
}

$programs = [Environment]::GetFolderPath('Programs')
$shortcutFolder = Join-Path $programs 'MAX - Beyond Rust Horizon'
$shortcutLink = Join-Path $shortcutFolder 'MAX - Beyond Rust Horizon.lnk'

function Get-ArpMatches {
    $roots = @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
    )
    $matches = @()
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        foreach ($key in (Get-ChildItem $root -ErrorAction SilentlyContinue)) {
            $name = (Get-ItemProperty $key.PSPath -ErrorAction SilentlyContinue).DisplayName
            if ($name -and ($name -match 'Beyond Rust Horizon|MAX-BRH')) {
                $matches += "$($key.PSPath) = $name"
            }
        }
    }
    return $matches
}

$inventory = Get-Content (Join-Path $RepoRoot 'max-brh-installer\data-inventory.json') -Raw | ConvertFrom-Json
$maxResEntry = $inventory.files | Where-Object { $_.path -eq 'MAX.RES' } | Select-Object -First 1
$oracleBefore = (& git -C $RepoRoot status --porcelain -- max-original max-cpp-port max-original-dasm) -join "`n"
$arpBefore = @(Get-ArpMatches)

$install1 = Join-Path $WorkRoot 'install-default'
$install2 = Join-Path $WorkRoot 'install-shortcut'
$install3 = Join-Path $WorkRoot 'install-delete-saves'

Log "setup: $Setup"
Log "work: $WorkRoot"

# --- 1. Silent install, no shortcut -------------------------------------------------

$exit = Run $Setup @('/S', "/D=$install1")
Check 'silent install exits with 0' { $exit -eq 0 }
Check 'game.exe and SDL3.dll are installed' {
    (Test-Path (Join-Path $install1 'game.exe')) -and (Test-Path (Join-Path $install1 'SDL3.dll'))
}
Check 'data/ contains all 509 inventory files' {
    (Get-ChildItem (Join-Path $install1 'data') -File -Recurse | Measure-Object).Count -eq $inventory.totals.files
}
Check 'MAX.RES sha256 matches the inventory' {
    (Hash (Join-Path $install1 'data\MAX.RES')) -eq $maxResEntry.sha256
}
Check 'PATCHES.RES and maxint.json are in data/' {
    (Test-Path (Join-Path $install1 'data\PATCHES.RES')) -and (Test-Path (Join-Path $install1 'data\maxint.json'))
}
Check 'mandatory remaster is installed' {
    (Test-Path (Join-Path $install1 'remaster\remaster-manifest.json')) -and
    ((Get-ChildItem (Join-Path $install1 'remaster') -File -Filter '*.ktx2' | Measure-Object).Count -ge 8)
}
Check 'docs and settings template are installed' {
    @('LICENSE', 'THIRD_PARTY_LICENSES.txt', 'README.txt', 'CHANGELOG.md', 'settings.jsonc') |
        ForEach-Object { Test-Path (Join-Path $install1 $_) } | Where-Object { -not $_ } | Measure-Object | ForEach-Object { $_.Count -eq 0 }
}
Check 'saves/ and missions/ directories exist' {
    (Test-Path (Join-Path $install1 'saves')) -and (Test-Path (Join-Path $install1 'missions'))
}
Check 'uninstall.exe is inside the install folder' { Test-Path (Join-Path $install1 'uninstall.exe') }
Check 'no Start Menu shortcut without /SHORTCUT' { -not (Test-Path $shortcutLink) }
Check 'no ARP uninstall entry is written' { (@(Get-ArpMatches)).Count -eq 0 }

# --- 2. Game starts from the installed catalog and reads saves from it -------------

New-Item -ItemType Directory -Force (Join-Path $install1 'saves') | Out-Null
foreach ($save in @('SAVE1.CAM', 'SAVE1.TRA', 'SAVE1.SCE', 'SAVE1.MPS')) {
    $source = Join-Path $RepoRoot "max-original\$save"
    if (Test-Path $source) {
        Copy-Item $source (Join-Path $install1 'saves') -Force
    }
}
Set-Content -Path (Join-Path $install1 'preferences.ini') -Value "g3_marker=1" -Encoding ascii
Add-Content -Path (Join-Path $install1 'settings.jsonc') -Value '// g3 marker' -Encoding utf8

$smokeDir = Join-Path $WorkRoot 'smoke'
New-Item -ItemType Directory -Force $smokeDir | Out-Null
$smokePpm = Join-Path $smokeDir 'frame.ppm'
$gameExit = Run (Join-Path $install1 'game.exe') @('--new-game', '--out', $smokePpm) $smokeDir 180
Check 'installed game.exe --new-game exits with 0 from a foreign CWD' { $gameExit -eq 0 }
Check '--new-game wrote the frame file' { (Test-Path $smokePpm) -and ((Get-Item $smokePpm).Length -gt 100000) }

$demoExit = Run (Join-Path $install1 'game.exe') @('--demo', (Join-Path $install1 'saves\SAVE1.CAM'), '--turns', '1') $WorkRoot 180
Check 'installed game.exe --demo reads a save from saves/' { $demoExit -eq 0 }

# --- 3. Silent install with /SHORTCUT ----------------------------------------------

$exit2 = Run $Setup @('/S', '/SHORTCUT', "/D=$install2")
Check 'silent install with /SHORTCUT exits with 0' { $exit2 -eq 0 }
Check 'Start Menu shortcut exists only with /SHORTCUT' { Test-Path $shortcutLink }
if (Test-Path $shortcutLink) {
    Check 'shortcut target is the installed game.exe' {
        $shell = New-Object -ComObject WScript.Shell
        $target = $shell.CreateShortcut($shortcutLink).TargetPath
        [IO.Path]::GetFullPath($target) -ieq [IO.Path]::GetFullPath((Join-Path $install2 'game.exe'))
    }
}

# --- 4. Upgrade over the existing installation -------------------------------------

$savesBefore = @{}
foreach ($file in (Get-ChildItem (Join-Path $install1 'saves') -File)) {
    $savesBefore[$file.Name] = Hash $file.FullName
}
$settingsHash = Hash (Join-Path $install1 'settings.jsonc')
$prefsHash = Hash (Join-Path $install1 'preferences.ini')

$exitUpgrade = Run $Setup @('/S', "/D=$install1")
Check 'upgrade install exits with 0' { $exitUpgrade -eq 0 }
Check 'upgrade keeps all save files byte-for-byte' {
    $ok = $true
    foreach ($name in $savesBefore.Keys) {
        if ((Hash (Join-Path $install1 "saves\$name")) -ne $savesBefore[$name]) { $ok = $false }
    }
    $ok
}
Check 'upgrade keeps settings.jsonc and preferences.ini' {
    ((Hash (Join-Path $install1 'settings.jsonc')) -eq $settingsHash) -and
    ((Hash (Join-Path $install1 'preferences.ini')) -eq $prefsHash)
}
Check 'upgrade restores game data from the package' {
    (Hash (Join-Path $install1 'data\MAX.RES')) -eq $maxResEntry.sha256
}

# --- 5. Uninstall keeps saves by default -------------------------------------------

Start-Uninstaller $install1
$uninstalled = WaitUntil { -not (Test-Path (Join-Path $install1 'game.exe')) } 60
$uninstalledClean = WaitUntil {
    (-not (Test-Path (Join-Path $install1 'data'))) -and
    (-not (Test-Path (Join-Path $install1 'uninstall.exe')))
} 60
Check 'silent uninstall removes game.exe' { $uninstalled }
Check 'silent uninstall removes data/ and uninstall.exe' { $uninstalledClean }
Check 'uninstall keeps saves by default' {
    (Test-Path (Join-Path $install1 'saves\SAVE1.CAM')) -and
    (Test-Path (Join-Path $install1 'saves\SAVE1.TRA'))
}
Check 'uninstall removes data/, remaster/ and settings' {
    (-not (Test-Path (Join-Path $install1 'data'))) -and
    (-not (Test-Path (Join-Path $install1 'remaster'))) -and
    (-not (Test-Path (Join-Path $install1 'settings.jsonc'))) -and
    (-not (Test-Path (Join-Path $install1 'uninstall.exe')))
}
Check 'uninstall leaves no Start Menu shortcut' { -not (Test-Path $shortcutLink) }

# Install 2 is uninstalled too; its shortcut must disappear.
Start-Uninstaller $install2
Check 'uninstall removes the Start Menu shortcut' { WaitUntil { -not (Test-Path $shortcutLink) } 60 }
Check 'uninstall removes the install folder when no saves exist' { WaitUntil { -not (Test-Path $install2) } 30 }

# --- 6. Uninstall with /SAVES deletes saves ----------------------------------------

$exit3 = Run $Setup @('/S', "/D=$install3")
New-Item -ItemType Directory -Force (Join-Path $install3 'saves') | Out-Null
Set-Content -Path (Join-Path $install3 'saves\save1.cam') -Value 'g3 save' -Encoding ascii
Start-Uninstaller $install3 @('/S', '/SAVES')
Check 'uninstall /SAVES deletes saves' {
    WaitUntil { -not (Test-Path (Join-Path $install3 'saves\save1.cam')) } 60
}
Check 'uninstall /SAVES removes the empty install folder' { WaitUntil { -not (Test-Path $install3) } 30 }

# --- 7. Oracles and registry -------------------------------------------------------

$oracleAfter = (& git -C $RepoRoot status --porcelain -- max-original max-cpp-port max-original-dasm) -join "`n"
Check 'oracles unchanged by the installer tests' { $oracleBefore -eq $oracleAfter }
Check 'no ARP uninstall entry after all tests' { (@(Get-ArpMatches)).Count -eq 0 }
Check 'start menu folder is gone' { -not (Test-Path $shortcutFolder) }

$result = [pscustomobject]@{
    setup = $Setup
    checks_passed = $script:Checks.Count
    checks_failed = $script:Failures.Count
    failures = @($script:Failures)
    work_root = $WorkRoot
}
$result | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $WorkRoot 'g3-result.json') -Encoding UTF8

Log "result: $($script:Checks.Count) passed, $($script:Failures.Count) failed"
if ($script:Failures.Count -gt 0) {
    throw "G3 FAILED: $($script:Failures -join '; ')"
}
Log 'G3 PASS'
