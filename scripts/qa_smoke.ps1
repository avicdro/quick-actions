# QAs smoke para quick-actions — correrlo desde WSL via powershell.exe.
# Fases:
#   boot: lanza el exe normal y dumpea el estado de sus ventanas (debe
#         quedar SOLO tray: main oculta 1x1 offscreen + WS_EX_TOOLWINDOW).
#   show: lanza el exe con --show (abrir la GUI al arrancar) y dumpea
#         (debe existir la ventana visible 900x700 centrada, sin toolwindow).
#   tray: lanza el exe normal y A los 2s inyecta WM_COMMAND(id=1001 "Ayuda")
#         en la ventana oculta del tray (como un clic real) y dumpea.
#   close: idem tray (abrir) + 1.5s despues WM_CLOSE sobre la ventana main
#         (equivale a la X) y dumpea: debe volver a TOOLWINDOW offscreen.
#   dup: S1 instancia unica — lanza A, lanza B despues; B tiene que
#         salir solo (exit 0) en <=2s y quedar SOLO 1 proceso vivo.
# Escribe reporte en logs\qa_<fase>.txt y captura logs\qa_<fase>.png.
# Termina matando TODOS los quick-actions.exe (idempotente).
param([ValidateSet("boot","show","tray","close","dup")][string]$Phase = "boot", [int]$WaitSec = 3, [int]$CommandId = 1001, [int]$ResizeX = 0, [int]$ResizeY = 0)

$ErrorActionPreference = "Continue"
$exe   = "C:\quick-actions\quick-actions.exe"
$dir   = "C:\quick-actions"
$report= Join-Path $dir ("logs\qa_{0}.txt" -f $Phase)
$shot  = Join-Path $dir ("logs\qa_{0}.png" -f $Phase)

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

if (-not ('QAW' -as [type])) {
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class QAW {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr wp, IntPtr lp);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
}
"@
}

# 0) limpiar instancias previas
Get-Process quick-actions -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800

# 1) arrancar
$exeArgs = @(); if ($Phase -eq "show") { $exeArgs = @("--show") }
if ($exeArgs.Count -gt 0) {
  $p = Start-Process -FilePath $exe -WorkingDirectory $dir -ArgumentList $exeArgs -PassThru
} else {
  $p = Start-Process -FilePath $exe -WorkingDirectory $dir -PassThru
}
Start-Sleep -Seconds $WaitSec

$alive = -not $p.HasExited
"proceso pid=$($p.Id) alive=$alive" | Out-File $report -Encoding utf8

# Fase dup (S1): segunda instancia tiene que suicidarse al punto.
if ($Phase -eq "dup") {
  $p2 = Start-Process -FilePath $exe -WorkingDirectory $dir -PassThru
  Start-Sleep -Seconds 2
  "dup: segunda exited=$($p2.HasExited) exitcode=$($p2.ExitCode) (esperado True/0)" | Out-File $report -Append -Encoding utf8
  $count = (Get-Process quick-actions -ErrorAction SilentlyContinue | Measure-Object).Count
  "dup: procesos vivos=$count (esperado 1)" | Out-File $report -Append -Encoding utf8
  Get-Process quick-actions -ErrorAction SilentlyContinue | Stop-Process -Force
  "done" | Out-File $report -Append -Encoding utf8
  Get-Content $report
  return
}

if ($alive) {
  # Fase tray/close: clic simulado ("Ayuda") via WM_COMMAND en la ventana
  # oculta del tray (id 1001 = ID_TRAY_SHOW_HELP).
  $global:trayHwnd = [IntPtr]::Zero
  $global:mainHwnd = [IntPtr]::Zero
  if ($Phase -in @("tray","close")) {
    Start-Sleep -Seconds 1
    $procId = $p.Id
    $cb2 = [QAW+EnumProc]{ param($h, $l)
      $wpid = 0; [void][QAW]::GetWindowThreadProcessId($h, [ref]$wpid)
      if ($wpid -eq $procId) {
        $c = New-Object System.Text.StringBuilder 128; [void][QAW]::GetClassName($h, $c, 128)
        $t = New-Object System.Text.StringBuilder 128; [void][QAW]::GetWindowText($h, $t, 128)
        if ("$($c)" -eq "QuickActionsTrayWnd") { $global:trayHwnd = $h }
        if ("$($t)" -eq "QuickActions" -or "$($t)" -eq "Quick Actions") { $global:mainHwnd = $h }
      }
      return $true
    }
    [void][QAW]::EnumWindows($cb2, [IntPtr]::Zero)
    if ($global:trayHwnd -ne [IntPtr]::Zero) {
      # Si el titulo del main aun es "QuickActions" (estado de arranque),
      # WM_COMMAND funciona igual; si ya es "Quick Actions", lo busca igual.
      [void][QAW]::PostMessage($global:trayHwnd, 0x0111, [IntPtr]$CommandId, [IntPtr]::Zero)
      "tray: WM_COMMAND($CommandId) inyectado" | Out-File $report -Append -Encoding utf8
      Start-Sleep -Seconds 2

      # Captura del CONTENIDO de la ventana main (PrintWindow): valida el
      # render aunque haya un juego fullscreen por encima (CopyFromScreen
      # solo ve el compositor).
      if ($global:mainHwnd -ne [IntPtr]::Zero) {
        if ($ResizeX -gt 0 -and $ResizeY -gt 0) {
          [void][QAW]::SetWindowPos($global:mainHwnd, [IntPtr]0, 200, 60, $ResizeX, $ResizeY, 0x0040)
          Start-Sleep -Milliseconds 800
          $r2 = New-Object QAW+RECT; [void][QAW]::GetWindowRect($global:mainHwnd, [ref]$r2)
        } else {
          $r2 = New-Object QAW+RECT; [void][QAW]::GetWindowRect($global:mainHwnd, [ref]$r2)
        }
        $w = [Math]::Max(1, $r2.R - $r2.L)
        $hh = [Math]::Max(1, $r2.B - $r2.T)
        $bmp2 = New-Object System.Drawing.Bitmap $w, $hh
        $g2 = [System.Drawing.Graphics]::FromImage($bmp2)
        $hdc2 = $g2.GetHdc()
        [void][QAW]::PrintWindow($global:mainHwnd, $hdc2, 2)  # 2 = PW_RENDERFULLCONTENT
        $g2.ReleaseHdc($hdc2); $g2.Dispose()
        $shotWin = Join-Path $dir ("logs\qa_{0}_win.png" -f $Phase)
        $bmp2.Save($shotWin, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp2.Dispose()
        "windowshot=$shotWin (${w}x${hh})" | Out-File $report -Append -Encoding utf8
      }

      if ($Phase -eq "close") {
        [void][QAW]::PostMessage($global:mainHwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
        "close: WM_CLOSE inyectado en main" | Out-File $report -Append -Encoding utf8
        Start-Sleep -Seconds 2
      }
    } else {
      "tray: NO se encontro QuickActionsTrayWnd" | Out-File $report -Append -Encoding utf8
    }
  }

  # 2) dumpear ventanas del proceso
  $procId = $p.Id
  $lines = New-Object System.Collections.ArrayList
  $cb = [QAW+EnumProc]{ param($h, $l)
    $wpid = 0; [void][QAW]::GetWindowThreadProcessId($h, [ref]$wpid)
    if ($wpid -eq $procId) {
      $t = New-Object System.Text.StringBuilder 256; [void][QAW]::GetWindowText($h, $t, 256)
      $c = New-Object System.Text.StringBuilder 256; [void][QAW]::GetClassName($h, $c, 256)
      $r = New-Object QAW+RECT;                      [void][QAW]::GetWindowRect($h, [ref]$r)
      $ex = [QAW]::GetWindowLong($h, -20)
      $st = [QAW]::GetWindowLong($h, -16)
      $vis = [QAW]::IsWindowVisible($h)
      $tool = if (($ex -band 0x80) -ne 0) { "TOOLWINDOW" } else { "TASKBAR" }
      [void]$lines.Add(("  hwnd=0x{0:X} title='{1}' class='{2}' vis={3} {4} rect=({5},{6})-({7},{8}) style=0x{9:X} ex=0x{10:X}" -f $h.ToInt64(), $t, $c, $vis, $tool, $r.L, $r.T, $r.R, $r.B, $st, $ex))
    }
    return $true
  }
  [void][QAW]::EnumWindows($cb, [IntPtr]::Zero)
  if ($lines.Count -eq 0) { [void]$lines.Add("  (sin ventanas top-level del proceso)") }
  $lines | Out-File $report -Append -Encoding utf8

  # 3) screenshot del escritorio
  $vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bmp = New-Object System.Drawing.Bitmap $vs.Width, $vs.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($vs.Left, $vs.Top, 0, 0, $bmp.Size)
  $bmp.Save($shot, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  "screenshot=$shot" | Out-File $report -Append -Encoding utf8
} else {
  "PROCESO MUERTO al arrancar (exit code $($p.ExitCode))" | Out-File $report -Append -Encoding utf8
}

# 4) salir
Get-Process quick-actions -ErrorAction SilentlyContinue | Stop-Process -Force
"done" | Out-File $report -Append -Encoding utf8
Get-Content $report