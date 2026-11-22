# Prepares the headless dedicated-server run directory (MAX_HEADLESS build).
# -Ai: free team slots become computer opponents (TEAM_TYPE_COMPUTER) and the lobby starts
#      with a single remote player.
# -DesyncTest: the arbiter corrupts a local unit value at turn 2 to exercise the desync-restart
#      flow (packet 51 + reload from the autosave).
param([switch]$Ai, [switch]$DesyncTest)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot | Split-Path -Parent
Set-Location $root

$exe = "build\cross-winxp-x64-server-debug\max-port.exe"
if (-not (Test-Path $exe)) { throw "Server build output not found: $exe (build first)" }

$run = Join-Path $root "build\server-run"
if (Test-Path $run) { Remove-Item $run -Recurse -Force }
New-Item -ItemType Directory -Path $run | Out-Null

Copy-Item $exe $run
Copy-Item "build\cross-winxp-x64-server-debug\max-port.debug" $run -ErrorAction SilentlyContinue
foreach ($f in @(".portable", "PATCHES.RES", "settings.json", "language.json", "help.json", "maxint.json", "LICENSE")) {
    Copy-Item (Join-Path $root "assets\$f") (Join-Path $run $f)
}

$settingsPath = Join-Path $run "settings.json"
$settings = Get-Content $settingsPath -Raw | ConvertFrom-Json

$settings.settings.SETUP.player_name = "Server"
$settings.settings.SETUP.intro_movie = 0
$settings.settings.SETUP.disable_music = 1
$settings.settings.SETUP.disable_fx = 1
$settings.settings.SETUP.disable_voice = 1
$settings.settings.SETUP.disable_movie = 1
$settings.settings.SETUP.game_data = "D:/dev/max-port-debug/original/MAX"

$settings.settings.OPTIONS.world = 0
$settings.settings.OPTIONS.timer = 30
$settings.settings.OPTIONS.endturn = 15
$settings.settings.OPTIONS.play_mode = 1

foreach ($t in @("red","green","blue","gray")) {
    if ($Ai -and $t -ne "red") {
        # Free slots host computer opponents when the AI mode is requested. A fixed clan keeps the
        # computer opponents deterministic across the peers (random clans draw from the RNG).
        $settings.settings.TEAMS."${t}_team_player" = 2
        $settings.settings.TEAMS."${t}_team_clan" = 1
    } else {
        $settings.settings.TEAMS."${t}_team_player" = 0
        $settings.settings.TEAMS."${t}_team_clan" = 1
    }
    $settings.settings.TEAMS."${t}_team_name" = ""
}

$settings.settings.GRAPHICS_SETTINGS.screen_mode = 0

$settings.settings.NETWORK_SETTINGS.host_port = 31554
$settings.settings.NETWORK_SETTINGS.host_address = "127.0.0.1"

if (-not $settings.settings.DEBUG) {
    $settings.settings | Add-Member -MemberType NoteProperty -Name DEBUG -Value ([pscustomobject]@{})
}
$settings.settings.DEBUG | Add-Member -MemberType NoteProperty -Name log_file_debug -Value 1 -Force
$settings.settings.DEBUG | Add-Member -MemberType NoteProperty -Name server_mode -Value 1 -Force
$settings.settings.DEBUG | Add-Member -MemberType NoteProperty -Name server_min_players -Value ($(if ($Ai) { 1 } else { 2 })) -Force
$settings.settings.DEBUG | Add-Member -MemberType NoteProperty -Name server_desync_test -Value ($(if ($DesyncTest) { 1 } else { 0 })) -Force

$settings | ConvertTo-Json -Depth 10 | Set-Content $settingsPath -Encoding UTF8

Write-Host "Server run directory prepared: $run"
Write-Host ("server_mode={0}, min_players={1}, port={2}, game_data={3}" -f
    $settings.settings.DEBUG.server_mode, $settings.settings.DEBUG.server_min_players,
    $settings.settings.NETWORK_SETTINGS.host_port, $settings.settings.SETUP.game_data)
