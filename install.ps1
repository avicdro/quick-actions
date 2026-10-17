# install.ps1 — instalador de Quick Actions para el usuario actual (sin admin).
#
# Copia el exe a %LOCALAPPDATA%\QuickActions y coloca config.toml junto a el
# (la app lee el config del dir del exe). El auto-arranque se activa desde
# el tray ("Iniciar con Windows") o con -Autostart (escribe
# [app] autostart = true en el config copiado, sin BOM).
#
# Uso:
#   powershell -ExecutionPolicy Bypass -File .\install.ps1
#   powershell -ExecutionPolicy Bypass -File .\install.ps1 -Autostart
#   powershell -ExecutionPolicy Bypass -File .\install.ps1 -ExePath C:\ruta\quick-actions.exe

[CmdletBinding()]
param(
    # Ruta del exe a instalar (default: resuelto automaticamente).
    [string]$ExePath = '',
    # Carpeta de destino.
    [string]$Destination = (Join-Path $env:LOCALAPPDATA 'QuickActions'),
    # Setear [app] autostart = true en el config (equivale al toggle del tray).
    [switch]$Autostart
)

$ErrorActionPreference = 'Stop'

# $PSScriptRoot es automatica (potentialmente read-only): fallback en var propia
$scriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }

Write-Host '=== Quick Actions - instalacion ==='

# --- 1. Resolver el exe ---
if ([string]::IsNullOrWhiteSpace($ExePath)) {
    $candidates = @(
        (Join-Path $scriptDir 'quick-actions.exe'),
        (Join-Path $scriptDir 'dist\quick-actions.exe'),
        (Join-Path 'C:\' 'quick-actions\quick-actions.exe'),
        (Join-Path (Split-Path $scriptDir -Parent) 'target\x86_64-pc-windows-gnu\release\quick-actions.exe')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            $ExePath = $candidate
            break
        }
    }
}

if ([string]::IsNullOrWhiteSpace($ExePath) -or -not (Test-Path $ExePath)) {
    throw 'No encontre quick-actions.exe. Pasalo con -ExePath C:\ruta\quick-actions.exe'
}

$ExePath = (Resolve-Path $ExePath).Path
$exeSourceDir = Split-Path $ExePath -Parent
Write-Host "Exe: $ExePath"
Write-Host "Destino: $Destination"

# --- 2. Parar instancia corriendo (si el exe esta en uso no se puede copiar) ---
Get-Process quick-actions -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 400

# --- 3. Crear destino y copiar exe ---
New-Item -ItemType Directory -Path $Destination -Force | Out-Null
Copy-Item $ExePath (Join-Path $Destination 'quick-actions.exe') -Force
Write-Host 'Exe copiado.'

# --- 4. Config: solo si no existe (no pisar la configuracion del usuario) ---
$configDest = Join-Path $Destination 'config.toml'
if (-not (Test-Path $configDest)) {
    $configSrc = Join-Path $exeSourceDir 'config.toml'
    if (Test-Path $configSrc) {
        Copy-Item $configSrc $configDest
        Write-Host 'Config copiada desde el fuente.'
    }
    else {
        Write-Host 'Sin config por copiar: la app crea una por defecto al primer arranque.'
    }
}
else {
    Write-Host 'Config existente conservada.'
}

# --- 5. Auto-arranque opt-in: [app] autostart = true ---
# Solo reescribe si la linea "autostart" ya existe (la traen el template y
# el default de la app). Sin tocar nada si no esta (sin riesgo de romper
# el toml): ahi el toggle del tray es el camino.
if ($Autostart -and (Test-Path $configDest)) {
    $noBom = [System.Text.UTF8Encoding]::new($false)
    $content = [System.IO.File]::ReadAllText($configDest)

    if ($content -match '(?m)^autostart\s*=\s*(true|false)\s*$') {
        $content = [regex]::Replace($content, '(?m)^autostart\s*=\s*(true|false)\s*$', 'autostart = true')
        # UTF-8 SIN BOM (la lib toml de Rust no acepta BOM)
        [System.IO.File]::WriteAllText($configDest, $content, $noBom)
        Write-Host 'Auto-arranque: activado en config ([app] autostart = true).'
    }
    else {
        Write-Host 'Auto-arranque: no encontre la linea "autostart" en el config.'
        Write-Host 'Activalo igual desde el tray: Iniciar con Windows.'
    }
}

# --- 6. Resumen y arranque ---
Write-Host ''
Write-Host 'Instalacion completa.'
Write-Host "- Exe/config: $Destination"
Write-Host "- Logs: $(Join-Path $Destination 'logs')"
Write-Host '- Auto-arranque: toggle "Iniciar con Windows" en el tray'
Write-Host ''
Start-Process (Join-Path $Destination 'quick-actions.exe')
