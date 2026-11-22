#requires -version 5.1
# Хелперы для управления окнами max_app (SDL3) из PowerShell:
# фокус, клики по клиентским координатам, клавиши, скриншот окна.
# Только для тестовых прогонов: build/** в .gitignore.
Add-Type -AssemblyName System.Drawing
if (-not ('UiWin' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class UiWin {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
'@
}
$script:LEFTDOWN=0x0002; $script:LEFTUP=0x0004; $script:RIGHTDOWN=0x0008; $script:RIGHTUP=0x0010
$script:KEYUP=0x0002

function Get-UiHwnd([int]$ProcessId) {
    $p = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $p) { throw "нет процесса $ProcessId" }
    $p.Refresh()
    return ,$p.MainWindowHandle
}

function Move-UiWindow([int]$ProcessId, [int]$X, [int]$Y) {
    $h = Get-UiHwnd $ProcessId
    # SWP_NOSIZE(0x1) | SWP_SHOWWINDOW(0x40)
    [void][UiWin]::SetWindowPos($h, [IntPtr]::Zero, $X, $Y, 0, 0, 0x0041)
}

function Focus-UiWindow([int]$ProcessId) {
    $h = Get-UiHwnd $ProcessId
    [void][UiWin]::ShowWindow($h, 9)          # SW_RESTORE
    [void][UiWin]::SetForegroundWindow($h)
    Start-Sleep -Milliseconds 250
    return $h
}

function Send-UiKey([int]$ProcessId, [int]$Vk, [int]$DelayMs = 120) {
    $h = Focus-UiWindow $ProcessId
    [UiWin]::keybd_event([byte]$Vk, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 40
    [UiWin]::keybd_event([byte]$Vk, 0, 0x0002, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds $DelayMs
}

function Send-UiText([int]$ProcessId, [string]$Text) {
    foreach ($ch in $Text.ToCharArray()) {
        $vk = [int][System.Char]::ToUpperInvariant($ch)
        Send-UiKey $ProcessId $vk 60
    }
}

function Click-UiClient([int]$ProcessId, [int]$X, [int]$Y, [switch]$Right) {
    $h = Focus-UiWindow $ProcessId
    $pt = New-Object UiWin+POINT
    $pt.X = 0; $pt.Y = 0
    [void][UiWin]::ClientToScreen($h, [ref]$pt)
    $sx = $pt.X + $X; $sy = $pt.Y + $Y
    [void][UiWin]::SetCursorPos($sx, $sy)
    Start-Sleep -Milliseconds 120
    if ($Right) {
        [UiWin]::mouse_event($script:RIGHTDOWN, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 40
        [UiWin]::mouse_event($script:RIGHTUP, 0, 0, 0, [UIntPtr]::Zero)
    } else {
        [UiWin]::mouse_event($script:LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 40
        [UiWin]::mouse_event($script:LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    }
    Start-Sleep -Milliseconds 200
}

function Save-UiShot([int]$ProcessId, [string]$Path) {
    $h = Get-UiHwnd $ProcessId
    $r = New-Object UiWin+RECT
    [void][UiWin]::GetWindowRect($h, [ref]$r)
    $w = $r.Right - $r.Left; $hh = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    return $Path
}
