# Quick Actions

Suite de atajos globales para IA y productividad en **Windows**, escrita en **Rust**. Queda residente en segundo plano y te da accesso instantáneo a Ollama (LLM local) sobre cualquier texto que tengas seleccionado, en la app que sea.

> Migración de scripts AutoHotkey v2 a un único ejecutable Rust con interfaz eframe/egui.

## ✨ Funcionalidades

- 🔥 **Hotkeys globales** vía hook nativo `WH_KEYBOARD_LL` (WinAPI) — funciona en cualquier app
- 🤖 **Integración con Ollama** — correcciones, traducciones, resúmenes y generación de texto
- 📋 **Gestión de portapapeles** — backup/restore del clipboard al copiar la selección
- 🖥️ **Ventanas de resultado** (egui) — original arriba, resultado abajo, botón copiar, Escape para cerrar
- 🕓 **Historial persistente** — guardado en JSON con escritura atómica
- 🔔 **Toasts de estado** con spinner animado durante las peticiones
- 🥥 **System tray icon** con icono propio embebido en el exe (`.ico`, menú completo: ayuda/config/autostart/pausa/modelo)
- 🔒 **Instancia única** (mutex de Windows): un segundo arranque enfoca la ventana abierta y sale, no duplica tray/hook
- ⚙️ **Config externa** `config.toml` — edita sin recompilar

## ⌨️ Atajos (modificador configurable — default `Alt`)

| Atajo | Acción |
|-------|--------|
| `Alt + G` | Corregir la selección → pega el resultado corregido |
| `Alt + T` | Traducir la selección → ventana con resultado |
| `Alt + R` | Resumir la selección → ventana con resultado |
| `Alt + Enter` | Generar respuesta a partir de la selección |
| `Alt + V` | Historial de correcciones (doble click = copiar) |
| `Alt + H` | Ayuda |

> Modificador alternativo: `modifier = "capslock"` en `[hotkeys]` (con sub-modo `hold` o `toggle` — ver config). En modo toggle CapsLock deja de alternar mayúsculas.

> Tip: puedes añadir contexto extra a la corrección encerrando texto entre corchetes `[contexto]`.

## 📦 Requisitos

- **Windows 10/11** (los hotkeys y el tray usan WinAPI)
- **Ollama** corriendo localmente (`ollama serve`, por defecto en `http://localhost:11434`)
- Un modelo instalado en Ollama, p. ej.: `ollama pull gemma4:cloud`

## 🚀 Instalación

Descarga los archivos de [Releases](https://github.com/avicdro/quick-actions/releases) (`quick-actions.exe`, `config.toml`, `install.ps1`, `uninstall.ps1`) a una carpeta y corre:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
# activando auto-arranque en el mismo paso:
powershell -ExecutionPolicy Bypass -File .\install.ps1 -Autostart
```

- Instala en `%LOCALAPPDATA%\QuickActions` (sin admin), detiene instancias previas y arranca la app al terminar
- Si ya hay una config instalada, **se conserva** (no pisa tu configuración)
- Auto-arranque: toggle "Iniciar con Windows" en el tray, o `-Autostart` en el instalador
- Desinstalar: `powershell -ExecutionPolicy Bypass -File .\uninstall.ps1` (con `-KeepData` respalda `config.toml` e `history.json` en `Documents\QuickActionsBackup`)

## ⚙️ Configuración

Coloca `config.toml` junto al `.exe`:

```toml
[hotkeys]
enabled = true
modifier = "alt"        # "alt" (default) o "capslock" (+ capsmode hold/toggle)

[ollama]
model = "gemma4:cloud"                          # Modelo a usar
endpoint = "http://localhost:11434/api/generate" # Endpoint de Ollama
timeout_seconds = 60

[ui]
font_size = 16
window_width = 800
window_height = 600
history_max_entries = 50

[paths]
chrome = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"

[app]
autostart = false        # Auto-arranque con Windows (tambien via tray)
check_updates = true     # Buscar releases nuevas al arrancar
repository = "avicdro/quick-actions"
```

## 🛠️ Build

### Script automático (recomendado, cross-compile desde WSL2)

```bash
./build.sh
```

Compila en release para Windows y copia `quick-actions.exe` + `config.toml` a `C:\quick-actions\`.

### Manual desde Linux (WSL2)

```bash
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
cp target/x86_64-pc-windows-gnu/release/quick-actions.exe /mnt/c/quick-actions/
```

> Requiere `mingw-w64` (paquete completo: trae `x86_64-w64-mingw32-gcc`, `windres` y `ar`; las dos últimas solo se usan para embutir el icono del tray).

### Desde Windows

```bash
cargo build --release
```

## ▶️ Uso

1. Ejecuta `quick-actions.exe` (verás el icono en el tray)
2. Asegúrate de que Ollama está corriendo
3. Selecciona texto en cualquier app y usa los atajos

## 🔧 Solución de problemas

### Notificación de NVIDIA ("Pulse Alt+Z") al abrir la GUI

NVIDIA App puede mostrar su toast de superposición sobre `quick-actions.exe`
porque inyecta su overlay en procesos que presentan una swapchain D3D12/Vulkan
(aunque la app pinte en la iGPU). No existe API pública para excluirse.

- Reiniciar el PC **no** limpia la detección: vive en
  `%LOCALAPPDATA%\NVIDIA Corporation\NVIDIA App\NvBackend`. Cierra NVIDIA App
  del tray, borra esa carpeta y relánzala (se regenera sola).
- Revisa en NVIDIA App → *Juegos* si existe una entrada para
  `quick-actions.exe` (elimínala o desactiva la overlay para esa app).
- El log del app registra el adapter real de wgpu en cada arranque
  (`wgpu_core::instance`); para un diagnóstico más profundo:
  `RUST_LOG=wgpu_core=debug,wgpu_hal=debug`.

### Ver qué build estoy corriendo

- Tray → Ayuda/Ajustes → pestaña **Ajustes → App** ("Versión: v0.2.4 (abcdef1)")
- Primera línea del log `logs/` ("Iniciando Quick Actions v0.2.4 (abcdef1)")

## 🔖 Versionado

Semver pre-1.0 con reglas propias del proyecto:

- **Patch ilimitado** (`v0.2.N` → crece por release, sin techo: 0.2.4, 0.2.5, … 0.2.100)
- **Minor** (`0.2` → `0.3`) solo para hitos que cierran una etapa de producto
- **`v1.0.0` es una decisión explícita** — se alcanza cuando la base es estable y pulida, nunca automáticamente
- Cada build incrusta el **commit corto de git** (`vX.Y.Z (abcdef1)`), visible en Ajustes y en el log: la granularidad real entre releases la da el hash, no el número
- Las tags de release son `vX.Y.Z` (sin ceros a la izquierda ni sufijos: `parse_version` del update-check espera ese formato)

## 🗺️ Roadmap

- [x] Fase 1 — AI Suite Ollama (MVP)
- [x] Fase 2 — Historial persistente, tray icon, toasts
- [ ] Fase 3 — Gemini Chrome (nueva ventana/pestaña)
- [ ] Fase 4 — App Launcher (panel flotante)
- [ ] Fase 5 — Notifications Overlay (Slack/Teams)

## 📄 Licencia

MIT
