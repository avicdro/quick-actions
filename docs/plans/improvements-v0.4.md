# Plan v0.4 — UX & Ajustes (post-test Windows real)

> Feedback del primer test real (2026-10-24): toasts infinitos, modificador
> preferido Alt, ajustes sin GUI, pausa modular, visual mejorable.
> IMPROVEMENTS.md sigue siendo el maestro del refactor — este doc es de producto.

## Ítems

### Ronda 2 (feedback post-test 2)

- [x] **V1 — GUI a11y + estética** (contraste, tipografías, separación):
  - `text_styles`: body 15, button 14.5, heading 19, small 12, monospace 13
  - spacing: item_spacing (10,8), button_padding (14,7), iconos más grandes
  - Tabs como pills más grandes (15px strong)
  - Cards: secciones de Ajustes en Frames con card bg + rounded 10 + título ámbar 16.5; Grid de 2 columnas para label↔campo
  - Checkmark de checkboxes en ámbar (estados activo/hover)
- [x] **V2 — Ventana real de Windows (Alt-Tab)**:
  - Quitar `with_taskbar(false)` del viewport principal (el toast lo conserva)
  - Al estar visible participa en taskbar + Alt-Tab; al ocultarse desaparece (ventana invisible)
  - Extra: min inner size 480x420 (antes 400x300)

### Ronda 3 (feedback post-V1/V2)

- [x] **V3 — Contraste tab activa + scroll completo**:
  - Tab activa: texto oscuro sobre el chip ámbar (ahora ámbar-sobre-ámbar es invisible)
  - ScrollArea de Ajustes: reservar la altura de la fila de botones (`max_height = available - buttons_h 46px`) para que el scroll llegue hasta abajo y los botones nunca se corten
- [x] **V4 — Semantica tray-app al cerrar**:
  - ~~X / Alt-F4 → `Visible(false)`~~ **eigentlicher bug**: egui no reabre una viewport oculta (emilk/egui#5229/#3655) — implementado con Win32: la ventana queda offscreen 1x1 "visible" para winit (frames vivos) y se le pone/quita `WS_EX_TOOLWINDOW` (`set_toolwindow`) — sin taskbar ni Alt-Tab al cerrar; al abrir, estilo normal (+ FindWindowW+pid check para el HWND)
  - Al abrirse (tray/atajos) ya se comporta como ventana normal (V2)
  - OJO a testear: los toasts usan un viewport independiente; si al estar el principal oculto deja de verse un toast, reportar (fallback: window separada con WS_EX_TOOLWINDOW)
- [x] **V5 — Arranque solo-tray (sin boton en taskbar)**:
  - eframe hace `set_visible(true)` tras el PRIMER frame ignorando `with_visible(false)` (anti-flash blanco, egui PR2279) → al arrancar (autostart) quedaba boton en la barra sin ventana visible
  - Fix: `set_toolwindow(true)` en el primer frame — la ventana nace sin boton de taskbar hasta que se abre de verdad
  - `main_hwnd()` ahora con EnumWindows fallback (al arrancar el titulo es "QuickActions" del app_name; el viewport del toast se excluye por titulo vacio)

### Ronda 1

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
- [x] **U5 — Refresh visual (egui)**:
  - Tema custom: navy oscuro (#0D1321/#121B2E) + accent ámbar #FFC93C (same family que el icono tray), rounding, selection, hover
  - Toast: pill con shadow + radius
  - Ayuda estructurada (badges de teclas en vez de ASCII art)
  - Espaciados en Resultado/Historial; textos vacíos con icono/tono
  - NOTA: el titlebar rojo es el accent de Windows del usuario, no del app
  - Implementado: `theme` module (paleta nominal), `apply_theme` una vez (`style_applied`), toast `BG_ELEV`+borde ámbar pulsante+rounding 10, `key_badge` chip, help reescrito con filas `{mod}+{key}` antes de nota toggles, historial vacío tintado dim, window_shadow bonito. `ShowHelp` queda por compatibilidad (el payload ya no se muestra)

### Ronda 4 (feedback post-V5, QA automatizado)

- [x] **V6 — Boot/open/close QA-driven (root causes reales)**:
  - **Causa raiz del boton en taskbar**: con `with_visible(false)` eframe NO lanza ni un frame de `update()` hasta que algo externo pide repaint (el clic del tray lo hace, por eso parecia intermitente) → el cap V5 del primer frame nunca corria; la ventana nacia con `WS_EX_APPWINDOW` de winit
  - Fix de raiz: en la closure de `run_native`, `set_ui_ctx(cc.egui_ctx)` + `ViewportCommand::Title("Quick Actions")` + `request_repaint()` (primer frame desde t=0; hook/tray pueden despertar el UI siempre)
  - `set_toolwindow(bool) -> bool` con regla canonica: tool = `TOOLWINDOW↑ APPWINDOW↓` (con APPWINDOW, la taskbar mantiene el boton aunque haya TOOLWINDOW), `ShowWindow(SW_HIDE)` → estilo → `SW_SHOWNOACTIVATE` (cambiar ex-style con ventana visible no refresca la taskbar), y **retry por frame** (`boot_tool_done`) hasta que pega
  - `present_main_window()` al abrir: `IsIconic?SW_RESTORE` + poke `TOPMOST→NOTOPMOST` + `BringWindowToTop` + `SetForegroundWindow` best-effort — soluciona "abre detras de un juego fullscreen" (foreground lock deniega activar desde procesos de fondo; el poke de z-order la sube sin activar)
  - QA: `scripts/qa_smoke.ps1` (fases boot/show/tray/close: lanza, inyecta WM_COMMAND/WM_CLOSE reales, dumpea hwnd/estilos/rects, screenshot) y `scripts/qa_live.ps1` (sin kill, para log async) — corre desde WSL via powershell.exe; flag `--show` para abrir la GUI al arrancar sin interaccion

## Ronda 5 (v0.2.4 — base y operación) — 2026-10-25

- [x] **S1 — Instancia única** (mutex Windows + focus): doble clic del exe
  ya no duplica tray/hook/update-check. `single_instance.rs`:
  `CreateMutexW("Local\QuickActions::single-instance")` (nombre fijo, handle
  leak por vida del proceso, fail-open si CreateMutex falla).
  - 2ª instancia: si la GUI de la viva está abierta de verdad (visible y sin
    TOOLWINDOW) → FocusWindowSecuencia de V6 (restore + topmost-poke +
    SetForegroundWindow) sobre el HWND de OTRO proceso (FindWindowW por
    título, sin filtro pid) y `exit(0)`; si está oculta en tray → exit(0)
    silencioso (mostrarla desde fuera desincronizaría el libro egui).
  - Raza del auto-update (U7: spawn nuevo → exit viejo ~300ms): reintentos de
    mutex si existe `update-applied.tmp` (5s × 150ms); timeout → se borra el
    marker para no envenenar futuros boot.
  - QA: fase `dup` en `scripts/qa_smoke.ps1` (B debe morir en ≤2s con 1 vivo).
- [x] **N1 — NVIDIA: diagnóstico real + spam del log**:
  - Hallazgo (log 2026-10-25): wgpu pinta en la **RTX 3060 Ti (Vulkan,
    DiscreteGpu)** — el equipo NO tiene iGPU usable (Win32_VideoController =
    NVIDIA + DisplayLink USB), así que `LowPower` no tenía dónde caerse. El
    "confirmado en nvidia-smi" de L9 era un espejismo: nvidia-smi en Windows
    no lista procesos Vulkan/gráficos.
  - Hallazgo 2: **~146k líneas/día** de `wgpu_core::device::resource:
    Device::maintain: waiting for submission` — el 30ms de
    `window_open` mantenía un render-loop continuo (~vsync) con la GUI
    abierta.
  - Fix: filtro de tracing con `EnvFilter` (`info` + target
    `wgpu_core::device::resource=warn`; `RUST_LOG` manda si existe) y repaint
    reactivo: 30ms SOLO con toast animado; sin toast, keepalive 5000ms — GUI
    abierta + idle = 0 frames (blink/hover piden el suyo).
  - Toast overlay NVIDIA (si persiste): no es remediado por código — ver
    README "Solución de problemas" (purge NvBackend + lista de juegos).
- [x] **B1 — Build id + versionado**:
  - `build.rs`: `QA_BUILD_ID` = commit corto (GITHUB_SHA en CI / git local),
    definido para TODOS los targets (lib.rs lo usa con env!).
  - `version_string()` en lib.rs: `v0.2.4 (abcdef1)` — línea de arranque del
    log + fila "Versión:" en Ajustes→App.
  - Política (README "Versionado"): patch ilimitado post-0.2, minor para
    hitos, 1.0.0 explícito-estable. Esta es **v0.2.4`.

- [x] **T1 — Tema claro + selector Oscuro/Claro/Sistema**:
  - `theme` (config `[ui]`, default "dark"; parse tolerante EN/ES, raro → Dark,
    + tests: `ThemeChoice` en config.rs) y card **Apariencia** en Ajustes
    (combo 3 opciones; persiste toml_edit + replica en el RwLock — el
    re-apply reactivo de `update()` pinta al frame siguiente, sin reinicio).
  - `theme` consts → `Palette` struct con DARK/LIGHT (`LIGHT`: papel cálido
    + tinta navy; ámbar #FFC93C → ámbar oscuro #9E6708 como texto/stroke
    porque el amarillo puro era ilegible sobre claro; acento de badges con
    fondo ámbar se conserva con tinta). `palette()` global (atomico) —
    toast viewport y cualquier dibujo consultan coherente.
  - `apply_theme(ctx, palette)`: baseline egui `set_theme(Dark/Light)` +
    overrides paramétricos (selection_bg/active_bg/alphas de sombra entran
    a la paleta); re-apply SOLO al cambiar de paleta.
  - QA visual (PrintWindow + nvidia-smi-free): Resultado y Ayuda en dark
    (sin regresión) y en light legibles ✓; log "T1 tema aplicado" por cambio.
  - Fix colateral: `default_toml` tenía la sección como comentario
    (`#[hotkeys]` → `[hotkeys]`) — configs recién generadas la llevan real.

## Registro

| Fecha | Ítem | Commit | Notas |
|-------|------|--------|-------|
| 2026-10-24 | U2 | — | default `modifier="alt"` en config.rs/config.toml/README + config instalada; log de arranque genérico |
| 2026-10-24 | U1 | — | `ToastKind` (Notice 3.5s / Error 6s / Loading sin expiry) + icono por clase |
| 2026-10-24 | U3 | — | 6 bools en `[hotkeys]` + guard en dispatch + submenu tray (ids 1201–1208) + config.instalada parcheada |
| 2026-10-24 | U4 | — | Tab::Ajustes (Ollama/Hotkeys/App, persistencia on-change); tray "Configuración" repuntado a `ShowAjustes`; `write_config_key` publicada; tray~`request_models_refresh()` pub |
| 2026-10-24 | U5 | — | tema navy+ámbar (`theme`/`apply_theme`), toast pill, help con `key_badge`s, dims vacíos; `help_text` field eliminado (help estructurado) |
| 2026-10-25 | V2 | — | `with_taskbar(false)` fuera del viewport principal → taskbar + Alt-Tab mientras visible; min size 480x420 |
| 2026-10-25 | V1 | — | text_styles 15/14.5/18.5, spacing (10,8)+button_padding(14,7)+icons 22, `TEXT_DIM` aclarado (#BAC3D2), tabs pill strong+accent, cards de Ajustes (`settings_card` + Grid 2 col), checkmarks fg ámbar en hover/activo |
| 2026-10-25 | V3 | — | tab activa texto oscuro sobre chip ámbar; ScrollArea con `max_height(available-46)` reserva fila de botones |
| 2026-10-25 | V4 | — | X/Alt-F4 → `Visible(false)` real (desaparece de taskbar/Alt-Tab; queda solo el tray); mostrar vuelve a Visible(true)+focus |
| 2026-10-25 | V4fix | — | egui#5229 impedía reabrir: en vez de Visible(false), WS_EX_TOOLWINDOW via `set_toolwindow()` (FindWindowW+pid check); reopen OK con tab Ayuda/Ajustes |
| 2026-10-25 | V5 | — | arranque solo-tray: `set_toolwindow(true)` en primer frame (eframe fuerza visible tras el 1er frame, PR2279); `main_hwnd()` con EnumWindows fallback pre-titulo |
| 2026-10-25 | V6 | — | QA destapa 3 raíces: eframe no pinta hasta 1er repaint (fix: Title+repaint en closure cc); APPWINDOW vs TOOLWINDOW (tool=+TOOLWINDOW −APPWINDOW con hide/show); fullscreen-cover (present_main_window topmost-poke). `--show` + scripts QA |
| 2026-10-25 | L8 | — | pulido de diseño (skill impeccable/Operate): selección de texto ámbar-oscuro + texto claro (egui 0.29: texto del item usa selection.stroke.color); cards full-width uniformes + radios 12 + aire arriba 12px; Atajos en grid alineado (Modificador:/CapsLock:); título de Resultado ámbar (era navy sobre navy); popup_shadow |
| 2026-10-25 | L9 | — | feedback visual: tab activa = SOLO borde ámbar sin relleno (scope local de selection en tabs) + texto ámbar; TextEdits con margin interior (8,6 y 10,8 en multilineas); renderer glow→wgpu Backends::PRIMARY + LowPower (iGPU) — nvidia-smi confirma que la app ya no despierta la dGPU/NVIDIA (overlay/flash negro fuera); QA: captura PrintWindow (ve la ventana aunque haya fullscreen encima) |
| 2026-10-25 | U6 | — | updates sin polling (ya era 1 llamada al arranque; aclarado en doc): botón "Buscar actualizaciones" en Ajustes→App (`run_manual_check` con feedback SIEMPRE por toast: ShowUpdate / "estás en la última" / error); checkbox de arranque intacto |
| 2026-10-25 | U7 | — | auto-update profesional de 1 click en el aviso (`self_update.rs`): descarga exe + SHA256SUMS.txt (release.yml lo publica ahora), verifica hash, swap por rename (exe vivo: rename OK / overwrite bloqueado), spawn nuevo + exit, arranque limpia `.prev.exe` + toast "Actualizado a vX ✓"; rollback si el swap falla |
| 2026-10-25 | S1 | — | `single_instance.rs` (mutex `Local\` + focus cross-proceso + retry marker U7); fase `dup` en qa_smoke |
| 2026-10-25 | N1 | — | log real del adapter (EnvFilter, RUST_LOG) + spam `Device::maintain` silenciado por target + repaint reactivo (30ms solo con toast); hallazgo: sin iGPU usable, dGPU era inevitable y nvidia-smi (Windows) no lista Vulkan |
| 2026-10-25 | B1 | — | `build.rs` QA_BUILD_ID (GITHUB_SHA/git), `version_string()`, fila "Versión" en Ajustes, arranque del log con hash; bump 0.2.4 |
| 2026-10-25 | T1 | — | paletas DARK/LIGHT (`theme::Palette` + atomic de activa), config `[ui] theme` (dark/light/system, parse EN/ES), card Apariencia en Ajustes con re-apply reactivo sin reinicio; QA visual dual ✓; fix `#[hotkeys]` comentado en default_toml; bump 0.2.5 |
| 2026-10-25 | I1 | — | icono nuevo (rayo sobre square dark): `assets/icono.ico` (staging del usuario) → `app.ico` multi-tamaño 16/20/24/32/48/64/128 regenerado con Pillow desde el 128px fuente; embed `1 ICON` intacto (build.rs/tray sin cambios); verificación end-to-end ExtractAssociatedIcon; bump 0.2.6 |
| 2026-10-25 | I1b+I2 | — | fill del ico 81%→97% (bbox-crop del arte + re-centrado, LANCZOS — el margen muerto lo hacía verse enano en el tray a 16px); icono de ventana/taskbar/Alt-Tab desde el recurso `1 ICON` (GetIconInfo+GetDIBits 32bpp top-down, BGRA→RGBA, `with_icon`) — eframe pintaba su default 'e' navy; QA visual: botón taskbar = rayo ✓; bump 0.2.7 |
| 2026-10-25 | I3 | — | fuente nativa del arte: `icono__4.png` (101×102, fill 100%, staging del usuario) → app.ico regenerado desde nativo (cuadrado 102 centrado → 124/128, LANCZOS, 7 tamaños) — más nítido que el upscale 104→124 de I1b; "icono (4).ico" descartado tras demostrar byte/pixel-identidad con el original (mismo padding); staging borrado; bump 0.2.8 |
