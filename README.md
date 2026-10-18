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
- ⚙️ **Config externa** `config.toml` — edita sin recompilar

## ⌨️ Atajos (todas usan CapsLock como modificador)

| Atajo | Acción |
|-------|--------|
| `CapsLock + G` | Corregir la selección → pega el resultado corregido |
| `CapsLock + T` | Traducir la selección → ventana con resultado |
| `CapsLock + R` | Resumir la selección → ventana con resultado |
| `CapsLock + Enter` | Generar respuesta a partir de la selección |
| `CapsLock + V` | Historial de correcciones (doble click = copiar) |
| `CapsLock + H` | Ayuda |

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
modifier = "capslock"   # "capslock" (default) o "alt"
capsmode = "hold"       # "hold": mantener presionado + tecla
                        # "toggle": tap latched; la proxima tecla lo dispara
                        #  (en toggle CapsLock ya no alterna mayusculas)

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

## 🗺️ Roadmap

- [x] Fase 1 — AI Suite Ollama (MVP)
- [x] Fase 2 — Historial persistente, tray icon, toasts
- [ ] Fase 3 — Gemini Chrome (nueva ventana/pestaña)
- [ ] Fase 4 — App Launcher (panel flotante)
- [ ] Fase 5 — Notifications Overlay (Slack/Teams)

## 📄 Licencia

MIT
