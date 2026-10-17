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

- [ ] **P1 — Auto-arranque con Windows** — toggle en tray + config (`[app] autostart = false`), escribe/elimina `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- [ ] **P2 — Pausa de hotkeys** — checkbox en tray "Suspended". `HotkeyCommand`/listener consulta flag `Arc<AtomicBool>`; toasts ignorados y hook hace passthrough de CapsLock
- [ ] **P3 — Modelo Ollama configurable on the fly** — selector en UI ayuda/tray guarda en config.toml `(edit atomic config.toml = escribdor atomic)`; recarga config en caliente
- [ ] **P4 — Autoupdate check** — al iniciar, GET `https://api.github.com/repos/{owner}/{repo}/releases/latest`, si tag > `CARGO_PKG_VERSION` → toast + click abre release (sin installar, solo avisar)
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
