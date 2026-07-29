# Quick Actions — Plan y Roadmap

> **Proyecto:** Migracion de scripts AutoHotkey v2 a Rust.
> **Ruta original AHK:** `V:\code\personal\ahk` (montado en `/mnt/v/code/personal/ahk`)
> **Compilacion:** Windows x86_64 (cross-compile desde Linux con MinGW).
> **Salida del .exe:** `C:\quick-actions\quick-actions.exe`

---

## Vision

Reemplazar toda la suite AHK con **un unico ejecutable Rust** que se queda residente en segundo plano. Prioridad absoluta: `ai-suite-ollama.ahk`. El resto se migra por fases.

---

## Arquitectura (threads y canales)

```
Thread Principal (main.rs)
│
├─→ Thread "hotkeys" (win_hook.rs)
│   │   Escucha WH_KEYBOARD_LL global
│   │   CapsLock+G → mpsc::send(HotkeyCommand::Correct)
│   │   CapsLock+T → mpsc::send(HotkeyCommand::Translate)
│   ...
│   └─→ Canal mpsc::UnboundedSender<HotkeyCommand> ───────────┐
│                                                               ▼
└─→ EventLoop de eframe (ui.rs)                           Thread "tokio runtime"
    │   Recibe HotkeyCommand via mpsc                     │   Recibe solicitudes HTTP
    │   Crea/destruye ventanas (result_view, etc.)        │   ollama.rs::generate()
    │                                                      │
    │   Si el comando requiere HTTP:                       │
    └─→ spawnea tokio::task ───────────────────────────────┘
                                                              Retorna String
                                                              │
    ┌─────────────────────────────────────────────────────────┘
    ▼
    Actualiza ventana de resultado con el texto de Ollama
```

---

## Fases de implementacion

### Fase 1 — AI Suite Ollama (MVP) ✅ LISTO
Replicar `ai-suite-ollama.ahk` al 100%:
- `CapsLock + G` → Corregir seleccion. Pega resultado. Fallback: Ctrl+A → End+Shift+Home. Soporte `[contexto]`.
- `CapsLock + T` → Traducir seleccion. Ventana con resultado.
- `CapsLock + R` → Resumir seleccion. Ventana con resultado.
- `CapsLock + Enter` → Generar respuesta. Ventana con resultado.
- `CapsLock + V` → Historial de correcciones (ventana con lista, copiar original).
- `CapsLock + H` → Ayuda.
- Tooltips de estado ("Corrigiendo...", etc.).
- Ventana de resultado: original arriba, resultado abajo, boton Copiar, boton Cerrar, Escape, resizable.

### Fase 2 — Mejoras
- Persistir historial a disco (`history.json`). ✅ Ya implementado
- Notificaciones toast nativas de Windows en vez de tooltips simples.
- System tray icon (actualmente no hay; se planea con `tray-icon` crate).

### Fase 3 — Gemini Chrome
- `CapsLock + N` → Ventana nueva de Chrome con Gemini.
- `CapsLock + E` → Pestaña nueva de Chrome con Gemini.
- Enfocar ventana de Chrome (WinAPI).

### Fase 4 — App Launcher
- `CapsLock + Q` → Panel flotante con grid de iconos/apps.
- Click para lanzar. Escape para cerrar.

### Fase 5 — Notifications Overlay
- Bolitas circulares de Slack/Teams leyendo titulos de ventanas.
- Click para enfocar la app.

---

## Dependencias principales

| Crate | Proposito |
|-------|-----------|
| `tokio` + `reqwest` | Runtime async + HTTP a Ollama |
| `serde` + `serde_json` | JSON (API Ollama + historial) |
| `toml` | Configuracion externa |
| `arboard` | Portapapeles (cross-platform) |
| `enigo` | Simular `Ctrl+C`, `Ctrl+A`, `Ctrl+V` |
| `eframe` + `egui` | Ventanas de resultados |
| `thiserror` + `anyhow` | Errores tipados + ergonomia en app |
| `chrono` | Fechas para historial |
| `windows` | Low-level keyboard hook (WinAPI) |

---

## Estructura de archivos

```
quick-actions/
├── Cargo.toml
├── .cargo/
│   └── config.toml              # Config de cross-compile (MinGW)
├── config.toml                  # Settings editables sin recompilar
├── PLAN.md                      # Este archivo
├── src/
│   ├── main.rs                  # Orchestrador: init, channels, shutdown
│   ├── lib.rs                   # Modulos publicos
│   ├── config.rs                # Carga de config.toml (serde)
│   ├── error.rs                 # #[derive(thiserror::Error)]
│   ├── hotkeys/
│   │   ├── mod.rs               # Enum HotkeyCommand, start_listener()
│   │   └── win_hook.rs          # WinAPI WH_KEYBOARD_LL (thread dedicado)
│   ├── clipboard.rs             # backup/restore, get/set text
│   ├── key_sim.rs               # Wrapper sobre enigo (Ctrl+C, etc.)
│   ├── ollama.rs                # Cliente HTTP para Ollama API
│   ├── history.rs               # Vec<Entry> + persistencia JSON atomica
│   ├── prompts.rs               # System prompts (corregir, traducir, etc.)
│   └── ui.rs                    # Ventanas: resultado, historial, ayuda (egui)
```

---

## Decisiones de diseno

| Decision | Justificacion |
|----------|---------------|
| CapsLock hook nativo (WH_KEYBOARD_LL) | Exactamente como AHK. Windows-only. |
| `egui`/`eframe` para UI | Ventanas flotantes rapidas, multiplataforma, facil de iterar. |
| `config.toml` externo | Editar sin recompilar. Hotkeys hardcodeadas (igual que AHK). |
| Sin system tray (inicialmente) | Se agregara en Fase 2. |
| Build en `C:\code\` | Configurado en `.cargo/config.toml` con `target-dir`. |

---

## Build instructions

### Script automatico (recomendado)

```bash
./build.sh
```

Este script:
1. Compila en release para Windows x86_64.
2. Copia automaticamente `quick-actions.exe` y `config.toml` a `C:\quick-actions\`.

### Manual desde Linux (cross-compile a Windows)

```bash
# Requiere: rustup target add x86_64-pc-windows-gnu
# Requiere: mingw-w64 (x86_64-w64-mingw32-gcc)

cargo build --release --target x86_64-pc-windows-gnu

# Copiar a C:\quick-actions\ (en WSL):
cp target/x86_64-pc-windows-gnu/release/quick-actions.exe /mnt/c/quick-actions/
cp config.toml /mnt/c/quick-actions/
```

### Desde Windows

```bash
cargo build --release
```

Salida: `target/release/quick-actions.exe`

---

## Instrucciones de uso

1. Coloca `quick-actions.exe` y `config.toml` en la misma carpeta.
2. Asegurate de que Ollama este corriendo (`ollama serve`).
3. Ejecuta `quick-actions.exe`.
4. Selecciona texto en cualquier app y usa los atajos.

---

## Historial de commits / cambios

| Fecha | Cambio |
|-------|--------|
| 2026-07-27 | Esqueleto inicial. Fase 1 completa (AI Suite Ollama). |

---

## TODOs tecnicos pendientes

- [ ] Auto-arranque en Windows (gestion del usuario — acceso directo en shell:startup).
- [ ] System tray icon (Fase 2).
- [ ] Ventanas con iconos en el launcher (Fase 4).
- [ ] Extraer iconos de .exe en Windows (Fase 4).
- [ ] Bolitas de Slack/Teams (Fase 5).

---

## Referencia: scripts AHK originales

| Script | Estado migracion |
|--------|-----------------|
| `ai-suite-ollama.ahk` | ✅ Completo (Fase 1) |
| `gemini-chrome.ahk` | ⏳ Fase 3 |
| `app-launcher.ahk` | ⏳ Fase 4 |
| `notifications-overlay.ahk` | ⏳ Fase 5 |
| `ahk-suite.ahk` | ✅ Reemplazado por `main.rs` |
