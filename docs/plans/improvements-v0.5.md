# Plan v0.5 — Correcciones de auditoría y calidad (2026-11-02)

> Documento de planing de la ronda de auditoría completa del proyecto
> (código, config, CI, seguridad, concurrencia). `IMPROVEMENTS.md` sigue
> siendo el maestro del refactor estructural (Fases 1–5); ESTE doc cubre
> los hallazgos NUEVOS que no estaban en ningún doc previo, más el quick
> wins accesorio. Marca con `[x]` lo completado.
>
> Estado de partida: 42 tests `--lib` en verde, clippy lib limpio, CI ok,
> v0.2.8 post-limpieza de iconos (I1–I3).

---

## Resumen de prioridades

| Bloque | Enfoque | Estado | Prioridad |
|--------|---------|--------|-----------|
| C | Bugs y problemas concretos (nuevos) | ⬜ Pendiente | 🔴 Alta |
| QW | Quick wins accesorios | ⬜ Pendiente | 🟢 Baja |
| F2* | Concurrencia (ya en IMPROVEMENTS Fase 2) | ⬜ Pendiente | 🟡 Media |
| F1* | Mantenibilidad (ya en IMPROVEMENTS Fase 1) | ⬜ Pendiente | 🟡 Media |

(*) Los bloques marcados con * NO se duplican aquí: se trackean en
`IMPROVEMENTS.md` (Fases 1–5 y 5.x). Este doc solo lista lo que no estaba
registrado y añade evidencia file:line nueva.

---

## Bloque C — Bugs y problemas concretos (nuevos, no están en tus docs)

### C1. Config muerta que miente al usuario

`font_size`, `window_width`, `window_height` (`src/config.rs:85-89`) se
documentan en `README.md` y `config.toml`, pero **nunca se leen en el
código**:

- La UI hardcodea `ctx.set_pixels_per_point(1.3)` (`ui.rs:963`).
- La ventana se abre a `900×700` fijo (`ui.rs:1014`/`1018`).
- Los text styles son fijos (`ui.rs:1731-1737`): 15.5/14.5/18.5/12/13.

El usuario edita valores que no hacen nada.

**Decisión de diseño** (un solo knob, coherente):
- `window_width` / `window_height` → **se cablean**: tamaño de la ventana
  al abrir (clamp 480×420..=4000×4000, respetando `min_inner_size`).
- `font_size` → **se elimina** (era una abstracción muerta): escalar los
  text styles a mano deja fuera los `RichText::size()` hardcoded (tabs,
  toast, cards) y produce UI inconsistente. El zoom uniforme correcto en
  egui es `pixels_per_point`, así que entra `scale` (default `1.3` = el
  look actual) que escala TODA la GUI (fuentes + widgets) por igual.
- `scale` también arregla el DPI (ver C2): es configurable y ya no es un
  magic number ignorante del monitor.

**Checklist**
- [ ] `config.rs`: `UiConfig` sin `font_size`, con `scale: f32` (default 1.3)
- [ ] `config.rs`: `default_toml` actualizado
- [ ] `config.toml` (repo, template que distribuye el build) actualizado
- [ ] `ui.rs`: ppp desde config (fallback 1.3 si el valor es basura)
- [ ] `ui.rs`: tamaño de ventana al abrir desde config (clamp)
- [ ] Tests de config actualizados (`config_default_values`,
      `config_parse_full_toml`, `config_parse_partial_toml_uses_defaults`)
- [ ] `README.md`: bloque de configuración actualizado

---

### C2. DPI hardcodeado que ignora Windows

`ui.rs:963` fuerza `set_pixels_per_point(1.3)` en cada frame: en monitores
a 125%/150% el egui queda borroso o desproporcionado, y en 1080p@100% la
UI es más grande de lo nativo sin remedio para el usuario.

**Fix**: `scale` pasa a config (`[ui] scale`, default 1.3 = look actual).
Quien quiera nativo pone `1.0`; en monitores HiDPI se ajusta a gustor.
(Resuelto dentro de C1.)

---

### C3. `OllamaClient` recreado por cada hotkey → reqwest sin pooling

`actions.rs:73` hace `OllamaClient::new(config.ollama.clone())` por cada
acción, y ese constructor (más `list_models` en `ollama.rs:111`,
`fetch_latest` en `update_check.rs:128` y `download` en `self_update.rs:162`)
crean un `reqwest::Client` NUEVO cada vez: se pierde el connection-pool
(keep-alive, TLS session cache). La nota 5.5 de IMPROVEMENTS ("reusa
reqwest::Client ✓ ya ok") es engañosa: el reuso solo existe *dentro* del
struct, que nace y muere con cada acción.

**Fix**: un solo `reqwest::Client` compartido, creado una vez en `main` y
colgado en `AppState::http`; timeouts POR-REQUEST (`RequestBuilder::timeout`)
porque cada llamada usa uno distinto (generate: config, tags: 3s, GitHub:
5s, updater: 120s).

**Checklist**
- [ ] `app_state.rs`: campo `pub http: Arc<reqwest::Client>` + param en `new()`
- [ ] `main.rs`: crear el cliente compartido (fallback `Client::new()`)
- [ ] `ollama.rs`: `OllamaClient::new(config, client)`; `generate()` con
      `.timeout()` por request; `list_models(config, &client)` ya no crea
      cliente propio
- [ ] `actions.rs`: construir `OllamaClient` con `state.http`
- [ ] `update_check.rs`: `spawn_check`/`run_manual_check`/`fetch_latest`
      reciben el cliente compartido
- [ ] `self_update.rs`: `apply_update`/`download` reciben el cliente
- [ ] `ui.rs`: llamadores de update-check/apply_update pasan `state.http`
- [ ] Cross-build windows ok + tests lib ok

---

### C4. `std::process::exit(0)` en la salida del tray — shutdown abrupto

`tray.rs:411` y `tray.rs:664` (dos sitios: WM_COMMAND y TPM_RETURNCMD)
usan `process::exit(0)`: corta destructores, el flush del logger
(WorkerGuard), y deja workers/hook sin cancelar. Es incoherente con el
propio cuidado de `single_instance.rs` (que sale por `return` explícitamente
para que el guard flusha).

**Fix**: camino de salida limpio:
- `UiCommand::Shutdown` nuevo en el protocolo.
- Tray "Salir": `PostQuitMessage(0)` (termina el pump del tray y ejecuta
  `NIM_DELETE`) + `UiCommand::Shutdown` al UI. Ya NO `process::exit`.
- `ui.rs`: flag `shutting_down`; con Shutdown se deja de interceptar el
  close (`CancelClose` solo si no se está saliendo) y se pide
  `ViewportCommand::Close` → `run_native` vuelve → `windows_main` cae y el
  `WorkerGuard` flusha el log en el drop.
- Los threads restantes (hook de teclado, tokio) terminan al morir el
  proceso — mismo comportamiento de hoy, pero con log completo.

**Checklist**
- [ ] `ui_protocol.rs`: variante `Shutdown`
- [ ] `ui.rs`: `shutting_down` + close no-interceptado en shutdown
- [ ] `tray.rs`: 2 sitios de EXIT sin `process::exit`
- [ ] `main.rs`: log final tras `run_native` (visibilidad del flush)
- [ ] QA manual en Windows: tray → Salir deja log completo y proceso fuera

---

### C5. Auto-update fail-open sin SHA256SUMS — riesgo de supply chain

`self_update.rs:106-111`: si `SHA256SUMS.txt` no está en la release (las
viejas no lo tenían), el updater SIGUE: descarga el exe y lo ejecuta
**sin verificar nada**. Para un auto-update es un fallo de seguridad.

**Fix (fail-closed)**: sin sums disponibles → abortar, no aplicar nada,
toast de error sugiriendo descarga manual desde Releases. Trade-off
aceptado: releases `<= v0.2.3` no pueden auto-actualizar (deben instalar
manual; era un flujo que igualmente era inseguro).

**Checklist**
- [ ] `self_update.rs`: rama de sums faltante pasa de warn+continuar a
      error+abortar (con limpieza de temporales)
- [ ] Doc del módulo actualizado (fail-closed)

---

### C6. Fragilidad por string-matching (estado del toast y hotkeys)

Doble acoplamiento texto↔comportamiento:

1. `classify_status()` (`ui_protocol.rs:54-62`) deduce la clase del toast
   de convenciones de texto (`starts_with("Error")`, `ends_with("...")`).
   Cambiar un mensaje o un idioma rompe spinner/expiry silenciosamente.
2. `hotkey_switch`/`is_hotkey_enabled` (`hotkeys/mod.rs:62-84`) hcen
   `match` sobre `cmd.config_field()` (strings) en vez de match directo
   del enum.

**Fix**:
- `UiCommand::SetStatus(String)` → `SetStatus(StatusMsg)`:
  ```rust
  pub enum StatusMsg { Clear, Loading(String), Notice(String), Error(String) }
  ```
  La clase ya no se adivina: quien envía la declara. `classify_status`
  desaparece; `ToastKind` queda para el dibujo (icono/expiry).
- `hotkey_switch`/`is_hotkey_enabled`: `match cmd { Correct => &mut cfg.correct, ... }`
  (a prueba de refactor del texto). `config_field()` queda SOLO para las
  claves TOML de persistencia.

**Checklist**
- [ ] `ui_protocol.rs`: `StatusMsg` + variante tipada; fuera `classify_status`
- [ ] `ui.rs`: match de `SetStatus` por variante (Clear/Loading/Notice/Error)
- [ ] Senders migrados: `actions.rs`, `tray.rs`, `self_update.rs`,
      `update_check.rs`, `main.rs` (marker update), `ui.rs` interno
- [ ] `hotkeys/mod.rs`: switch/enabled por enum (con test)
- [ ] Tests del protocolo actualizados (fuera los de `classify_status`)

---

### C7. `main_hwnd()` depende del título de la ventana

`ui.rs:1493` y `single_instance.rs:126` buscan el HWND por título
(`FindWindowW("Quick Actions")` + EnumWindows fallback). Frágil: una
colisión de título o el rename en caliente (`Title` command) abre carreras
que hoy funcionan por los reintentos de QA, pero es innecesario.

**Fix** (barato): cachear el HWND del propio proceso la primera vez que
se encuentra (`AppState::main_hwnd: OnceLock<isize>`); las llamadas
posteriores (`set_toolwindow`, `present_main_window`) usan la cache y solo
cae al FindWindow si está vacía. El de `single_instance` (otro proceso)
mantiene `FindWindowW` (la cache no aplica cross-proceso; documentado).

**Checklist**
- [ ] `app_state.rs`: campo `main_hwnd` (OnceLock, leak-safe)
- [ ] `ui.rs`: `main_hwnd(state)` con fast-path de cache; firmas
      `set_toolwindow`/`present_main_window` reciben el estado
- [ ] `single_instance.rs`: comentario (no aplica cache cross-proceso)
- [ ] Cross-build windows ok

---

## Bloque QW — Quick wins accesorios (de la auditoría, fuera de C)

| # | Mejora | Archivo | Estado | Esfuerzo |
|---|--------|---------|--------|----------|
| QW1 | Eliminar `anyhow` (dep muerta; nadie la usa en `src/`) | `Cargo.toml` | [ ] | 🟢 trivial |
| QW2 | `tracing::info!("UI command recibido")` por cada comando → `debug!` | `ui.rs:123` | [ ] | 🟢 trivial |
| QW3 | Deducir `HistoryEntryView` en un solo sitio (truncado ×3 duplicado) | `actions.rs`+`ui.rs` | [ ] | 🟢 bajo |
| QW4 | Cerrar ventana: `TextEdit::as_str()` read-only review (id egui) — doc | `ui.rs:249/300/314` | [ ] | 🟢 docs |

*(QW1 lo ejecuta la ronda C de arranque; el resto queda pendiente para
después del Bloque C y antes de retomar Fase 2 de IMPROVEMENTS.)*

---

## Bloques ya trackeados en IMPROVEMENTS.md (solo evidencia nueva)

- **F2.1 ThreadPool**: `std::thread::spawn` por hotkey en `ui.rs:222` sin
  límite; cada worker además hace `block_on` + sleeps bloqueantes.
- **F2.3 release_modifiers**: busy-wait 40×50ms con `GetAsyncKeyState`
  (`key_sim.rs:38-46`) en el worker thread.
- **F3.1 error chains**: `AppError::Clipboard/Ollama/Keyboard(String)`
  pierden el `#[source]` (error.rs).
- **F3.2 / log**: ruido — `tracing::info!("UI command recibido")` por
  comando (ui.rs:123).
- **5.6 clipboard**: `get_text` detecta "vacío" por `contains("empty")`
  sobre el string del error de arboard (clipboard.rs:69-73) — frágil.
- **5.1 history**: `hist.save()` síncrono en cada corrección
  (actions.rs:140).

---

## Orden de ejecución (Bloque C)

```
C-anyhow (QW1) → C1+C2 (config real) → C3 (client compartido)
→ C4 (shutdown limpio) → C5 (fail-closed) → C6 (tipado)
→ C7 (HWND cache) → QW2/QW3
```

Principios del repo (IMPROVEMENTS.md): un cambio a la vez, compile+test
antes de seguir, `cargo fmt --all --check` + `cargo test --lib` +
`cargo build --release --target x86_64-pc-windows-gnu` como verificación
mínima en cada commit (los módulos windows-only no compilan en host).

---

## Registro de Cambios

| Fecha | Ítem | Commit | Notas |
|-------|------|--------|-------|
| 2026-11-02 | — | — | Doc creado; auditoría completa (base v0.2.8) |