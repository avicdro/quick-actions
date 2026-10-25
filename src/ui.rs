use eframe::egui;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use crate::actions;
use crate::app_state::AppState;
use crate::hotkeys::{CapsMode, HotkeyCommand, Modifier};

// Protocolo de datos/comandos (cross-platform, movido a su propio modulo
// para que la logica no dependa de egui — de camino a la fase 1 de refactor).
pub use crate::ui_protocol::{HistoryData, HistoryEntryView, ResultData, UiChannels, UiCommand};

/// Pestanas de la ventana unificada.
#[derive(Debug, Clone, PartialEq)]
enum Tab {
    Result,
    History,
    Help,
    /// U4: ajustes in-app (config.toml editado sin salir de la GUI).
    Ajustes,
}

/// Estado de la aplicacion de UI.
///
/// Una sola ventana con pestanas: Resultado | Historial | Ayuda.
/// La ventana se muestra cuando hay contenido y se oculta cuando
/// el usuario la cierra.
pub struct QuickActionsApp {
    /// Canal UI (tray -> app)
    ui_rx: Receiver<UiCommand>,
    /// Canal hotkeys (hook -> app)
    hotkey_rx: Receiver<HotkeyCommand>,
    /// Estado compartido de la app (fase 1.3: config, clipboard, history,
    /// modelos, ui_tx, repaint — todo en uno, inyectado por `main`).
    state: Arc<AppState>,

    // --- Contenido de pestanas ---
    /// Resultado actual (ultimo).
    result_data: Option<ResultData>,
    /// Texto original editable para egui.
    result_original: String,
    /// Texto resultado editable para egui.
    result_resultado: String,
    /// Flag de "copiado" para feedback.
    result_copied: bool,
    /// Datos del historial.
    history_data: Option<HistoryData>,
    /// Estado actual (loading, error). Vacio = sin estado.
    status: String,

    // --- Estado de UI ---
    /// Pestana activa.
    current_tab: Tab,
    /// Si la ventana principal esta visible.
    window_open: bool,
    /// Estado visible anterior (para evitar enviar ViewportCommand innecesarios).
    was_visible: bool,
    /// Clipboard cacheado para egui (evita crear uno nuevo en cada frame).
    egui_clipboard: Option<arboard::Clipboard>,
    /// Si el toast de loading esta visible.
    toast_visible: bool,
    /// Estado visible anterior del toast.
    toast_was_visible: bool,
    /// Si el toast es de error (rojo) o loading (azul).
    toast_is_error: bool,
    /// Si los atajos estan pausados (P2).
    paused: bool,
    /// U1: momento en el que el toast se auto-oculta (None = hasta nuevo aviso).
    toast_expiry: Option<std::time::Instant>,
    /// Si hay aviso de update activo: URL de descarga en pestaña Resultado.
    update_url: Option<String>,

    /// U5: si ya aplicamos el tema custom (una sola vez al primer frame).
    style_applied: bool,
}

impl QuickActionsApp {
    /// Crea una nueva instancia.
    pub fn new(
        ui_rx: Receiver<UiCommand>,
        hotkey_rx: Receiver<HotkeyCommand>,
        state: Arc<AppState>,
    ) -> Self {
        Self {
            ui_rx,
            hotkey_rx,
            state,
            style_applied: false,
            result_data: None,
            result_original: String::new(),
            result_resultado: String::new(),
            result_copied: false,
            history_data: None,
            status: String::new(),
            current_tab: Tab::Result,
            window_open: false,
            was_visible: false,
            egui_clipboard: None,
            toast_visible: false,
            toast_was_visible: false,
            toast_is_error: false,
            paused: false,
            toast_expiry: None,
            update_url: None,
        }
    }

    /// Procesa comandos UI del canal (tray, workers).
    fn process_ui_commands(&mut self) {
        while let Ok(cmd) = self.ui_rx.try_recv() {
            tracing::info!("UI command recibido");
            match cmd {
                UiCommand::ShowResult(data) => {
                    self.result_original = data.original.clone();
                    self.result_resultado = data.resultado.clone();
                    self.result_copied = false;
                    self.result_data = Some(data);
                    self.current_tab = Tab::Result;
                    self.window_open = true;
                    // Un resultado normal cancela cualquier aviso de update
                    self.update_url = None;
                }
                UiCommand::ShowHistory(data) => {
                    self.history_data = Some(data);
                    self.current_tab = Tab::History;
                    self.window_open = true;
                }
                // U4: "Configuración" del tray abre la pestana Ajustes.
                UiCommand::ShowAjustes => {
                    self.current_tab = Tab::Ajustes;
                    self.window_open = true;
                }
                // U5: la ayuda se dibuja estructurada (el texto del mensaje
                // queda obsoleto; se mantiene por compatibilidad del canal).
                UiCommand::ShowHelp(_) => {
                    self.current_tab = Tab::Help;
                    self.window_open = true;
                }
                UiCommand::SetStatus(msg) => {
                    // NO abrir la ventana principal. Solo actualizar el toast.
                    if msg.is_empty() {
                        self.toast_visible = false;
                        self.toast_expiry = None;
                    } else {
                        self.toast_is_error = msg.starts_with("Error");
                        self.toast_visible = true;
                        // U1: clase deduce el auto-hide; Loading nunca expira.
                        let kind = crate::ui_protocol::classify_status(&msg);
                        self.toast_expiry = kind.expiry_secs().map(|secs| {
                            std::time::Instant::now() + std::time::Duration::from_secs_f64(secs)
                        });
                    }
                    self.status = msg;
                }
                UiCommand::SetPaused(v) => {
                    self.paused = v;
                    if v {
                        self.status = "Pausado — atajos desactivados".to_string();
                        self.toast_visible = true;
                        // U1: ya no vive al infinito (Notice 3.5s).
                        self.toast_expiry = Some(
                            std::time::Instant::now()
                                + std::time::Duration::from_secs_f64(
                                    crate::ui_protocol::NOTICE_TOAST_SECS,
                                ),
                        );
                    } else {
                        // Reanudar: limpiar status para que el proximo
                        // SetStatus de una accion defina el toast.
                        self.status = String::new();
                        self.toast_visible = false;
                        self.toast_expiry = None;
                    }
                }
                UiCommand::ShowUpdate { version, url, body } => {
                    // Modo aviso: pestaña Resultado reutilizada como visor
                    // de release notes + enlace de descarga.
                    let body = if body.trim().is_empty() {
                        "(sin notas de la version)".to_string()
                    } else {
                        // Truncar por chars (UTF-8 safe) a ~1200
                        body.chars().take(1200).collect()
                    };
                    self.result_original = body;
                    self.update_url = Some(url);
                    self.result_data = Some(ResultData {
                        titulo: format!("Nueva version disponible: {version}"),
                        original: String::new(),
                        resultado: String::new(),
                    });
                    self.current_tab = Tab::Result;
                    self.window_open = true;
                }
                UiCommand::CloseAll => {
                    self.window_open = false;
                }
            }
        }
    }

    /// Procesa hotkeys: recibe del canal y spawnea threads de trabajo.
    fn process_hotkeys(&self) {
        while let Ok(cmd) = self.hotkey_rx.try_recv() {
            tracing::info!("Hotkey: {:?}", cmd);
            let state = Arc::clone(&self.state);

            std::thread::spawn(move || {
                if let Err(e) = actions::handle_command(cmd, &state) {
                    tracing::error!("Error en handler: {e}");
                }
            });
        }
    }

    /// Dibuja la pestana de resultado.
    fn draw_result_tab(&mut self, ui: &mut egui::Ui) {
        let m = self.state.hook_modifier.name();
        if self.result_data.is_none() {
            ui.label(format!(
                "No hay resultado todavía. Selecciona texto y pulsa {m}+T/R/Enter."
            ));
            return;
        }

        let Some(data) = self.result_data.as_ref() else {
            return;
        };

        // Modo aviso de update (ShowUpdate): visor de notas + enlace.
        if let Some(url) = self.update_url.clone() {
            ui.label(egui::RichText::new("Notas de la version:").color(egui::Color32::GRAY));
            ui.add_sized(
                [ui.available_width(), 200.0],
                egui::TextEdit::multiline(&mut self.result_original.as_str()).interactive(false),
            );
            ui.add(egui::Separator::default().horizontal());
            let url_click = url.clone();
            ui.horizontal(|ui| {
                if ui
                    .link(egui::RichText::new("Descargar desde GitHub ↗").strong())
                    .clicked()
                {
                    open_url_in_browser(&url_click);
                }
            });
            ui.label(
                egui::RichText::new("(no instala nada automaticamente)")
                    .small()
                    .color(egui::Color32::GRAY),
            );
            return;
        }

        ui.label(egui::RichText::new("Texto original:").color(egui::Color32::GRAY));
        ui.add_sized(
            [ui.available_width(), 120.0],
            egui::TextEdit::multiline(&mut self.result_original.as_str()).interactive(false),
        );

        ui.add(egui::Separator::default().horizontal());

        ui.label(
            egui::RichText::new(&data.titulo)
                .color(egui::Color32::from_rgb(0, 0, 128))
                .strong(),
        );
        ui.add_sized(
            [ui.available_width(), ui.available_height() - 60.0],
            egui::TextEdit::multiline(&mut self.result_resultado.as_str()).interactive(false),
        );

        ui.horizontal(|ui| {
            if ui.button("Copiar resultado").clicked() {
                if let Some(ref mut cb) = self.egui_clipboard {
                    let _ = cb.set_text(&self.result_resultado);
                    self.result_copied = true;
                }
            }
            if self.result_copied {
                ui.label(egui::RichText::new("Copiado!").color(egui::Color32::GREEN));
            }
        });
    }

    /// Dibuja la pestana de historial.
    fn draw_history_tab(&mut self, ui: &mut egui::Ui) {
        let m = self.state.hook_modifier.name();
        let entries = self.history_data.as_ref().map(|d| d.entries.clone());

        match entries {
            None => {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Sin historial todavia. Pulsa {m}+V para cargarlo."
                    ))
                    .color(theme::TEXT_DIM),
                );
            }
            Some(ref entries) if entries.is_empty() => {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("El historial esta vacio.").color(theme::TEXT_DIM));
            }
            Some(entries) => {
                ui.label(format!("{} entradas:", entries.len()));
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entry in &entries {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("[{}] {}", entry.index, entry.fecha))
                                    .color(egui::Color32::GRAY),
                            );
                            ui.label(&entry.preview);
                            if ui.button("Copiar original").clicked() {
                                if let Some(ref mut cb) = self.egui_clipboard {
                                    let _ = cb.set_text(&entry.original);
                                }
                            }
                        });
                        ui.separator();
                    }
                });
            }
        }

        // Boton para recargar historial
        if ui.button("Actualizar").clicked() {
            if let Ok(hist) = self.state.history.lock() {
                let entries: Vec<HistoryEntryView> = hist
                    .entries()
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        let preview = if e.original.len() > 60 {
                            format!("{}...", &e.original[..60])
                        } else {
                            e.original.clone()
                        };
                        HistoryEntryView {
                            index: i + 1,
                            preview,
                            fecha: e.fecha.clone(),
                            original: e.original.clone(),
                        }
                    })
                    .collect();
                self.history_data = Some(HistoryData {
                    entries,
                    selected: 0,
                });
            }
        }
    }

    /// Dibuja la pestana de ayuda (U5: filas estructuradas con badges de
    /// tecla en vez de un bloque ASCII).
    fn draw_help_tab(&self, ui: &mut egui::Ui) {
        let mod_name = self.state.hook_modifier.name();
        let toggle_note = self.state.hook_modifier == Modifier::CapsLock
            && self.state.hook_capsmode == CapsMode::Toggle;

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(6.0);
            ui.strong("Asistente de IA");
            ui.label(
                egui::RichText::new("Selecciona texto en cualquier app y pulsa:")
                    .color(theme::TEXT_DIM),
            );
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);

            for (_cmd, label, desc) in crate::hotkeys::HOTKEY_ACTIONS {
                ui.horizontal(|ui| {
                    ui.add_space(2.0);
                    key_badge(ui, &format!("{mod_name}+{label}"));
                    ui.add_space(4.0);
                    ui.label(desc);
                });
            }

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(
                    "• El resultado corregido se copia al portapapeles.\n\
                     • Doble click en el historial copia la entrada.\n\
                     • La pausa total y los atajos individuales se cambian en el menu\n  \
                     del tray y en la pestana Ajustes.",
                )
                .color(theme::TEXT_DIM),
            );
            if toggle_note {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(
                        "⚠ CapsLock esta en modo toggle: un tap lo latchea y la\n  \
                         proxima tecla de atajo dispara (no alterna mayusculas).",
                    )
                    .color(theme::ACCENT),
                );
            }
        });
    }

    /// Dibuja la pestana de ajustes (U4): config.toml editable desde la GUI.
    ///
    /// Cada widget persiste al momento del cambio (`Config::write_config_key`
    /// preserva comentarios via toml_edit) y replica el valor en el `RwLock`
    /// compartido para efecto inmediato. Solo el modificador/capsmode exige
    /// reinicio (los statics del hook se levantan al arrancar).
    fn draw_settings_tab(&self, ui: &mut egui::Ui) {
        // Snapshot: bajo el lock SOLO para clonar (no sostener el guard de
        // lectura mientras la UI dibuja: los commits puntuales hacen write()).
        let snapshot = self.state.config.read().ok().map(|c| c.clone());
        let mut cfg = match snapshot {
            Some(c) => c,
            None => {
                ui.label("Config bloqueada (poisoned). Revisa logs/panic.log.");
                return;
            }
        };

        // V3: reservar la altura de la fila de botones para que el scroll
        // llegue hasta abajo y dicha fila nunca quede fuera de la ventana.
        let buttons_h = 46.0;
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - buttons_h)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // ================================================== Ollama
                settings_card(ui, "Ollama", |ui| {
                    egui::Grid::new("grid_ollama")
                        .num_columns(2)
                        .spacing([16.0, 11.0])
                        .show(ui, |ui| {
                            // Modelo: campo libre + dropdown de la cache.
                            ui.label("Modelo:");
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut cfg.ollama.model)
                                                .desired_width(190.0)
                                                .font(egui::TextStyle::Monospace),
                                        )
                                        .changed()
                                    {
                                        self.persist_ollama_full(&cfg.ollama);
                                    }
                                    let models = self
                                        .state
                                        .ollama_models
                                        .read()
                                        .ok()
                                        .map(|m| m.clone())
                                        .unwrap_or_default();
                                    let mut box_selected = false;
                                    egui::ComboBox::from_id_salt("combo_modelo")
                                        .selected_text(cfg.ollama.model.as_str())
                                        .width(175.0)
                                        .show_ui(ui, |ui| {
                                            if models.is_empty() {
                                                let _ = ui.selectable_label(
                                                    false,
                                                    "(lista vacia: pulsa Actualizar)",
                                                );
                                            }
                                            for model in models {
                                                if ui
                                                    .selectable_value(
                                                        &mut cfg.ollama.model,
                                                        model.clone(),
                                                        &model,
                                                    )
                                                    .clicked()
                                                {
                                                    box_selected = true;
                                                }
                                            }
                                        });
                                    if box_selected {
                                        self.persist_ollama_full(&cfg.ollama);
                                    }
                                });
                            });
                            ui.end_row();

                            // Endpoint: campo ancho.
                            ui.label("Endpoint:");
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut cfg.ollama.endpoint)
                                        .hint_text("http://localhost:11434/api/generate")
                                        .desired_width(400.0),
                                )
                                .changed()
                            {
                                self.persist_ollama_full(&cfg.ollama);
                            }
                            ui.end_row();

                            // Timeout (DragValue ya clampa al rango).
                            ui.label("Timeout:");
                            ui.horizontal(|ui| {
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut cfg.ollama.timeout_seconds)
                                            .range(5..=600)
                                            .speed(0.5)
                                            .suffix(" s"),
                                    )
                                    .changed()
                                {
                                    self.persist_ollama_full(&cfg.ollama);
                                }
                                ui.label(
                                    egui::RichText::new("segundos de espera para la API")
                                        .color(theme::TEXT_DIM),
                                );
                            });
                            ui.end_row();
                        });
                });

                // ================================================== Hotkeys
                settings_card(ui, "Atajos", |ui| {
                    // Modificador (alt/capslock) — requiere reinicio (statics).
                    let modifier_applied = self.state.hook_modifier.name().to_lowercase();
                    let mut modifier_changed = false;
                    egui::ComboBox::from_label("Modificador")
                        .selected_text(cfg.hotkeys.modifier.clone())
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(
                                    &mut cfg.hotkeys.modifier,
                                    "alt".to_string(),
                                    "alt",
                                )
                                .clicked()
                            {
                                modifier_changed = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut cfg.hotkeys.modifier,
                                    "capslock".to_string(),
                                    "capslock",
                                )
                                .clicked()
                            {
                                modifier_changed = true;
                            }
                        });
                    if modifier_changed {
                        let _ = crate::config::Config::write_config_key(
                            &self.state.config_path,
                            "hotkeys",
                            "modifier",
                            cfg.hotkeys.modifier.clone(),
                        );
                        if let Ok(mut c) = self.state.config.write() {
                            c.hotkeys.modifier = cfg.hotkeys.modifier.clone();
                        }
                    }
                    if cfg.hotkeys.modifier != modifier_applied {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 196, 92),
                            "↻ El cambio de modificador requiere reiniciar la app",
                        );
                    }

                    // Capsmode: derivar bool <-> "toggle"/"hold"; solo con capslock.
                    let capsmode_applied = match self.state.hook_capsmode {
                        crate::hotkeys::CapsMode::Toggle => "toggle",
                        crate::hotkeys::CapsMode::Hold => "hold",
                    };
                    let mut caps_toggle = cfg.hotkeys.capsmode == "toggle";
                    let caps_checked = ui.add_enabled(
                        cfg.hotkeys.modifier == "capslock",
                        egui::Checkbox::new(
                            &mut caps_toggle,
                            "CapsLock modo toggle (tap latched; ya no alterna mayusculas)",
                        ),
                    );
                    if caps_checked.changed() {
                        cfg.hotkeys.capsmode =
                            if caps_toggle { "toggle" } else { "hold" }.to_string();
                        let _ = crate::config::Config::write_config_key(
                            &self.state.config_path,
                            "hotkeys",
                            "capsmode",
                            cfg.hotkeys.capsmode.clone(),
                        );
                        if let Ok(mut c) = self.state.config.write() {
                            c.hotkeys.capsmode = cfg.hotkeys.capsmode.clone();
                        }
                    }
                    if cfg.hotkeys.capsmode != capsmode_applied {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 196, 92),
                            "↻ El cambio de capsmode requiere reiniciar la app",
                        );
                    }

                    // Pausa modular per-atajo (U3): grid 2 columnas.
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("Habilitar individualmente:").color(theme::TEXT_DIM),
                    );
                    egui::Grid::new("grid_atajos")
                        .num_columns(2)
                        .spacing([12.0, 7.0])
                        .show(ui, |ui| {
                            for (i, (cmd, label, desc)) in
                                crate::hotkeys::HOTKEY_ACTIONS.iter().enumerate()
                            {
                                let mut on = crate::hotkeys::is_hotkey_enabled(*cmd, &cfg.hotkeys);
                                if ui
                                    .checkbox(
                                        &mut on,
                                        format!(
                                            "{desc} ({}+{label})",
                                            self.state.hook_modifier.name()
                                        ),
                                    )
                                    .changed()
                                {
                                    let _ = crate::config::Config::write_hotkey(
                                        &self.state.config_path,
                                        cmd.config_field(),
                                        on,
                                    );
                                    if let Ok(mut c) = self.state.config.write() {
                                        *crate::hotkeys::hotkey_switch(*cmd, &mut c.hotkeys) = on;
                                    }
                                }
                                // 2 por fila.
                                if i % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                });

                // ================================================== App
                settings_card(ui, "App", |ui| {
                    // Autostart: estado vigente de configuracion (el tray/git
                    // writes ya repican el registro); al cambiar, sincroniza
                    // registro+config.
                    let mut autostart = cfg.app.autostart;
                    if ui.checkbox(&mut autostart, "Iniciar con Windows").changed() {
                        match crate::autostart::set_enabled(autostart) {
                            Ok(()) => {
                                let _ = crate::config::Config::write_autostart(
                                    &self.state.config_path,
                                    autostart,
                                );
                                if let Ok(mut c) = self.state.config.write() {
                                    c.app.autostart = autostart;
                                }
                                let msg = if autostart {
                                    "Auto-arranque activado"
                                } else {
                                    "Auto-arranque desactivado"
                                };
                                let _ =
                                    self.state.ui_tx.send(UiCommand::SetStatus(msg.to_string()));
                            }
                            Err(e) => {
                                let _ = self
                                    .state
                                    .ui_tx
                                    .send(UiCommand::SetStatus(format!("Error: {e}")));
                            }
                        }
                        self.state.repaint();
                    }

                    // Check de updates.
                    if ui
                        .checkbox(
                            &mut cfg.app.check_updates,
                            "Avisar de nuevas versiones al arrancar",
                        )
                        .changed()
                    {
                        let _ = crate::config::Config::write_config_key(
                            &self.state.config_path,
                            "app",
                            "check_updates",
                            cfg.app.check_updates,
                        );
                        if let Ok(mut c) = self.state.config.write() {
                            c.app.check_updates = cfg.app.check_updates;
                        }
                    }

                    // Repositorio (grid alineado con ollama).
                    egui::Grid::new("grid_app")
                        .num_columns(2)
                        .spacing([16.0, 11.0])
                        .show(ui, |ui| {
                            ui.label("Repositorio:");
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut cfg.app.repository)
                                        .hint_text("usuario/repo")
                                        .desired_width(200.0),
                                )
                                .changed()
                            {
                                let _ = crate::config::Config::write_config_key(
                                    &self.state.config_path,
                                    "app",
                                    "repository",
                                    cfg.app.repository.clone(),
                                );
                                if let Ok(mut c) = self.state.config.write() {
                                    c.app.repository = cfg.app.repository.clone();
                                }
                            }
                            ui.label(
                                egui::RichText::new("usado por el check de updates")
                                    .color(theme::TEXT_DIM),
                            );
                            ui.end_row();
                        });
                });
            });

        // ================================================== Botones
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Abrir config.toml (avanzado)").clicked() {
                let _ = std::process::Command::new("notepad")
                    .arg(&self.state.config_path)
                    .spawn();
            }
            if ui.button("Actualizar lista de modelos").clicked() {
                self.state.spawn_models_refresh();
            }
        });
    }

    /// Persiste la seccion [ollama] completa (model/endpoint/timeout) y la
    /// replica en el RwLock. Mas simple que adivinar cual key cambio: la
    /// seccion es chica y toml_edit preserva el formato de las demas.
    fn persist_ollama_full(&self, ollama: &crate::config::OllamaConfig) {
        let _ = crate::config::Config::write_config_key(
            &self.state.config_path,
            "ollama",
            "model",
            ollama.model.clone(),
        );
        let _ = crate::config::Config::write_config_key(
            &self.state.config_path,
            "ollama",
            "endpoint",
            ollama.endpoint.clone(),
        );
        let _ = crate::config::Config::write_config_key(
            &self.state.config_path,
            "ollama",
            "timeout_seconds",
            // toml_edit solo tiene From<i64>: el campo ya esta saneado y
            // clampado a 5..=600 en la UI.
            i64::try_from(ollama.timeout_seconds).unwrap_or(300),
        );
        if let Ok(mut c) = self.state.config.write() {
            c.ollama = ollama.clone();
        }
    }
}

impl eframe::App for QuickActionsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Guardar contexto la primera vez (fase 1.3: en AppState concret),
        // para que hook/tray/workers despierten el event loop por ahi.
        if self.state.ui_ctx.get().is_none() {
            self.state.set_ui_ctx(ctx.clone());
        }

        // U1: toasts transitorios (auto-hide).
        if self.toast_visible
            && self
                .toast_expiry
                .is_some_and(|exp| std::time::Instant::now() >= exp)
        {
            self.toast_visible = false;
            self.toast_expiry = None;
            self.status.clear();
        }

        // Cancelar cierre de ventana (evita que eframe cierre la app).
        // En su lugar, ocultamos la ventana.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.window_open = false;
        }

        ctx.set_pixels_per_point(1.3);

        // U5: tema custom del app al primer frame.
        if !self.style_applied {
            apply_theme(ctx);
            self.style_applied = true;
            // V5: al arrancar SOLO tray — eframe muestra la ventana tras el
            // primer frame (anti-flash, egui PR2279) ignorando visible(false);
            // la capamos como tool window (offscreen 1x1 sin botón taskbar).
            set_toolwindow(true);
        }

        // Inicializar clipboard cacheado si aun no existe.
        if self.egui_clipboard.is_none() {
            self.egui_clipboard = arboard::Clipboard::new().ok();
        }

        self.process_hotkeys();
        self.process_ui_commands();

        // Mostrar/ocultar ventana segun estado.
        if self.window_open != self.was_visible {
            if self.window_open {
                // Mostrar ventana como ventana estandar de Windows:
                // - Con decoraciones (barra de titulo, botones minimizar/maximizar/cerrar)
                // - Resizable
                // - Centrada en pantalla
                // - Con foco
                ctx.send_viewport_cmd(egui::ViewportCommand::Title("Quick Actions".to_string()));
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
                set_toolwindow(false); // V4-fix: volver a ventana normal
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(900.0, 700.0)));
                let monitor = ctx
                    .input(|i| i.viewport().monitor_size)
                    .unwrap_or(egui::vec2(1920.0, 1080.0));
                let pos = egui::pos2((monitor.x - 900.0) / 2.0, (monitor.y - 700.0) / 2.0);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else {
                // V4-fix: NO usar ViewportCommand::Visible(false) — bug egui
                // (emilk/egui#5229, #3655): una viewport que se pasa a
                // invisible deja de pintarse y los comandos posteriores
                // (Visible(true)) no se aplican: la ventana no reabre.
                //
                // Alternativa tray-app: la ventana queda "visible" para
                // winit (offscreen 1x1, frames y toasts siguen vivos), pero
                // a nivel Win32 se le pone WS_EX_TOOLWINDOW: sin boton en
                // taskbar ni Alt-Tab. Al abrir se quita el estilo.
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1.0, 1.0)));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                    -10000.0, -10000.0,
                )));
                set_toolwindow(true);
            }
            self.was_visible = self.window_open;
        }

        // Dibujar contenido solo si la ventana esta visible
        if self.window_open {
            egui::CentralPanel::default().show(ctx, |ui| {
                // Barra de pestanas (V1: pills legibles, acento en activa).
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let tabs = [
                        (Tab::Result, "Resultado"),
                        (Tab::History, "Historial"),
                        (Tab::Help, "Ayuda"),
                        (Tab::Ajustes, "Ajustes"),
                    ];
                    for (tab, label) in tabs {
                        let selected = self.current_tab == tab;
                        // V3: sobre el chip ambar de seleccion, texto oscuro
                        // (contraste); inactivas dim legibles.
                        let mut text = egui::RichText::new(label).size(15.0).color(if selected {
                            egui::Color32::from_rgb(33, 26, 8)
                        } else {
                            theme::TEXT_DIM
                        });
                        if selected {
                            text = text.strong();
                        }
                        if ui.selectable_label(selected, text).clicked() {
                            self.current_tab = tab;
                        }
                    }
                });
                ui.separator();

                // Contenido de la pestana activa
                match self.current_tab {
                    Tab::Result => self.draw_result_tab(ui),
                    Tab::History => self.draw_history_tab(ui),
                    Tab::Help => self.draw_help_tab(ui),
                    Tab::Ajustes => self.draw_settings_tab(ui),
                }
            });
        }

        // Toast overlay: mini-ventana siempre encima, sin decoraciones,
        // en la esquina inferior derecha. No roba foco.
        //
        // show_viewport_immediate debe llamarse en CADA FRAME mientras el
        // toast este visible. Si solo se llama cuando cambia el estado,
        // el viewport se cierra inmediatamente (eframe lo considera "no redrawn").
        if self.toast_visible {
            let toast_id = egui::ViewportId::from_hash_of("toast");
            let monitor = ctx
                .input(|i| i.viewport().monitor_size)
                .unwrap_or(egui::vec2(1920.0, 1080.0));
            let toast_w = 280.0;
            let toast_h = 56.0;
            let toast_x = monitor.x - toast_w - 24.0;
            let toast_y = monitor.y - toast_h - 72.0;

            let status = self.status.clone();
            let is_error = self.toast_is_error;
            let paused = self.paused;
            // U1: clase del toast para elegir icono (spinner solo en loading).
            let kind = crate::ui_protocol::classify_status(&status);

            ctx.show_viewport_immediate(
                toast_id,
                egui::ViewportBuilder::default()
                    .with_title("")
                    .with_inner_size([toast_w, toast_h])
                    .with_position([toast_x, toast_y])
                    .with_decorations(false)
                    .with_always_on_top()
                    .with_taskbar(false)
                    .with_resizable(false)
                    .with_transparent(true),
                move |ctx, _class| {
                    // U5: pill toast con la paleta del app.
                    let bg = if is_error {
                        egui::Color32::from_rgb(42, 16, 18) // rojo profundo
                    } else {
                        theme::BG_ELEV
                    };

                    let border = if is_error {
                        theme::ERROR
                    } else {
                        theme::ACCENT
                    };

                    // Animacion de pulso: el borde se aclara y oscila
                    let time = ctx.input(|i| i.time);
                    let pulse = (time * 2.0).sin() * 0.5 + 0.5; // 0.0 a 1.0
                    let border_alpha = 60.0 + pulse * 80.0;
                    let border_color = egui::Color32::from_rgba_premultiplied(
                        border.r(),
                        border.g(),
                        border.b(),
                        border_alpha as u8,
                    );

                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::none()
                                .fill(bg)
                                .rounding(egui::Rounding::same(10.0))
                                .stroke(egui::Stroke::new(1.5, border_color))
                                .inner_margin(egui::Margin {
                                    left: 16.0,
                                    right: 16.0,
                                    top: 10.0,
                                    bottom: 10.0,
                                }),
                        )
                        .show(ctx, |ui| {
                            ui.horizontal_centered(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(10.0, 0.0);

                                if paused {
                                    // Icono de pausa: circulo ambar con dos barras
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(20.0, 20.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    let center = rect.center();
                                    painter.circle_filled(
                                        center,
                                        9.0,
                                        egui::Color32::from_rgb(232, 176, 60),
                                    );
                                    let bar_color = egui::Color32::from_rgb(45, 38, 22);
                                    for dx in [-3.0f32, 3.0] {
                                        painter.rect_filled(
                                            egui::Rect::from_center_size(
                                                center + egui::vec2(dx, 0.0),
                                                egui::vec2(2.8, 10.0),
                                            ),
                                            1.0,
                                            bar_color,
                                        );
                                    }
                                } else if kind == crate::ui_protocol::ToastKind::Loading {
                                    // Spinner con tamaño personalizado
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(20.0, 20.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    let time = ui.input(|i| i.time as f32);
                                    let angle = time * std::f32::consts::TAU;
                                    let center = rect.center();
                                    let radius = 8.0;
                                    // Arc spinner
                                    painter.circle_stroke(
                                        center,
                                        radius,
                                        egui::Stroke::new(
                                            2.0,
                                            egui::Color32::from_rgb(100, 140, 220),
                                        ),
                                    );
                                    // Punto girando
                                    let dot_pos = center
                                        + egui::vec2(radius * angle.cos(), radius * angle.sin());
                                    painter.circle_filled(
                                        dot_pos,
                                        3.0,
                                        egui::Color32::from_rgb(140, 180, 255),
                                    );
                                } else if kind == crate::ui_protocol::ToastKind::Notice {
                                    // Aviso informativo: circulo ambar con "i"
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(20.0, 20.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    let center = rect.center();
                                    painter.circle_filled(
                                        center,
                                        9.0,
                                        egui::Color32::from_rgb(232, 176, 60),
                                    );
                                    painter.text(
                                        center,
                                        egui::Align2::CENTER_CENTER,
                                        "i",
                                        egui::FontId::proportional(12.0),
                                        egui::Color32::from_rgb(40, 32, 12),
                                    );
                                } else {
                                    // Icono de error: circulo con !
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(20.0, 20.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    let center = rect.center();
                                    painter.circle(
                                        center,
                                        9.0,
                                        egui::Color32::from_rgb(220, 80, 80),
                                        egui::Stroke::NONE,
                                    );
                                    painter.text(
                                        center,
                                        egui::Align2::CENTER_CENTER,
                                        "!",
                                        egui::FontId::proportional(13.0),
                                        egui::Color32::WHITE,
                                    );
                                }

                                // Texto del estado
                                let text_color = if is_error {
                                    egui::Color32::from_rgb(255, 210, 210)
                                } else if paused {
                                    egui::Color32::from_rgb(250, 230, 190)
                                } else {
                                    theme::TEXT
                                };
                                ui.label(
                                    egui::RichText::new(&status)
                                        .color(text_color)
                                        .size(14.0)
                                        .strong(),
                                );
                            });
                        });
                },
            );
        } else if self.toast_was_visible {
            // Cerrar el toast viewport cuando deja de ser visible
            ctx.send_viewport_cmd_to(
                egui::ViewportId::from_hash_of("toast"),
                egui::ViewportCommand::Close,
            );
            self.toast_was_visible = false;
        }

        // Repaint: rapido con ventana o toast (30ms), lento sin nada (5000ms).
        let needs_repaint = self.window_open || self.toast_visible;
        let repaint_ms = if needs_repaint { 30 } else { 5000 };
        ctx.request_repaint_after(std::time::Duration::from_millis(repaint_ms));
    }
}

/// U5: paleta de la marca (navy + ambar, familia del icono del tray).
pub mod theme {
    use egui::Color32;

    /// Fondo principal de la ventana.
    pub const BG_PANEL: Color32 = Color32::from_rgb(13, 19, 33);
    /// Fondo de panales/areas elevadas (toast, popups).
    pub const BG_ELEV: Color32 = Color32::from_rgb(19, 28, 48);
    /// Fondo extreme (inputs).
    pub const BG_INPUT: Color32 = Color32::from_rgb(10, 16, 28);
    /// Fondo de widgets (botones/checkbox base).
    pub const BG_WIDGET: Color32 = Color32::from_rgb(22, 31, 53);
    /// Fondo de widget con hover.
    pub const BG_WIDGET_HOVER: Color32 = Color32::from_rgb(29, 43, 78);
    /// V1: fondo de card (secciones de Ajustes) — un escalon sobre el panel.
    pub const CARD: Color32 = Color32::from_rgb(18, 27, 46);
    /// Acento ambar (misma familia que el icono del tray).
    pub const ACCENT: Color32 = Color32::from_rgb(255, 201, 60);
    /// Texto secundario contrastado (V1: sube el dim antiguo).
    pub const TEXT_DIM: Color32 = Color32::from_rgb(186, 195, 210);
    /// Texto primario.
    pub const TEXT: Color32 = Color32::from_rgb(232, 230, 227);
    /// Linea/borde sutil.
    pub const STROKE: Color32 = Color32::from_rgb(58, 69, 96);
    /// Error (icono/fondo).
    pub const ERROR: Color32 = Color32::from_rgb(200, 80, 80);
}

/// V4-fix: HWND de la ventana principal.
///
/// Fast path: FindWindowW por titulo exacto ("Quick Actions", fijado al
/// abrir). Fallback para el arranque (titulo "QuickActions" del app_name):
/// EnumWindows y primera top-level visible de este proceso con titulo que
/// empiece por "Quick" (el viewport del toast tiene titulo vacio).
#[cfg(windows)]
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
#[cfg(windows)]
extern "system" fn enum_quick_hwnd(hwnd: windows::Win32::Foundation::HWND, out: LPARAM) -> BOOL {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != std::process::id() || !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1); // seguir enumerando
        }
        let mut buf = [0u16; 64];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        if title.starts_with("Quick") {
            let dst = out.0 as *mut windows::Win32::Foundation::HWND;
            if !dst.is_null() {
                *dst = hwnd;
            }
            return BOOL(0); // encontrado: parar
        }
        BOOL(1)
    }
}

#[cfg(windows)]
fn main_hwnd() -> Option<HWND> {
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowW, GetWindowThreadProcessId,
    };

    unsafe {
        // Fast path: ya abrio la ventana alguna vez (titulo con espacio).
        if let Ok(hwnd) = FindWindowW(None, windows::core::w!("Quick Actions")) {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == std::process::id() {
                return Some(hwnd);
            }
        }

        // Fallback: EnumWindows de este proceso (titulo "QuickActions" al
        // arrancar). Solo si nunca se ha abierto con titulo nuevo.
        let mut result = HWND(std::ptr::null_mut());
        let sptr = &mut result as *mut HWND as isize;
        if EnumWindows(Some(enum_quick_hwnd), LPARAM(sptr)).is_ok() && !result.0.is_null() {
            return Some(result);
        }
        None
    }
}

/// V4-fix: alterna WS_EX_TOOLWINDOW en la ventana principal.
///
/// Tool window = aparece en la taskbar ni en Alt-Tab aunque el HWND siga
/// tecnicamente visible (el truco de egui de ventana offscreen). Con esto
/// el "cerrar = hide" funciona sin el bug de Visible(false) de egui.
#[cfg(windows)]
fn set_toolwindow(tool: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_TOOLWINDOW,
    };

    let Some(hwnd) = main_hwnd() else { return };

    unsafe {
        // GWL_EXSTYLE es WINDOW_LONG_PTR_INDEX (0.58 devuelve isize).
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let ex = if tool {
            ex | (WS_EX_TOOLWINDOW.0 as isize)
        } else {
            ex & !(WS_EX_TOOLWINDOW.0 as isize)
        };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        // FRAMECHANGED para que la taskbar aplique el cambio al momento.
        let _ = SetWindowPos(
            hwnd,
            HWND(std::ptr::null_mut()),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(windows))]
fn set_toolwindow(_tool: bool) {}

/// U5+V1: aplica el tema visual del app (dark navy + acento ambar) una vez.
fn apply_theme(ctx: &egui::Context) {
    use egui::{Color32, FontId, Shadow, Stroke, Vec2};

    ctx.style_mut(|style| {
        let v = &mut style.visuals;
        v.panel_fill = theme::BG_PANEL;
        v.window_fill = theme::BG_ELEV;
        v.extreme_bg_color = theme::BG_INPUT;
        v.window_stroke = Stroke::new(1.0, theme::STROKE);
        v.window_shadow = Shadow {
            offset: Vec2::new(0.0, 5.0),
            blur: 22.0,
            spread: 2.0,
            color: Color32::from_black_alpha(140),
        };
        v.selection.bg_fill = Color32::from_rgba_premultiplied(255, 201, 60, 70);
        v.selection.stroke = Stroke::new(1.0, theme::ACCENT);

        // Rounding global.
        v.window_rounding = 8.0.into();
        v.menu_rounding = 6.0.into();
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.rounding = egui::Rounding::same(6.0);
        }
        // V1 a11y: checkmarks e iconos bien visibles (texto claro, acento en
        // hover/activo); los strokes mas gruesos para baja vision.
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.6, theme::TEXT_DIM);
        v.widgets.inactive.fg_stroke = Stroke::new(1.6, theme::TEXT);
        v.widgets.open.fg_stroke = Stroke::new(1.6, theme::TEXT);
        v.widgets.hovered.fg_stroke = Stroke::new(1.6, theme::ACCENT);
        v.widgets.active.fg_stroke = Stroke::new(1.6, theme::ACCENT);
        v.widgets.inactive.bg_fill = theme::BG_WIDGET;
        v.widgets.inactive.weak_bg_fill = theme::BG_WIDGET;
        v.widgets.hovered.bg_fill = theme::BG_WIDGET_HOVER;
        v.widgets.hovered.weak_bg_fill = theme::BG_WIDGET_HOVER;
        v.widgets.active.bg_fill = Color32::from_rgb(42, 62, 110);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(42, 62, 110);
        v.widgets.hovered.bg_stroke = Stroke::new(1.2, theme::STROKE);
        v.widgets.active.bg_stroke = Stroke::new(1.2, theme::ACCENT);

        // Texto.
        v.override_text_color = Some(theme::TEXT);
        // Fondo de text edits etc.
        v.extreme_bg_color = theme::BG_INPUT;

        // V1: escala tipografica accesible (por sobre el default de egui).
        style.text_styles = [
            (egui::TextStyle::Body, FontId::proportional(15.5)),
            (egui::TextStyle::Button, FontId::proportional(14.5)),
            (egui::TextStyle::Heading, FontId::proportional(18.5)),
            (egui::TextStyle::Small, FontId::proportional(12.0)),
            (egui::TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();

        // V1: espacios y tamanos de interaccion.
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 7.0);
        style.spacing.icon_width = 22.0;
        style.spacing.icon_width_inner = 14.0;
        style.spacing.icon_spacing = 7.0;
    });
}

/// V1: card de seccion para la pestana Ajustes (frame amurallado con
/// titulo acento). `body` dibuja el contenido dentro del card.
fn settings_card<R>(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui) -> R) {
    ui.add_space(6.0);
    egui::Frame::none()
        .fill(theme::CARD)
        .rounding(egui::Rounding::same(10.0))
        .stroke(egui::Stroke::new(1.0, theme::STROKE))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .strong()
                        .size(16.5)
                        .color(theme::ACCENT),
                );
                ui.add_space(4.0);
                body(ui);
            });
        });
}

/// U5: badge de tecla para la pantalla de ayuda (chip redondeado).
fn key_badge(ui: &mut egui::Ui, text: &str) {
    egui::Frame::none()
        .fill(theme::BG_WIDGET_HOVER)
        .rounding(egui::Rounding::same(5.0))
        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
        .stroke(egui::Stroke::new(1.0, theme::STROKE))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .monospace()
                    .color(theme::ACCENT)
                    .size(13.0),
            );
        });
}

/// Abre una URL en el navegador por defecto de Windows.
///
/// `cmd /C start "" {url}` con `CREATE_NO_WINDOW` para no flashear consola.
fn open_url_in_browser(url: &str) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let result = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();

    if let Err(e) = result {
        tracing::warn!("No se pudo abrir {url}: {e}");
    }
}
