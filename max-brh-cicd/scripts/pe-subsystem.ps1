#requires -version 5.1
<#
.SYNOPSIS
  Проверка подсистемы PE-файла (Subsystem из IMAGE_OPTIONAL_HEADER) без dumpbin.

.DESCRIPTION
  Значения Subsystem: 2 = Windows GUI (окно без консоли), 3 = Windows Console
  (stdout/stderr в терминале). Поле лежит в Optional Header по смещению 0x44 (68)
  и для PE32, и для PE32+ (в PE32+ отсутствует BaseOfData, но ImageBase шире —
  смещение Subsystem не меняется). Optional Header начинается после сигнатуры
  «PE\0\0» (4 байта) и IMAGE_FILE_HEADER (20 байт), поэтому абсолютное смещение:

      e_lfanew (по DOS-смещению 0x3C) + 4 + 20 + 0x44 = e_lfanew + 0x5C

  Скрипт годится и для запуска отдельно, и для подключения из сборочных
  скриптов (dot-source): после проверки функция Get-PeSubsystem остаётся
  доступна вызывающему.

.PARAMETER Path
  Один или несколько PE-файлов (.exe/.dll).

.PARAMETER Expected
  Ожидаемая подсистема: 2 (GUI) или 3 (Console). Несовпадение — ошибка (exit 1).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\pe-subsystem.ps1 `
      -Path release\max_app-1.0.0-windows-x86_64-ui\max-brh.exe -Expected 2

.EXAMPLE
  . .\max-brh-cicd\scripts\pe-subsystem.ps1 -Path .\max-brh.exe -Expected 3
#>
param(
    [Parameter(Mandatory = $true, Position = 0)][string[]]$Path,
    [int]$Expected = 0
)

$machineNames = @{
    0x014c = "x86"
    0x8664 = "x64"
    0x01c4 = "ARM"
    0xaa64 = "ARM64"
}
$subsystemNames = @{
    1 = "Native"
    2 = "Windows GUI"
    3 = "Windows Console"
}

function Get-PeSubsystem {
    param([Parameter(Mandatory = $true)][string]$File)

    $resolved = (Resolve-Path -LiteralPath $File -ErrorAction Stop).Path
    $bytes = [System.IO.File]::ReadAllBytes($resolved)

    if ($bytes.Length -lt 0x40) {
        throw "$resolved`: файл короче DOS-заголовка"
    }
    if ($bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) {
        throw "$resolved`: нет MZ-заголовка"
    }

    $lfanew = [BitConverter]::ToInt32($bytes, 0x3C)
    $optional = $lfanew + 4 + 20
    if ($lfanew -lt 0x40 -or ($optional + 0x46) -gt $bytes.Length) {
        throw "$resolved`: битый e_lfanew=$lfanew (файл $($bytes.Length) байт)"
    }
    if ($bytes[$lfanew] -ne 0x50 -or $bytes[$lfanew + 1] -ne 0x45 -or
        $bytes[$lfanew + 2] -ne 0 -or $bytes[$lfanew + 3] -ne 0) {
        throw "$resolved`: нет сигнатуры PE\0\0 по смещению e_lfanew=$lfanew"
    }

    $machine = [int][BitConverter]::ToUInt16($bytes, $lfanew + 4)
    $magic = [int][BitConverter]::ToUInt16($bytes, $optional)
    $subsystem = [int][BitConverter]::ToUInt16($bytes, $optional + 0x44)

    [pscustomobject]@{
        Path      = $resolved
        Machine   = if ($machineNames.ContainsKey($machine)) { $machineNames[$machine] } else { "0x{0:X4}" -f $machine }
        Magic     = if ($magic -eq 0x20B) { "PE32+" } elseif ($magic -eq 0x10B) { "PE32" } else { "0x{0:X4}" -f $magic }
        Subsystem = $subsystem
        Name      = if ($subsystemNames.ContainsKey($subsystem)) { $subsystemNames[$subsystem] } else { "?" }
    }
}

$failed = $false
foreach ($file in $Path) {
    try {
        $pe = Get-PeSubsystem -File $file
    } catch {
        Write-Host "PE: $($_.Exception.Message)" -ForegroundColor Red
        $failed = $true
        continue
    }

    $line = "PE: {0} — {1} {2}, Subsystem={3} ({4})" -f $pe.Path, $pe.Magic, $pe.Machine, $pe.Subsystem, $pe.Name
    if ($Expected -ne 0 -and $pe.Subsystem -ne $Expected) {
        Write-Host ("{0}; ожидалось {1}" -f $line, $Expected) -ForegroundColor Red
        $failed = $true
    } else {
        Write-Host $line
    }
}

if ($failed) {
    exit 1
}
