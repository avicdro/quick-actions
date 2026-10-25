//! `AppState` (fase 1.3 del refactor) — el unico estado compartido de la
//! app, empaquetado e inyectado como `Arc<AppState>` a todos los threads
//! (UI, tray, hook de teclado, workers de acciones).
//!
//! Reemplaza los statics `OnceLock` dispersos (contexto egui de `ui.rs`,
//! `SENDER`/`SUSPENDED` del hook, `TRAY_DATA` del tray, `MODIFIER`/`CAPS_MODE`
//! de hotkeys): nada de globals de negocio; todo lo compartido vive aqui y
//! se crea en `main` una sola vez.
//!
//! Windows-only porque agrega `ClipboardManager` y la UI de egui; el resto
//! de tipos (Config, History, OllamaClient, UiCommand) son cross.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use crate::clipboard::ClipboardManager;
use crate::config::Config;
use crate::history::History;
use crate::ui_protocol::{RepaintFn, UiCommand};

/// Estado compartido de la aplicacion (ver doc del modulo).
pub struct AppState {
    /// Config con hot-reload (el tray y Ajustes la reescriben en vivo).
    pub config: Arc<RwLock<Config>>,
    /// Ruta absoluta del config.toml (persistencia toml_edit).
    pub config_path: PathBuf,
    /// P2: pausa global de atajos (toggle del tray).
    pub suspendido: Arc<AtomicBool>,
    /// Clipboard manager compartido.
    pub clipboard: Arc<ClipboardManager>,
    /// Historia compartida.
    pub history: Arc<Mutex<History>>,
    /// Cache de modelos Ollama para el submenu del tray y Ajustes (P3).
    pub ollama_models: Arc<RwLock<Vec<String>>>,
    /// Guardia anti-refetch-duplicado (`true` = refetch en curso).
    pub models_fetching: Arc<AtomicBool>,
    /// Canal hacia el UI (toasts, abrir pestanas, resultados).
    pub ui_tx: Sender<UiCommand>,
    /// egui::Context real, cableado en el primer frame del UI (via
    /// [`AppState::set_ui_ctx`]); hasta entonces la repintada es no-op.
    pub ui_ctx: Arc<OnceLock<egui::Context>>,
    /// Despierta el event loop del UI (reemplaza `CONTEXT` static).
    pub repaint: RepaintFn,
    /// Modificador APLICADO en el hook (capturado al arrancar; cambiarlo
    /// en Ajustes exige reinicio — el hook no lee la config en caliente).
    /// Lo consumen key_sim (soltar el modificador fisico) y los textos.
    pub hook_modifier: crate::hotkeys::Modifier,
    /// Modo de CapsLock aplicado en el hook (capturado al arrancar).
    pub hook_capsmode: crate::hotkeys::CapsMode,
}

impl AppState {
    /// Empaqueta los datos que antes se repartian como params sueltos.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        config: Arc<RwLock<Config>>,
        config_path: PathBuf,
        suspendido: Arc<AtomicBool>,
        clipboard: Arc<ClipboardManager>,
        history: Arc<Mutex<History>>,
        ollama_models: Arc<RwLock<Vec<String>>>,
        ui_tx: Sender<UiCommand>,
        hook_modifier: crate::hotkeys::Modifier,
        hook_capsmode: crate::hotkeys::CapsMode,
    ) -> Self {
        let ui_ctx = Arc::new(OnceLock::<egui::Context>::new());
        let repaint: RepaintFn = {
            let ctx_cell = Arc::clone(&ui_ctx);
            Arc::new(move || {
                if let Some(ctx) = ctx_cell.get() {
                    ctx.request_repaint();
                }
            })
        };
        Self {
            config,
            config_path,
            suspendido,
            clipboard,
            history,
            ollama_models,
            models_fetching: Arc::new(AtomicBool::new(false)),
            ui_tx,
            ui_ctx,
            repaint,
            hook_modifier,
            hook_capsmode,
        }
    }

    /// Cablea el contexto real de egui (primer frame; quien lo llama es
    /// el propio App en `update`, como hacia el static que reemplaza).
    pub fn set_ui_ctx(&self, ctx: egui::Context) {
        let _ = self.ui_ctx.set(ctx);
    }

    /// Despierta el event loop del UI desde cualquier thread. Es no-op
    /// si el UI todavia no renderizo su primer frame.
    #[allow(clippy::redundant_closure_for_method_calls)]
    pub fn repaint(&self) {
        (self.repaint)();
    }

    /// Snapshot de la config compartida (`None` si el lock esta poisoned).
    #[must_use]
    pub fn config_snapshot(&self) -> Option<Config> {
        self.config.read().ok().map(|c| c.clone())
    }

    /// Refetch de la lista de modelos en background (P3/U4): llena la
    /// cache compartida sin bloquear al llamador (tray o pestana Ajustes).
    ///
    /// Guardia interna: si ya hay un refetch en curso, no duplica.
    pub fn spawn_models_refresh(&self) {
        // Anti-duplicado (no existe Mutex::is_locked; AtomicBool de guardia).
        if self.models_fetching.swap(true, Ordering::Relaxed) {
            return;
        }

        let Some(ollama_config) = self.config_snapshot().map(|c| c.ollama) else {
            self.models_fetching.store(false, Ordering::Relaxed);
            return;
        };
        let cache = Arc::clone(&self.ollama_models);
        let fetching = Arc::clone(&self.models_fetching);

        std::thread::spawn(move || {
            let models = crate::runtime::global()
                .block_on(crate::ollama::OllamaClient::list_models(&ollama_config));
            match models {
                Ok(list) => {
                    *cache.write().unwrap_or_else(|e| e.into_inner()) = list;
                }
                Err(e) => tracing::warn!("Refetch de modelos fallo: {e}"),
            }
            fetching.store(false, Ordering::Relaxed);
        });
    }
}
