# QA live: lanza quick-actions y dumpea SIN matar el proceso (para leer el
# log con el appender async vivo). Fases: boot (plain) / show (--show).
param([ValidateSet("boot","show")][string]$Phase = "boot", [int]$WaitSec = 6)

$ErrorActionPreference = "Continue"
$exe = "C:\quick-actions\quick-actions.exe"
$dir = "C:\quick-actions"
$report = Join-Path $dir ("logs\qa_live_{0}.txt" -f $Phase)

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class QAWLive {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
}
"@

Get-Process quick-actions -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800

$exeArgs = @(); if ($Phase -eq "show") { $exeArgs = @("--show") }
if ($exeArgs.Count -gt 0) {
  $p = Start-Process -FilePath $exe -WorkingDirectory $dir -ArgumentList $exeArgs -PassThru
} else {
  $p = Start-Process -FilePath $exe -WorkingDirectory $dir -PassThru
}
Start-Sleep -Seconds $WaitSec

"proceso pid=$($p.Id) alive=$(-not $p.HasExited)" | Out-File $report -Encoding utf8

$procId = $p.Id
$lines = New-Object System.Collections.ArrayList
$cb = [QAWLive+EnumProc]{ param($h, $l)
  $wpid = 0; [void][QAWLive]::GetWindowThreadProcessId($h, [ref]$wpid)
  if ($wpid -eq $procId) {
    $t = New-Object System.Text.StringBuilder 256; [void][QAWLive]::GetWindowText($h, $t, 256)
    $c = New-Object System.Text.StringBuilder 256; [void][QAWLive]::GetClassName($h, $c, 256)
    $r = New-Object QAWLive+RECT;                  [void][QAWLive]::GetWindowRect($h, [ref]$r)
    $ex = [QAWLive]::GetWindowLong($h, -20)
    $vis = [QAWLive]::IsWindowVisible($h)
    $tool = if (($ex -band 0x80) -ne 0) { "TOOLWINDOW" } elseif (($ex -band 0x40000) -ne 0) { "APPWINDOW" } else { "?" }
    [void]$lines.Add(("  title='{0}' class='{1}' vis={2} {3} rect=({4},{5})-({6},{7}) ex=0x{8:X}" -f $t, $c, $vis, $tool, $r.L, $r.T, $r.R, $r.B, $ex))
  }
  return $true
}
[void][QAWLive]::EnumWindows($cb, [IntPtr]::Zero)
$lines | Out-File $report -Append -Encoding utf8
Get-Content $report