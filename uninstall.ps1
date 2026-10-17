# uninstall.ps1 — desinstala Quick Actions del usuario actual (sin admin).
#
# - Detiene la app si esta corriendo
# - Limpia la entrada de auto-arranque en HKCU Run (por si quedo activa)
# - Borra %LOCALAPPDATA%\QuickActions (exe + config + historial + logs)
#
# Ojo: borra TODO el folder, incluyendo tu config.toml personalizado y
# el historial de correcciones (history.json).
#
# Uso:
#   powershell -ExecutionPolicy Bypass -File .\uninstall.ps1
#   powershell -ExecutionPolicy Bypass -File .\uninstall.ps1 -KeepData
#     (borra el exe pero conserva config.toml e history.json)

[CmdletBinding()]
param(
    # Carpeta instalada.
    [string]$Destination = (Join-Path $env:LOCALAPPDATA 'QuickActions'),
    # Conservar config.toml e history.json (backup junto a Documents).
    [switch]$KeepData
)

$ErrorActionPreference = 'Stop'

Write-Host '=== Quick Actions - desinstalacion ==='

# --- 1. Detener la app ---
Get-Process quick-actions -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 400
Write-Host 'App detenida.'

# --- 2. Limpiar auto-arranque (HKCU, sin admin) ---
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
Remove-ItemProperty -Path $runKey -Name 'QuickActions' -ErrorAction SilentlyContinue
Write-Host 'Auto-arranque limpio.'

# --- 3. Borrar la instalacion ---
if (Test-Path $Destination) {
    if ($KeepData) {
        $backupDir = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'QuickActionsBackup'
        New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
        foreach ($name in @('config.toml', 'history.json')) {
            $src = Join-Path $Destination $name
            if (Test-Path $src) {
                Copy-Item $src (Join-Path $backupDir $name) -Force
                Write-Host "Preservado: $name -> $backupDir"
            }
        }
    }

    Write-Host "Borrando $Destination ..."
    Remove-Item -Recurse -Force $Destination
}
else {
    Write-Host 'No habia carpeta instalada (nada que borrar).'
}

Write-Host 'Desinstalacion completa.'
