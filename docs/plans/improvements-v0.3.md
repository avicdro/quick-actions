# Quick Actions — Plan v0.3 (Rápidos + Roadmap)

> Checklist maestro. Marcar con `[x]` al terminar cada ítem.
> Complementa `IMPROVEMENTS.md` (fases de refactorización — no se duplica aquí).

---

## 🔥 Rápidos R1–R6 (COMPLETADOS 2026-10-17)

- [x] **R1 — LICENSE** — archivo MIT creado ( AssemblyCopyright 2026 avicdro )
- [x] **R2 — Panic hook** — `main.rs` → `install_panic_hook()`: logs en `logs/panic.log` con timestamp + nombre de thread + backtrace, además de `tracing::error` al pipeline normal. Sin `panic = "abort"` en release, un panic en un thread interno (tray, hotkey, worker) no moría el proceso pero era invisible — ahora queda registrado
- [x] **R3 — CI** — `.github/workflows/ci.yml`:
  - job `test-linux`: `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test --lib` (20 tests)
  - job `build-windows`: cross-release `x86_64-pc-windows-gnu` (mingw-w64) + upload-artifact
- [x] **R4 — Releases** — `.github/workflows/release.yml`: on tag `v*` → build cross + publica `quick-actions.exe` + `config.toml` con release notes autogeneradas
- [x] **R5 — build.sh preflight** — ahora corre `cargo fmt --check` + `cargo test --lib` antes de compilar; flag `--no-check` para saltar
- [x] **R6 — exe fuera de git** — ya estaba: `*.exe` en `.gitignore` (verificado con `git check-ignore`)

### Bonus hecho en el camino (necesario para que R3 fuera posible)

- [x] **rustfmt aplicado a todo el repo** — el código no estaba formatted, `fmt --check` fallaba de entrada; `cargo fmt --all` (273+ ins/161 del, 14 files, mecánico sin lógica)
- [x] **`main.rs` estructurado con `#[cfg(windows)]`** — antes el bin no compilaba para Linux (usaba `eframe`/`windows` sin gate): ahora `windows_main()` con `#[cfg(windows)]` y stub `main()` para host; los tests de lib cross-platform ahora pueden correr en el CI

**Commitzación:** un commit con todos los rápidos (`feat: quick wins r1-r6`).

---

## 🗺️ Rápidos de producto (siguiente bloque)

- [x] **P1 — Auto-arranque con Windows** (COMPLETADO 2026-10-17) —
  - `autostart.rs` (nuevo, windows-only): lee/escribe/borra `HKCU\...\CurrentVersion\Run` valor `QuickActions` = exe actual entre comillas; sin admin
  - `config.toml` → `[app] autostart = false` (`AppConfig`)
  - Tray: ítem "Iniciar con Windows" con ✓ (MF_CHECKED según estado real del registro), toggle escribe registro Y config.toml
  - Escritura de config via `toml_edit` (preserva comentarios/formato) + test cross-platform de roundtrip
  - `sync_with_config()` al arranque: config `true` repara/recrea la entrada (exe movido); config `false` NO deshace toggles de tray (el registro manda a partir de ahí)
  - Cargo: +features `Win32_System_Registry`, `Win32_Security`; +dep `toml_edit`
  - Verificado: 23 tests + clippy + cross-check Windows OK

- [x] **P2 — Pausa de hotkeys** (COMPLETADO 2026-10-17) —
  - `Arc<AtomicBool>` compartido main → hook (win_hook) y tray
  - `keyboard_proc`: si suspended → passthrough total del teclado (ni Alt+combos se interceptan)
  - Tray: ítem *"Pausar atajos"* con ✓, toggle → `UiCommand::SetPaused(bool)`
  - UI: toast **"Pausado — atajos desactivados"** con icono ámbar de dos barras, persistente hasta reanudar
  - Session-only (no se persiste en config — semántica "modo reunión")
- [x] **P3 — Modelo Ollama configurable on the fly** (COMPLETADO 2026-10-17) —
  - `ollama.rs`: `list_models()` via `GET /api/tags` (+ `endpoint_base()` derivado del endpoint de generación, con tests)
  - Tray: submenu dinámico *"Modelo: {actual}"* con los modelos instalados (✓ en el vigente); cache `Arc<Mutex<Vec>>` llenada en background al arrancar + refetch tras cada cambio (guardia atomic anti-duplicado); nunca bloquea el click del menú
  - Si la cache está vacía: ítem "(buscando modelos...)" + refetch en background → reaparece el menú y ya están
  - Handler `ID_MODELO_BASE..+15`: `set_ollama_model()` persiste en `config.toml` (toml_edit) y hot-reload en `Arc<RwLock<Config>>` compartido — las próximas acciones ya usan el modelo nuevo
  - `config.rs`: helper común `write_config_key()` (reusado por `write_autostart` y `write_ollama_model`) + test roundtrip
  - Hot-reload: `config: Arc<RwLock<Config>>` para App y tray; `handle_command` lee snapshot al inicio de cada acción
  - Nota documentada: el comentario en-línea de la línea reemplazada se pierde, el resto del archivo se preserva
  - 26 tests + clippy + cross-check Windows OK
- [x] **P4 — Check de updates** (COMPLETADO 2026-10-18) —
  - `update_check.rs` (nuevo, cross-platform): `GET https://api.github.com/repos/{repo}/releases/latest` en thread de fondo al arrancar (timeout 5s, User-Agent requerido por la API); falla silenciosa (offline/rate-limit no molestan)
  - Comparación semver inline (`parse_version` + `is_newer_tag`): `v`-prefix, sufijos `-rc1`/`+build` ignorados, tags raras → no aviso. 6 tests
  - Config: `[app] check_updates = true` (opt-out) + `repository = "avicdro/quick-actions"`
  - UI: `UiCommand::ShowUpdate{version,url,body}` → reutiliza pestaña Resultado como visor: título azul "Nueva version disponible: vX", notas (`"Notas de la version:"`), botón *"Descargar desde GitHub ↗"* (`cmd /C start "" url` con `CREATE_NO_WINDOW`), aclaración "(no instala nada automaticamente)". Un `ShowResult` normal cancela el aviso (`update_url = None`)
  - BONUS: protocolo UI extraído a `ui_protocol.rs` **cross-platform** (`UiCommand`, `ResultData`, `HistoryData`, `HistoryEntryView`, `UiChannels`) — `ui.rs` (windows-only) lo re-exporta; `update_check` puede importar sin egui. Prepara la fase 1 de refactor
  - 29 tests + clippy + cross-check OK
- [ ] **P5 — Instalador/desinstalador** — script PowerShell `install.ps1`: copia a `%LOCALAPPDATA%\QuickActions`, config → `%APPDATA%\quick-actions\`, symlink en autostart; `uninstall.ps1` inverso
- [ ] **P6 — Íconos tray/UI reales** — `.ico` embebido (`include_bytes!`) para tray y viewport
- [ ] **P7 — CapsLock como toggle configurable** — `[hotkeys] capsmode = "hold" | "toggle"` (default `hold`, comportamiento actual); en toggle, CapsLock trabaja como rapñador y suelto dispara

---

## 📋 Refactor grande (IMPROVEMENTS.md solo referencia)

Ver detalle completo en `IMPROVEMENTS.md` (fases 1–5). Resumen del orden acordado:

- [ ] **Fase 1.1** — dividir `ui.rs` (587 líneas) en `ui/{window_manager,tabs/*}.rs`
- [ ] **Fase 1.2** — `ActionHandler` trait: desacoplar `actions.rs` de `ui.rs`
- [ ] **Fase 1.3** — eliminar `OnceLock` globales: `AppState` inyectado (tray, win_hook, ui)
- [ ] **Fase 1.4** — `platform/` traits: Clipboard/KeySim/HotkeyListener con implementaciones Windows + Mock
- [ ] **Fase 2.1** — worker pool acotado con `tokio::sync::Semaphore` (3 concurrentes)
- [ ] **Fase 2.2** — `CancellationToken` para requests Ollama y shutdown limpio
- [ ] **Fase 2.3** — `release_modifiers` async con timeout (no busy-wait)
- [ ] **Fase 2.4** — hotkeys event-driven con `tokio::sync::mpsc`
- [ ] **Fase 3.1** — error chains con `#[source]` (thiserror)
- [ ] **Fase 3.2** — logging estructurado JSON en release
- [ ] **Fase 3.3** — retry con backoff para Ollama
- [ ] **Fase 4** — tests integración (mockito), property tests, mocks por platform traits

---

## Registro

| Fecha | Ítem | Commit | Nota |
|-------|------|--------|------|
| 2026-10-17 | plan creado | — | v0.3 |
| 2026-10-17 | R1–R6 completados | `R1-R6: quick wins + rustfmt + cfg(windows) main` | fmt+clippy+20 tests OK |
| 2026-10-17 | rustls 0.23.46 (vuln fix) | ea0c889 | Dependabot PR #1 cerrado |
| 2026-10-17 | P1 completado | `feat: P1 auto-arranque...` (24d77a0) | 23 tests + cross-check OK |
| 2026-10-17 | P2 completado | `feat: P2 pausa de hotkeys...` (54fbede) | passthrough total + toast Pausado |
| 2026-10-17 | P3 completado | `feat: P3 modelo on-the-fly...` | submenu tray + hot-reload (26 tests) |
| 2026-10-18 | P4 completado | `feat: P4 check updates...` | GitHub API + ui_protocol extracción (29 tests) |
