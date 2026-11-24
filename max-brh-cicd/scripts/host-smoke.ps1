# Native P2P host smoke - `PLAN-host-mode.md` section 5 H5, gate (e). Windows edition.
#
# Spawns native headless `max-brh.exe` processes on 127.0.0.1 - one `--host` and
# `Clients - 1` direct `--connect` guests - and plays `Turns` turns of the P2P
# host mode. The gate asserts:
#   * every client finished the requested number of turns;
#   * `desyncs=0` and `barrier_timeouts=0` on every client;
#   * the shared digest (`shared:` line) is identical on every client;
#   * no `desync-turn*.txt` report and no `host-audit:`/`audit:` line was
#     produced (the desync audit 23/24/49 never had to run).
#
# No Horizon, no Docker, no `live-smoke.ps1`, no C++ oracle: this is the pure
# Rust native star topology (host + direct guests). The bash twin is
# `host-smoke.sh` (same cases, same assertions).
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\host-smoke.ps1 [-Clients 2|3|all] [-Turns 10]
#
# Environment:
#   HOST_SMOKE_BIN      reuse an already built `max-brh.exe`
#   HOST_SMOKE_PROFILE  debug (default) | release
#   HOST_SMOKE_PORT     base UDP port (default: a random 32000..34999)
#   HOST_SMOKE_TIMEOUT  seconds to wait for the match (default 240)
#   HOST_SMOKE_LOGS     log directory (default <repo>\build\host-smoke)
#   HOST_SMOKE_DATA     game data directory (default <repo>\max-original)
#   HOST_SMOKE_WORLD    planet index (default 12 - GREEN_1)
#   HOST_SMOKE_SEED     RNG seed of the lobby settings (default 4242)
#   CARGO_TARGET_DIR    respected by the build
param(
    [string]$Clients = "all",
    [int]$Turns = 10
)

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$DataDir = if ($env:HOST_SMOKE_DATA) { $env:HOST_SMOKE_DATA } else { Join-Path $Root "max-original" }
$Logs = if ($env:HOST_SMOKE_LOGS) { $env:HOST_SMOKE_LOGS } else { Join-Path $Root "build\host-smoke" }
$TimeoutSec = if ($env:HOST_SMOKE_TIMEOUT) { [int]$env:HOST_SMOKE_TIMEOUT } else { 240 }
$Profile = if ($env:HOST_SMOKE_PROFILE) { $env:HOST_SMOKE_PROFILE } else { "debug" }
$World = if ($env:HOST_SMOKE_WORLD) { $env:HOST_SMOKE_WORLD } else { "12" }
$Seed = if ($env:HOST_SMOKE_SEED) { $env:HOST_SMOKE_SEED } else { "4242" }
$BasePort = if ($env:HOST_SMOKE_PORT) { [int]$env:HOST_SMOKE_PORT } else { 0 }

function Fail([string]$Message) {
    Write-Host "host-smoke: FAIL $Message" -ForegroundColor Red
    exit 1
}

function Quote([string]$Value) {
    if ($Value -match '\s') { return '"' + $Value + '"' }
    return $Value
}

# --- build (unless an explicit binary was given) --------------------------
New-Item -ItemType Directory -Force -Path $Logs | Out-Null
if ($env:HOST_SMOKE_BIN) {
    $Bin = (Resolve-Path $env:HOST_SMOKE_BIN).Path
}
else {
    $cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (-not (Test-Path $cargo)) {
        $cmd = Get-Command cargo -ErrorAction SilentlyContinue
        if (-not $cmd) { Fail "cargo not found (install Rust 1.94.0)" }
        $cargo = $cmd.Source
    }
    $profileFlag = if ($Profile -eq "release") { "--release" } else { "" }
    Write-Host "host-smoke: cargo build --no-default-features -p max_app $profileFlag"
    # Build from the workspace directory: `max-brh\.cargo\config.toml` raises the
    # Windows main-thread stack to 8 MiB (the headless debug build overflows the
    # default 1 MiB), and cargo only reads `.cargo\config.toml` from the current
    # directory ancestry. `Start-Process` merges no streams: cargo writes its
    # progress to stderr, which `$ErrorActionPreference = "Stop"` would otherwise
    # turn into a terminating NativeCommandError.
    $buildLog = Join-Path $Logs "build.log"
    $buildArgs = @("build", "--no-default-features", "-p", "max_app")
    if ($profileFlag) { $buildArgs += $profileFlag }
    $build = Start-Process -FilePath $cargo -ArgumentList $buildArgs `
        -WorkingDirectory (Join-Path $Root "max-brh") `
        -RedirectStandardOutput $buildLog -RedirectStandardError "$buildLog.err" `
        -PassThru -Wait -NoNewWindow
    if ($build.ExitCode -ne 0) {
        Get-Content "$buildLog.err" -Tail 20 -ErrorAction SilentlyContinue
        Fail "build failed (see $buildLog and $buildLog.err)"
    }
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root "max-brh\target" }
    $Bin = Join-Path $targetDir "$Profile\max-brh.exe"
    if (-not (Test-Path $Bin)) { Fail "built binary not found: $Bin" }
}

if (-not (Test-Path (Join-Path $DataDir "MAX.RES"))) {
    Fail "game data missing: $DataDir\MAX.RES (the original M.A.X. data is required)"
}

# --- helpers ---------------------------------------------------------------
function Get-Field([string]$Line, [string]$Key) {
    $match = [regex]::Match($Line, "(?:^|\s)$([regex]::Escape($Key))=(\S+)")
    if ($match.Success) { return $match.Groups[1].Value }
    return ""
}

function Get-SharedLine([string]$Path) {
    $line = Select-String -Path $Path -Pattern '^shared: ' -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($line) { return $line.Line.Substring("shared: ".Length) }
    return ""
}

function Wait-Processes([System.Diagnostics.Process[]]$Processes, [int]$Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        $alive = $Processes | Where-Object { -not $_.HasExited }
        if (-not $alive) { return $true }
        Start-Sleep -Milliseconds 250
    }
    return $false
}

function Stop-Processes([System.Diagnostics.Process[]]$Processes) {
    foreach ($process in $Processes) {
        if (-not $process.HasExited) {
            try { $process.Kill() } catch { }
        }
    }
}

# Runs `Clients` clients for `Turns` turns; throws on a failed gate.
function Invoke-Case([int]$Clients, [int]$Turns) {
    $label = "${Clients}c-${Turns}turns"
    $dir = Join-Path $Logs $label
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null

    $processes = @()
    $port = 0
    $hostProcess = $null
    for ($attempt = 1; $attempt -le 5; $attempt++) {
        if ($BasePort -gt 0) { $port = $BasePort } else { $port = Get-Random -Minimum 32000 -Maximum 35000 }
        $hostDir = Join-Path $dir "host"
        New-Item -ItemType Directory -Force -Path $hostDir | Out-Null
        $hostArgs = @(
            "--host", "--listen", "127.0.0.1", "--host-port", "$port",
            "--min-players", "$Clients", "--turns", "$Turns",
            "--player", "host", "--team", "0", "--world", "$World", "--seed", "$Seed",
            "--data-dir", (Quote $DataDir)
        )
        $hostProcess = Start-Process -FilePath $Bin -ArgumentList $hostArgs `
            -WorkingDirectory $hostDir -RedirectStandardOutput (Join-Path $hostDir "out.log") `
            -RedirectStandardError (Join-Path $hostDir "err.log") -PassThru -NoNewWindow
        $processes += $hostProcess
        Start-Sleep -Seconds 1
        if (-not $hostProcess.HasExited) { break }
        Write-Host "host-smoke: port $port is busy, retry ($attempt/5)"
        $hostProcess = $null
    }
    if (-not $hostProcess) {
        throw "could not start the host (all ports busy)"
    }
    Write-Host "host-smoke: $Clients client(s), $Turns turns, 127.0.0.1:$port"

    for ($i = 1; $i -lt $Clients; $i++) {
        $guestDir = Join-Path $dir "guest$i"
        New-Item -ItemType Directory -Force -Path $guestDir | Out-Null
        $guestArgs = @(
            "--connect", "127.0.0.1:$port", "--player", "guest$i", "--team", "$i",
            "--turns", "$Turns", "--data-dir", (Quote $DataDir)
        )
        $guestProcess = Start-Process -FilePath $Bin -ArgumentList $guestArgs `
            -WorkingDirectory $guestDir -RedirectStandardOutput (Join-Path $guestDir "out.log") `
            -RedirectStandardError (Join-Path $guestDir "err.log") -PassThru -NoNewWindow
        $processes += $guestProcess
        Start-Sleep -Milliseconds 300
    }

    $finished = Wait-Processes $processes $TimeoutSec
    if (-not $finished) {
        Stop-Processes $processes
        throw "the match did not finish within ${TimeoutSec}s"
    }
    Start-Sleep -Milliseconds 300

    $hostLog = Join-Path $dir "host\out.log"
    $hostLine = (Select-String -Path $hostLog -Pattern '^host-game: ' -ErrorAction SilentlyContinue |
        Select-Object -First 1)
    if (-not $hostLine) {
        throw "the host log has no host-game: line (see $hostLog)"
    }
    $hostLine = $hostLine.Line
    $failed = $false
    $hostTurns = Get-Field $hostLine "turns"
    $hostDesyncs = Get-Field $hostLine "desyncs"
    $hostTimeouts = Get-Field $hostLine "barrier_timeouts"
    if ($hostTurns -ne "$Turns" -or $hostDesyncs -ne "0" -or $hostTimeouts -ne "0") {
        Write-Host "host-smoke: FAIL host: turns=$hostTurns/$Turns desyncs=$hostDesyncs barrier_timeouts=$hostTimeouts" -ForegroundColor Red
        $failed = $true
    }
    $sharedRef = Get-SharedLine $hostLog
    if (-not $sharedRef) {
        Write-Host "host-smoke: the host log has no shared: line" -ForegroundColor Red
        $failed = $true
    }

    for ($i = 1; $i -lt $Clients; $i++) {
        $guestLog = Join-Path $dir "guest$i\out.log"
        $guestLine = (Select-String -Path $guestLog -Pattern '^network: ' -ErrorAction SilentlyContinue |
            Select-Object -First 1)
        if (-not $guestLine) {
            Write-Host "host-smoke: guest$i log has no network: line (see $guestLog)" -ForegroundColor Red
            $failed = $true
            continue
        }
        $guestLine = $guestLine.Line
        $gTurns = Get-Field $guestLine "turns"
        $gDesyncs = Get-Field $guestLine "desyncs"
        $gTimeouts = Get-Field $guestLine "barrier_timeouts"
        $gShared = Get-SharedLine $guestLog
        if ($gTurns -ne "$Turns" -or $gDesyncs -ne "0" -or $gTimeouts -ne "0") {
            Write-Host "host-smoke: FAIL guest$i`: turns=$gTurns/$Turns desyncs=$gDesyncs barrier_timeouts=$gTimeouts" -ForegroundColor Red
            $failed = $true
        }
        if ($sharedRef -and $gShared -ne $sharedRef) {
            Write-Host "host-smoke: FAIL guest$i`: shared digest differs" -ForegroundColor Red
            Write-Host "  host:  $sharedRef"
            Write-Host "  guest: $gShared"
            $failed = $true
        }
    }

    $reports = Get-ChildItem -Path $dir -Recurse -Filter "desync-turn*.txt" -ErrorAction SilentlyContinue
    if ($reports) {
        Write-Host "host-smoke: FAIL a desync audit report was written: $($reports[0].FullName)" -ForegroundColor Red
        $failed = $true
    }
    $auditLines = Select-String -Path (Join-Path $dir "*\out.log") -Pattern '^(host-)?audit: ' -ErrorAction SilentlyContinue
    if ($auditLines) {
        Write-Host "host-smoke: FAIL a desync audit line is present in the logs" -ForegroundColor Red
        $failed = $true
    }

    if ($failed) {
        throw "FAIL ($Clients client(s), $Turns turns) - logs: $dir"
    }

    Write-Host "HOST-SMOKE PASS clients=$Clients turns=$Turns"
    Write-Host "  shared: $sharedRef"
    Write-Host "  host: $hostLine"
    for ($i = 1; $i -lt $Clients; $i++) {
        $guestLine = (Select-String -Path (Join-Path $dir "guest$i\out.log") -Pattern '^network: ' |
            Select-Object -First 1).Line
        Write-Host "  guest$i`: $guestLine"
    }
}

# --- arguments ---------------------------------------------------------------
$cases = @()
switch ($Clients) {
    "all" { $cases = @(2, 3) }
    "both" { $cases = @(2, 3) }
    "2" { $cases = @(2) }
    "3" { $cases = @(3) }
    default { Fail "expected 2|3|all, got '$Clients'" }
}
if ($Turns -lt 1) { Fail "Turns must be positive, got $Turns" }

$failed = $false
foreach ($caseClients in $cases) {
    try {
        Invoke-Case $caseClients $Turns
    }
    catch {
        Write-Host "host-smoke: $($_.Exception.Message)" -ForegroundColor Red
        $failed = $true
    }
}
if ($failed) {
    Write-Host "host-smoke: FAILED" -ForegroundColor Red
    exit 1
}
Write-Host "host-smoke: OK ($($cases -join ' ') client(s), $Turns turns)"
exit 0
