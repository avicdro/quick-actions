# Plan v0.4 — UX & Ajustes (post-test Windows real)

> Feedback del primer test real (2026-10-24): toasts infinitos, modificador
> preferido Alt, ajustes sin GUI, pausa modular, visual mejorable.
> IMPROVEMENTS.md sigue siendo el maestro del refactor — este doc es de producto.

## Ítems

- [x] **U1 — Toasts transitorios** (auto-hide):
  - `ToastKind` en `ui_protocol.rs` (cross, con tests): `Loading` (`msg` termina en `...`, sin expiry), `Notice` (3.5s), `Error` (empieza con "Error", 6s)
  - `ui.rs`: `toast_expiry: Option<Instant>`; `SetStatus(msg)` y `SetPaused(true)` fijan expiry; `update()` chequea y cierra (el path `toast_was_visible` ya cierra el viewport)
  - Loading NO expira (spinner mientras corre la request Ollama)
  - Extra: icono por clase — spinner solo en `Loading`; `Notice` dibuja burbuja ámbar "i" (antes cualquier notice mostraba spinner falso)
- [x] **U2 — Modificador default = Alt**:
  - `HotkeysConfig::default()` y `default_toml` → `modifier = "alt"`; root `config.toml` → `alt`; config instalada en `C:\quick-actions` → `alt`
  - Log de arranque de main.rs → genérico (no "CapsLock+")
  - README: tabla de atajos `Alt+`, config example con `modifier = "alt"` (CapsLock queda documentado como opción)
  - P7 sigue íntegro: capslock+hold/toggle disponible por config
- [x] **U3 — Pausa modular por atajo**:
  - `HotkeysConfig`: bools `correct/translate/summarize/generate/history/help` (default true)
  - `HotkeyCommand::key_label() + config_field()` (cross, tests)
  - Dispatch: `actions::handle_command` lee flag del snapshot y corta con toast Notice "Atajo '{key}' desactivado (Ajustes)"
  - Tray submenu "Atajos": 6 checks + "Todos"/"Ninguno" (persisten via `write_config_key`; ids 1201–1208)
  - Se integra con la pausa total P2 (SUSPENDED manda antes)
- [x] **U4 — Pestaña Ajustes en la GUI**:
  - `Tab::Ajustes`: modelo (TextField + ComboBox de la cache Ollama), endpoint, timeout, modifier/capsmode combos (+label "requiere reinicio" si difiere del vigente), 6 checks de atajos, autostart, check_updates, repository, botón "Abrir config.toml"
  - Persistencia on-change via `write_config_key` + update del `RwLock<Config>` (efecto vivo)
  - `QuickActionsApp::new` recibe cache de modelos (`Arc<RwLock<Vec<String>>>`) y path del config
  - Extra: el click "Configuración" del tray ya NO abre el editor — manda `UiCommand::ShowAjustes` y abre la pestaña (el notepad queda tras "Abrir config.toml (avanzado)")
- [ ] **U5 — Refresh visual (egui)**:
  - Tema custom: navy oscuro (#0D1321/#121B2E) + accent ámbar #FFC93C (same family que el icono tray), rounding, selection, hover
  - Toast: pill con shadow + radius
  - Ayuda estructurada (badges de teclas en vez de ASCII art)
  - Espaciados en Resultado/Historial; textos vacíos con icono/tono
  - NOTA: el titlebar rojo es el accent de Windows del usuario, no del app

## Registro

| Fecha | Ítem | Commit | Notas |
|-------|------|--------|-------|
| 2026-10-24 | U2 | — | default `modifier="alt"` en config.rs/config.toml/README + config instalada; log de arranque genérico |
| 2026-10-24 | U1 | — | `ToastKind` (Notice 3.5s / Error 6s / Loading sin expiry) + icono por clase |
| 2026-10-24 | U3 | — | 6 bools en `[hotkeys]` + guard en dispatch + submenu tray (ids 1201–1208) + config.instalada parcheada |
| 2026-10-24 | U4 | — | Tab::Ajustes (Ollama/Hotkeys/App, persistencia on-change); tray "Configuración" repuntado a `ShowAjustes`; `write_config_key` publicada; tray~`request_models_refresh()` pub |
