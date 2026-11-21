# Компакция docker_data.vhdx (Docker Desktop + WSL2).
#   powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\docker-vhdx-compact.ps1 [-NoRestart]
param(
    [string]$Vhdx = "C:\Users\Solo Sovey\AppData\Local\Docker\wsl\disk\docker_data.vhdx",
    [switch]$NoRestart
)
$ErrorActionPreference = 'Continue'

function SizeGB([string]$p) {
    if (-not (Test-Path $p)) { return $null }
    return [math]::Round((Get-Item $p).Length / 1GB, 1)
}
function TryLockFree([string]$p) {
    try {
        $fs = [System.IO.File]::Open($p, 'Open', 'ReadWrite', 'None')
        $fs.Close()
        return $true
    } catch { return $false }
}

if (-not (Test-Path $Vhdx)) { Write-Host "нет $Vhdx"; exit 1 }
Write-Host ("before: {0} GB" -f (SizeGB $Vhdx))

Write-Host '== остановка Docker Desktop =='
Get-Process 'Docker Desktop','com.docker.backend','com.docker.build','com.docker.dev-envs' -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3

Write-Host '== wsl --shutdown =='
wsl --shutdown 2>&1 | Out-Null

Write-Host '== ожидание освобождения VHDX (до 180 с) =='
$deadline = (Get-Date).AddSeconds(180)
$free = $false
$attempt = 0
while ((Get-Date) -lt $deadline) {
    $attempt++
    # Вывод wsl -l -v приходит в UTF-16-подобном виде (с NUL между символами) —
    # без очистки regex никогда не совпадёт (баг прежней версии скрипта).
    $table = ((wsl -l -v 2>&1 | Out-String) -replace "`0", '')
    $stopped = $table -match 'docker-desktop\s+Stopped'
    $locked = -not (TryLockFree $Vhdx)
    if ($attempt % 5 -eq 0) {
        Write-Host ("  t+{0}s: distro_stopped={1} file_locked={2} vmmem={3}" -f (($attempt - 1) * 3), $stopped, $locked, (Get-Process vmmem* -ErrorAction SilentlyContinue | Measure-Object).Count)
    }
    if (-not $locked -and $stopped) { $free = $true; break }
    # WSLService может удерживать виртуальный диск, даже когда дистрибутивы
    # остановлены: пробуем перезапустить службу (требует прав администратора).
    if ($attempt -eq 20) {
        Write-Host '  служба WSLService всё ещё держит файл — перезапуск службы'
        try { Restart-Service WSLService -Force -ErrorAction Stop; Start-Sleep -Seconds 5 } catch { Write-Host ("  не удалось перезапустить WSLService: {0}" -f $_.Exception.Message) }
    }
    Start-Sleep -Seconds 3
}
if (-not $free) {
    Write-Host 'VHDX не освободился — повторите позже (WSL/VM держит файл).'
    if (-not $NoRestart) {
        Write-Host '== запуск Docker Desktop =='
        Start-Process "C:\Program Files\Docker\Docker\Docker Desktop.exe" -ErrorAction SilentlyContinue
    }
    exit 2
}

$done = $false
if (Get-Command Optimize-VHD -ErrorAction SilentlyContinue) {
    Write-Host '== Optimize-VHD -Mode Full =='
    try {
        Optimize-VHD -Path $Vhdx -Mode Full -ErrorAction Stop
        $done = $true
    } catch {
        Write-Host ("Optimize-VHD: {0}" -f $_.Exception.Message)
    }
} else {
    Write-Host 'Optimize-VHD недоступен.'
}

if (-not $done) {
    Write-Host '== diskpart compact vdisk =='
    $script = @"
select vdisk file="$Vhdx"
attach vdisk readonly
compact vdisk
detach vdisk
exit
"@
    $tmp = Join-Path $env:TEMP 'compact-vdisk.txt'
    Set-Content -Path $tmp -Value $script -Encoding ASCII
    diskpart /s $tmp
}

Write-Host '== sparse flag (WSL 3.0+) =='
wsl --manage docker-desktop --set-sparse true 2>&1 | Select-Object -First 2

Write-Host ("after: {0} GB" -f (SizeGB $Vhdx))
if (-not $NoRestart) {
    Write-Host '== запуск Docker Desktop =='
    Start-Process "C:\Program Files\Docker\Docker\Docker Desktop.exe" -ErrorAction SilentlyContinue
}
Write-Host 'done'
