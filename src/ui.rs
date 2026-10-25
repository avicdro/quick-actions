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
    /// U7: tag de la release pendiente de aplicar (boton "Actualizar ahora").
    update_tag: Option<String>,

    /// T1: paleta aplicada actualmente (None = dark por defecto de egui);
    /// el re-apply reactivo de tema compara contra esto cada frame.
    applied: Option<&'static theme::Palette>,
    /// V6: cap del arranque en tool-window, aplicado con reintento por
    /// frame (el titulo del root existe recien al final del frame 1; el
    /// primer intento no encuentra el HWND y no-op).
    boot_tool_done: bool,
}

impl QuickActionsApp {
    /// Crea una nueva instancia.
    pub fn new(
        ui_rx: Receiver<UiCommand>,
        hotkey_rx: Receiver<HotkeyCommand>,
        state: Arc<AppState>,
        show_on_start: bool,
    ) -> Self {
        Self {
            ui_rx,
            hotkey_rx,
            state,
            applied: None,
            boot_tool_done: false,
            result_data: None,
            result_original: String::new(),
            result_resultado: String::new(),
            result_copied: false,
            history_data: None,
            status: String::new(),
            current_tab: Tab::Result,
            // QA (--show): abrir la ventana en el arranque (como un clic
            // del tray, sin interaccion).
            window_open: show_on_start,
            was_visible: false,
            egui_clipboard: None,
            toast_visible: false,
            toast_was_visible: false,
            toast_is_error: false,
            paused: false,
            toast_expiry: None,
            update_url: None,
            update_tag: None,
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
                    self.update_tag = None;
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
                    // U7: tag para el boton "Actualizar ahora".
                    self.update_tag = Some(version.clone());
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
                egui::TextEdit::multiline(&mut self.result_original.as_str())
                    .margin(egui::Margin::symmetric(10.0, 8.0))
                    .interactive(false),
            );
            ui.add(egui::Separator::default().horizontal());
            let url_click = url.clone();
            ui.horizontal(|ui| {
                if ui.button("Descargar en el navegador ↗").clicked() {
                    open_url_in_browser(&url_click);
                }
                // U7: update de un click (descarga + verificar SHA-256 +
                // swap del binario + reinicio de la app).
                if let Some(tag) = self.update_tag.clone() {
                    if ui.button("Actualizar ahora").clicked() {
                        let repo = self
                            .state
                            .config_snapshot()
                            .map(|c| c.app.repository)
                            .unwrap_or_default();
                        if repo.is_empty() {
                            let _ = self.state.ui_tx.send(UiCommand::SetStatus(
                                "Error: sin repositorio configurado (Ajustes)".to_string(),
                            ));
                            self.state.repaint();
                        } else {
                            let ui_tx = self.state.ui_tx.clone();
                            let repaint = self.state.repaint.clone();
                            let _ = self.state.ui_tx.send(UiCommand::SetStatus(
                                "Descargando actualizacion...".to_string(),
                            ));
                            self.state.repaint();
                            // Worker: el swap/reinicio hace todo el trabajo.
                            let tag2 = tag.clone();
                            std::thread::spawn(move || {
                                crate::self_update::apply_update(&repo, &tag2, ui_tx, repaint);
                            });
                        }
                    }
                }
            });
            ui.label(
                egui::RichText::new("Descarga el exe, verifica su SHA-256 y reinicia la app solo.")
                    .small()
                    .color(egui::Color32::GRAY),
            );
            return;
        }

        ui.label(egui::RichText::new("Texto original:").color(egui::Color32::GRAY));
        ui.add_sized(
            [ui.available_width(), 120.0],
            egui::TextEdit::multiline(&mut self.result_original.as_str())
                .margin(egui::Margin::symmetric(10.0, 8.0))
                .interactive(false),
        );

        ui.add(egui::Separator::default().horizontal());

        ui.label(
            egui::RichText::new(&data.titulo)
                .color(theme::palette().accent) // L8: antes navy-sobre-navy, invisible
                .strong(),
        );
        ui.add_sized(
            [ui.available_width(), ui.available_height() - 60.0],
            egui::TextEdit::multiline(&mut self.result_resultado.as_str())
                .margin(egui::Margin::symmetric(10.0, 8.0))
                .interactive(false),
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
                    .color(theme::palette().text_dim),
                );
            }
            Some(ref entries) if entries.is_empty() => {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("El historial esta vacio.")
                        .color(theme::palette().text_dim),
                );
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
                    .color(theme::palette().text_dim),
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
                .color(theme::palette().text_dim),
            );
            if toggle_note {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(
                        "⚠ CapsLock esta en modo toggle: un tap lo latchea y la\n  \
                         proxima tecla de atajo dispara (no alterna mayusculas).",
                    )
                    .color(theme::palette().accent),
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

                // ================================================== Apariencia
                // T1: tema oscuro / claro / sistema (live re-apply).
                settings_card(ui, "Apariencia", |ui| {
                    egui::Grid::new("grid_apariencia")
                        .num_columns(2)
                        .spacing([16.0, 11.0])
                        .show(ui, |ui| {
                            ui.label("Tema:");
                            let mut choice = cfg.ui.theme_choice();
                            egui::ComboBox::from_id_salt("combo_tema")
                                .selected_text(match choice {
                                    crate::config::ThemeChoice::Dark => "Oscuro",
                                    crate::config::ThemeChoice::Light => "Claro",
                                    crate::config::ThemeChoice::System => "Sistema (Windows)",
                                })
                                .width(170.0)
                                .show_ui(ui, |ui| {
                                    for (opt, label) in [
                                        (crate::config::ThemeChoice::Dark, "Oscuro"),
                                        (crate::config::ThemeChoice::Light, "Claro"),
                                        (crate::config::ThemeChoice::System, "Sistema (Windows)"),
                                    ] {
                                        if ui.selectable_value(&mut choice, opt, label).clicked() {
                                            // Persistir + replicar en el RwLock:
                                            // el re-apply reactivo del update()
                                            // pinta la paleta nueva al frame
                                            // siguiente, sin reinicio.
                                            let _ = crate::config::Config::write_config_key(
                                                &self.state.config_path,
                                                "ui",
                                                "theme",
                                                opt.as_config_str(),
                                            );
                                            if let Ok(mut c) = self.state.config.write() {
                                                c.ui.theme = opt.as_config_str().to_string();
                                            }
                                        }
                                    }
                                });
                            ui.end_row();
                        });
                    ui.label(
                        egui::RichText::new(
                            "\"Sistema\" sigue el tema claro/oscuro de Windows al instante.",
                        )
                        .color(theme::palette().text_dim),
                    );
                });

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
                                                .margin(egui::Margin::symmetric(8.0, 6.0))
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
                                        .margin(egui::Margin::symmetric(8.0, 6.0))
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
                                        .color(theme::palette().text_dim),
                                );
                            });
                            ui.end_row();
                        });
                });

                // ================================================== Hotkeys
                settings_card(ui, "Atajos", |ui| {
                    // L8: filas alineadas en grid (label a la izquierda con
                    // dos puntos, como la card Ollama — antes el label del
                    // combo quedaba a la derecha y floto sin columna).
                    let modifier_applied = self.state.hook_modifier.name().to_lowercase();
                    let mut modifier_changed = false;
                    let mut caps_changed = false;
                    let mut caps_toggle = false;
                    egui::Grid::new("grid_hotkeys_cfg")
                        .num_columns(2)
                        .spacing([16.0, 11.0])
                        .show(ui, |ui| {
                            ui.label("Modificador:");
                            let combo_mod = egui::ComboBox::from_id_salt("combo_modificador")
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
                            let _ = combo_mod;
                            ui.end_row();

                            // Capsmode: derivar bool <-> "toggle"/"hold"; solo con capslock.
                            caps_toggle = cfg.hotkeys.capsmode == "toggle";
                            ui.label("CapsLock:");
                            let caps_checked = ui.add_enabled(
                                cfg.hotkeys.modifier == "capslock",
                                egui::Checkbox::new(
                                    &mut caps_toggle,
                                    "modo toggle (tap latched; ya no alterna mayusculas)",
                                ),
                            );
                            if caps_checked.changed() {
                                caps_changed = true;
                            }
                            ui.end_row();
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

                    let capsmode_applied = match self.state.hook_capsmode {
                        crate::hotkeys::CapsMode::Toggle => "toggle",
                        crate::hotkeys::CapsMode::Hold => "hold",
                    };
                    if caps_changed {
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
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Habilitar individualmente:")
                            .color(theme::palette().text_dim),
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
                                        .margin(egui::Margin::symmetric(8.0, 6.0))
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
                                    .color(theme::palette().text_dim),
                            );
                            ui.end_row();
                        });

                    // B1: version exacta del build corriendo (tag + commit
                    // corto de git) — saber de un vistazo si uno lleva el
                    // fix de turno (p.ej. el caso NVIDIA).
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("Versión: {}", crate::version_string()))
                            .small()
                            .color(theme::palette().text_dim),
                    );

                    // U6: control manual del check (sin polling desde v0.2:
                    // 1 unica llamada al arrancar; de aca en mas, el usuario
                    // manda). Feedback garantizado por toast.
                    ui.add_space(8.0);
                    if ui.button("Buscar actualizaciones").clicked() {
                        let _ = self.state.ui_tx.send(UiCommand::SetStatus(
                            "Buscando actualizaciones...".to_string(),
                        ));
                        self.state.repaint();
                        let repo = cfg.app.repository.clone();
                        let ui_tx = self.state.ui_tx.clone();
                        // Check manual en background con feedback SIEMPRE
                        // (aviso / ultima version / error) — no bloquea UI.
                        std::thread::spawn(move || {
                            crate::update_check::run_manual_check(&repo, ui_tx);
                        });
                    }
                    if cfg.app.repository.is_empty() {
                        ui.label(
                            egui::RichText::new("Setea primero un repositorio")
                                .small()
                                .color(theme::palette().text_dim),
                        );
                    }
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

        // T1: tema — paleta por eleccion de config (dark/light/system);
        // re-aplicar SOLO si la paleta efectiva cambia (combo de Ajustes
        // on-the-fly o tema de Windows en modo sistema). Cada frame el
        // check es trivial (dos comparaciones).
        let choice = self
            .state
            .config
            .read()
            .ok()
            .map(|c| c.ui.theme_choice())
            .unwrap_or(crate::config::ThemeChoice::Dark);
        let sys_light = ctx.system_theme() == Some(egui::Theme::Light);
        let palette = theme::palette_for(choice, sys_light);
        if self.applied != Some(palette) {
            apply_theme(ctx, palette);
            self.applied = Some(palette);
            tracing::info!("T1 tema aplicado: {choice:?} (sistema claro={sys_light}).");
        }

        // V5/V6: cap del arranque en tool window (solo-tray). Reintento por
        // frame hasta que pega: eframe muestra el root tras el primer frame
        // (anti-flash, egui PR2279) y el titulo 'QuickActions' tambien lo
        // aplica egui al final de ese frame — en el frame 1 el HWND no es
        // localizable y el intento no-op deja el boton de taskbar.
        // (Con --show el open-block de abajo lo desactiva en el mismo frame.)
        if !self.window_open && !self.boot_tool_done {
            self.boot_tool_done = set_toolwindow(true);
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
                self.boot_tool_done = set_toolwindow(false); // V4-fix: ventana normal
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(900.0, 700.0)));
                let monitor = ctx
                    .input(|i| i.viewport().monitor_size)
                    .unwrap_or(egui::vec2(1920.0, 1080.0));
                let pos = egui::pos2((monitor.x - 900.0) / 2.0, (monitor.y - 700.0) / 2.0);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                // V6: subirla por encima de todo (juego fullscreen, apps)
                // — egui Focus solo puede fallar con foreground-lock y la
                // ventana quedaba detras, "invisible" para el usuario.
                present_main_window();
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
                    // L9: tab activa = SOLO borde ambar, sin relleno (feedback
                    // usuario: el chip opaco se veia pesado). El scope-style
                    // de este hijo neutraliza el fill de seleccion solo para
                    // estas tabs (la seleccion de texto global queda intacta).
                    ui.style_mut().visuals.selection.bg_fill = egui::Color32::TRANSPARENT;
                    ui.style_mut().visuals.selection.stroke =
                        egui::Stroke::new(1.5, theme::palette().accent);
                    let tabs = [
                        (Tab::Result, "Resultado"),
                        (Tab::History, "Historial"),
                        (Tab::Help, "Ayuda"),
                        (Tab::Ajustes, "Ajustes"),
                    ];
                    for (tab, label) in tabs {
                        let selected = self.current_tab == tab;
                        // L9: texto ambar si activa, dim lo demas (con borde
                        // solo, el relleno oscuro ya no pide texto oscuro).
                        let mut text = egui::RichText::new(label).size(15.0).color(if selected {
                            theme::palette().accent
                        } else {
                            theme::palette().text_dim
                        });
                        if selected {
                            text = text.strong();
                        }
                        let resp = ui.selectable_label(selected, text);
                        if selected {
                            // La parrilla del chip activo (borde redondeado).
                            ui.painter().rect_stroke(
                                resp.rect.expand(3.0),
                                8.0,
                                egui::Stroke::new(1.5, theme::palette().accent),
                            );
                        }
                        if resp.clicked() {
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
                        theme::palette().bg_elev
                    };

                    let border = if is_error {
                        theme::palette().error
                    } else {
                        theme::palette().accent
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
                                    theme::palette().text
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

        // Repaint: con toast animado, 30ms; sin toast, SOLO el keepalive de
        // 5000ms para que winit siga con frames vivos (V5). El 30ms con
        // ventana abierta de antes obligaba un render-loop continuo (~vsync)
        // con la GUI visible: ~140k lineas de wgpu `Device::maintain` al dia
        // en el log y la dGPU despierta a proposito. Sin toast, egui repinta
        // solo cuando hay input/eventos (reactivo): GUI abierta + idle = 0
        // frames, y el cursor/blink de los TextEdit pide su propio repaint.
        let repaint_ms = if self.toast_visible { 30 } else { 5000 };
        ctx.request_repaint_after(std::time::Duration::from_millis(repaint_ms));
    }
}

/// U5 + T1: paletas del app — DARK (navy + ambar, familia del icono del
/// tray) y LIGHT (papel calido + tinta navy, mismo acento ambar en tono
/// oscuro para contraste). `palette()` consulta la activa global (atomico)
/// desde cualquier sitio de dibujo; `apply_theme` la aplica al estilo egui.
pub mod theme {
    use egui::Color32;
    use std::sync::atomic::{AtomicU8, Ordering};

    use crate::config::ThemeChoice;

    /// Paleta nominal: superficies, texto, acento y derivados de estado.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Palette {
        /// Fondo principal de la ventana.
        pub bg_panel: Color32,
        /// Fondo de panales/areas elevadas (toast, popups).
        pub bg_elev: Color32,
        /// Fondo extreme (inputs).
        pub bg_input: Color32,
        /// Fondo de widgets (botones/checkbox base).
        pub bg_widget: Color32,
        /// Fondo de widget con hover.
        pub bg_widget_hover: Color32,
        /// V1: fondo de card (secciones de Ajustes) — un escalon entre elev y panel.
        pub card: Color32,
        /// Acento ambar de la marca: EN CLARO va en tono oscuro (contraste
        /// como texto/stroke; los badges con fondo ambar llevan tinta).
        pub accent: Color32,
        /// Texto primario (tinta navy en claro).
        pub text: Color32,
        /// Texto secundario contrastado (V1: sube el dim antiguo).
        pub text_dim: Color32,
        /// Linea/borde sutil.
        pub stroke: Color32,
        /// Error (icono/fondo).
        pub error: Color32,
        /// L8: fondo de seleccion de texto opaco (el stroke de la
        /// seleccion pinta el texto del item con `text`).
        pub selection_bg: Color32,
        /// L8: fondo de widget activo (boton pulsado / item seleccionado).
        pub active_bg: Color32,
        /// Alpha de la sombra grande de ventana.
        pub window_shadow_alpha: u8,
        /// Alpha de la sombra de popups/menus (combos).
        pub popup_shadow_alpha: u8,
    }

    /// Dark (U5/L8): navy profundo + ambar #FFC93C.
    pub const DARK: Palette = Palette {
        bg_panel: Color32::from_rgb(13, 19, 33),
        bg_elev: Color32::from_rgb(19, 28, 48),
        bg_input: Color32::from_rgb(10, 16, 28),
        bg_widget: Color32::from_rgb(22, 31, 53),
        bg_widget_hover: Color32::from_rgb(29, 43, 78),
        card: Color32::from_rgb(18, 27, 46),
        accent: Color32::from_rgb(255, 201, 60),
        text: Color32::from_rgb(232, 230, 227),
        text_dim: Color32::from_rgb(186, 195, 210),
        stroke: Color32::from_rgb(58, 69, 96),
        error: Color32::from_rgb(200, 80, 80),
        selection_bg: Color32::from_rgb(84, 64, 22),
        active_bg: Color32::from_rgb(42, 62, 110),
        window_shadow_alpha: 140,
        popup_shadow_alpha: 150,
    };

    /// Light (T1): papel calido + tinta navy, ambar oscuro para
    /// titulos/strokes (el ambar #FFC93C como TEXTO era ilegible en claro).
    pub const LIGHT: Palette = Palette {
        bg_panel: Color32::from_rgb(237, 234, 226),
        bg_elev: Color32::from_rgb(250, 248, 243),
        bg_input: Color32::from_rgb(255, 255, 255),
        bg_widget: Color32::from_rgb(228, 223, 212),
        bg_widget_hover: Color32::from_rgb(219, 212, 198),
        card: Color32::from_rgb(244, 241, 234),
        accent: Color32::from_rgb(158, 103, 8),
        text: Color32::from_rgb(26, 33, 44),
        text_dim: Color32::from_rgb(95, 106, 124),
        stroke: Color32::from_rgb(201, 196, 184),
        error: Color32::from_rgb(168, 52, 52),
        selection_bg: Color32::from_rgb(244, 214, 122),
        active_bg: Color32::from_rgb(213, 224, 243),
        window_shadow_alpha: 45,
        popup_shadow_alpha: 55,
    };

    /// 0 = dark, 1 = light (solo lo toca `set_active`; el dibujo lee).
    static ACTIVE_IS_LIGHT: AtomicU8 = AtomicU8::new(0);

    /// Paleta activa para DIBUJAR (antes de que `apply_theme` la aplique:
    /// DARK — y de fallback para cualquier frame previo).
    pub fn palette() -> &'static Palette {
        if is_light() {
            &LIGHT
        } else {
            &DARK
        }
    }

    /// T1: paleta por eleccion de config + tema del sistema.
    pub fn palette_for(choice: ThemeChoice, system_is_light: bool) -> &'static Palette {
        match choice {
            ThemeChoice::Dark => &DARK,
            ThemeChoice::Light => &LIGHT,
            ThemeChoice::System => {
                if system_is_light {
                    &LIGHT
                } else {
                    &DARK
                }
            }
        }
    }

    /// La paleta activa es la clara? (para el baseline egui en apply_theme).
    pub fn is_light() -> bool {
        ACTIVE_IS_LIGHT.load(Ordering::Relaxed) == 1
    }

    /// Fija la paleta activa (lo llama `apply_theme` ANTES del style_mut:
    /// todo frame pinta y consulta coherentes).
    pub fn set_active(p: &'static Palette) {
        ACTIVE_IS_LIGHT.store(
            if std::ptr::eq(p, &LIGHT) { 1 } else { 0 },
            Ordering::Relaxed,
        );
    }
}

/// V4-fix: HWND de la ventana principal.
///
/// Fast path: FindWindowW por titulo exacto ("Quick Actions", fijado al
/// abrir). Fallback para el arranque (titulo "QuickActions" del app_name):
/// EnumWindows + el titular EXACTO — sin filtro de visibilidad, porque la
/// ventana arranca oculta y eframe la revela tras el primer frame
/// (egui PR2279), DESPUES de cuando aplicamos el toolwindow (V5); por eso
/// IsWindowVisible fallaba en el boot y quedaba el boton en la taskbar.
/// El tray ("QuickActionsTray") y el toast (titulo vacio) no matchean.
#[cfg(windows)]
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
#[cfg(windows)]
extern "system" fn enum_quick_hwnd(hwnd: windows::Win32::Foundation::HWND, out: LPARAM) -> BOOL {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowTextW, GetWindowThreadProcessId,
    };

    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != std::process::id() {
            return BOOL(1); // seguir enumerando
        }
        // Clase del tray excluida explicitamente (su titulo empieza igual).
        let mut cls = [0u16; 64];
        let nc = GetClassNameW(hwnd, &mut cls);
        let class = String::from_utf16_lossy(&cls[..nc.max(0) as usize]);
        if class == "QuickActionsTrayWnd" {
            return BOOL(1);
        }

        let mut buf = [0u16; 64];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        if title == "QuickActions" || title == "Quick Actions" {
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

/// V4-fix/V6: alterna WS_EX_TOOLWINDOW en la ventana principal.
///
/// Tool window = sin boton en taskbar ni Alt-Tab aunque el HWND siga
/// tecnicamente visible (el truco de egui de ventana offscreen). Con esto
/// el "cerrar = hide" funciona sin el bug de Visible(false) de egui.
///
/// Devuelve `true` si encontro el HWND y dejo el estilo aplicado. En el
/// arranque puede fallar: el titulo del root lo aplica egui al FINAL del
/// primer frame, de modo que en el frame 1 el EnumWindows-match no hay
/// nada que buscar — el llamador reintenta por frame mientras sea false.
#[cfg(windows)]
fn set_toolwindow(tool: bool) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
        SW_SHOWNOACTIVATE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };

    let Some(hwnd) = main_hwnd() else {
        return false;
    };

    unsafe {
        // GWL_EXSTYLE es WINDOW_LONG_PTR_INDEX (0.58 devuelve isize).
        //
        // IMPORTANTE (dump de QA): winit crea la ventana con
        // WS_EX_APPWINDOW (0x40000) — con APPWINDOW presente la taskbar
        // MUESTRA el boton aunque tambien este TOOLWINDOW. Regla canonica:
        // tool = set TOOLWINDOW + clear APPWINDOW; normal = inverso.
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let was_tool = ex & (WS_EX_TOOLWINDOW.0 as isize) != 0;
        let desired = if tool {
            (ex | (WS_EX_TOOLWINDOW.0 as isize)) & !(WS_EX_APPWINDOW.0 as isize)
        } else {
            (ex | (WS_EX_APPWINDOW.0 as isize)) & !(WS_EX_TOOLWINDOW.0 as isize)
        };
        if was_tool == tool {
            let hwnd_addr = hwnd.0 as usize;
            tracing::debug!("set_toolwindow({tool}): ya estaba (hwnd=0x{hwnd_addr:X})");
            return true;
        }
        // Cambiar ex-style con la ventana VISIBLE no actualiza la taskbar:
        // hay que ocultar → estilo → volver a mostrar (canonico Win32).
        if tool {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired);
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
        // Reaparecer tras el hide de arriba (offscreen 1x1: el usuario no
        // nota ni el parpadeo; la taskbar si solto su boton).
        if tool {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        // QA: trazar el ex-style real tras aplicar (la taskbar puede
        // re-estilizar al set_visible del eframe tras el primer frame).
        let ex_now = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let tool_now = ex_now & (WS_EX_TOOLWINDOW.0 as isize) != 0;
        let stuck = tool_now == tool;
        let hwnd_addr = hwnd.0 as usize;
        if !stuck {
            tracing::warn!(
                "set_toolwindow({tool}) NO pego: ex deseo=0x{ex:X} ex real=0x{ex_now:X} (hwnd=0x{hwnd_addr:X})"
            );
        } else {
            tracing::info!("set_toolwindow({tool}): ok (ex=0x{ex_now:X}, hwnd=0x{hwnd_addr:X})");
        }

        stuck
    }
}

/// Presenta la ventana principal por encima de todo lo visible.
///
/// Contexto (bug del usuario en fullscreen): egui `Focus` no puede
/// activarnos desde un proceso de fondo (foreground lock: el clic del tray
/// lo consumio Explorer, no nosotros), asi que la ventana quedaba DETRAS de
/// un juego en fullscreen y se percibia "no aparece". El poke topmost→no
/// topmost la sube al tope del z-order sin activar, y el
/// SetForegroundWindow best-effort le roba el foco cuando Windows lo
/// permite (o simplemente queda visible y clickeable).
#[cfg(windows)]
fn present_main_window() {
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_NOTOPMOST,
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
        SW_SHOWNOACTIVATE,
    };

    let Some(hwnd) = main_hwnd() else { return };

    unsafe {
        // Restaurar si acaso estaba minimizada; si no, no-op seguro.
        if windows::Win32::UI::WindowsAndMessaging::IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        // Poke de z-order: TOPMOST una pasada + NOTOPMOST → queda sobre
        // ventanas normales (incluido un juego fullscreen-borderless) sin
        // quedarse clavada always-on-top para siempre.
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
        let _ = SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd); // best-effort (puede ser denegado)
    }
}

#[cfg(not(windows))]
fn present_main_window() {}

#[cfg(not(windows))]
fn set_toolwindow(_tool: bool) -> bool {
    false
}

/// U5 + V1 + T1: aplica la paleta al estilo egui — baseline dark/light
/// acorde (los valores que NO se pisan abajo heredan de un base sane:
/// separadores, submenus, drag...) y luego los overrides de la paleta.
/// Se invoca solo AL CAMBIAR de paleta (config on-the-fly o tema del
/// sistema); el coste es trivial comparado con un frame.
fn apply_theme(ctx: &egui::Context, p: &'static theme::Palette) {
    use egui::{Color32, FontId, Shadow, Stroke, Vec2};

    // La paleta activa ANTES de tocar estilos: cualquier dibujo del frame
    // (incluido el toast viewport) consulta `theme::palette()` coherente.
    theme::set_active(p);
    if theme::is_light() {
        ctx.set_theme(egui::Theme::Light); // baseline claro de egui
    } else {
        ctx.set_theme(egui::Theme::Dark);
    }

    ctx.style_mut(|style| {
        let v = &mut style.visuals;
        v.panel_fill = p.bg_panel;
        v.window_fill = p.bg_elev;
        v.extreme_bg_color = p.bg_input;
        v.window_stroke = Stroke::new(1.0, p.stroke);
        v.window_shadow = Shadow {
            offset: Vec2::new(0.0, 5.0),
            blur: 22.0,
            spread: 2.0,
            color: Color32::from_black_alpha(p.window_shadow_alpha),
        };
        // L8: seleccion de texto legible en TODO (TextEdit y etiquetas):
        // fondo ambar opaco + texto de la paleta (egui 0.29: el texto del
        // item seleccionado usa selection.stroke.color; el translucido
        // amarillo anterior tapaba las letras y los items iban ambar-sobre-
        // ambar).
        v.selection.bg_fill = p.selection_bg;
        v.selection.stroke = Stroke::new(1.0, p.text);

        // Rounding global (L8: radios 12 en superficies, 8 en widgets).
        v.window_rounding = 12.0.into();
        v.menu_rounding = 10.0.into();
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.rounding = egui::Rounding::same(8.0);
        }

        // L8: sombra de popups/menus (combos) — offset + blur real.
        v.popup_shadow = Shadow {
            offset: Vec2::new(0.0, 6.0),
            blur: 18.0,
            spread: 1.0,
            color: Color32::from_black_alpha(p.popup_shadow_alpha),
        };
        // V1 a11y: checkmarks e iconos bien visibles (texto claro, acento en
        // hover/activo); los strokes mas gruesos para baja vision.
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.6, p.text_dim);
        v.widgets.inactive.fg_stroke = Stroke::new(1.6, p.text);
        v.widgets.open.fg_stroke = Stroke::new(1.6, p.text);
        v.widgets.hovered.fg_stroke = Stroke::new(1.6, p.accent);
        v.widgets.active.fg_stroke = Stroke::new(1.6, p.accent);
        v.widgets.inactive.bg_fill = p.bg_widget;
        v.widgets.inactive.weak_bg_fill = p.bg_widget;
        v.widgets.hovered.bg_fill = p.bg_widget_hover;
        v.widgets.hovered.weak_bg_fill = p.bg_widget_hover;
        v.widgets.active.bg_fill = p.active_bg;
        v.widgets.active.weak_bg_fill = p.active_bg;
        v.widgets.hovered.bg_stroke = Stroke::new(1.2, p.stroke);
        v.widgets.active.bg_stroke = Stroke::new(1.2, p.accent);

        // Texto.
        v.override_text_color = Some(p.text);
        // Fondo de text edits etc.
        v.extreme_bg_color = p.bg_input;

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
    // L8: ritmo de secciones — mas aire ARRIBA de la card (heading rule:
    // mas espacio arriba que abajo) y ancho completo uniforme (antes cada
    // card floto al tamano de su contenido, con anchos desparejos).
    ui.add_space(12.0);
    egui::Frame::none()
        .fill(theme::palette().card)
        .rounding(egui::Rounding::same(12.0))
        .stroke(egui::Stroke::new(1.0, theme::palette().stroke))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            // Ancho completo: la card ocupa todo el panel disponible.
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .strong()
                        .size(16.5)
                        .color(theme::palette().accent),
                );
                ui.add_space(6.0);
                body(ui);
            });
        });
}

/// U5: badge de tecla para la pantalla de ayuda (chip redondeado).
fn key_badge(ui: &mut egui::Ui, text: &str) {
    egui::Frame::none()
        .fill(theme::palette().bg_widget_hover)
        .rounding(egui::Rounding::same(5.0))
        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
        .stroke(egui::Stroke::new(1.0, theme::palette().stroke))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .monospace()
                    .color(theme::palette().accent)
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
