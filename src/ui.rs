use eframe::egui;
use std::sync::mpsc::Receiver;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

use crate::actions;
use crate::clipboard::ClipboardManager;
use crate::config::Config;
use crate::history::History;
use crate::hotkeys::HotkeyCommand;

/// Contexto global de egui para que otros threads (hook, workers)
/// puedan despertar el event loop del UI sin polling constante.
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

/// Guarda el contexto de egui. Debe llamarse una vez al iniciar el UI.
pub fn set_context(ctx: &egui::Context) {
    let _ = CONTEXT.set(ctx.clone());
}

/// Despierta el event loop del UI. Llamar desde cualquier thread.
pub fn request_repaint() {
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

/// Datos para mostrar una ventana de resultado.
#[derive(Debug, Clone)]
pub struct ResultData {
    /// Titulo de la ventana (ej: "Traduccion", "Resumen").
    pub titulo: String,
    /// Texto original (entrada del usuario).
    pub original: String,
    /// Resultado generado por Ollama.
    pub resultado: String,
}

/// Datos para mostrar el historial.
#[derive(Debug, Clone)]
pub struct HistoryData {
    /// Entradas del historial.
    pub entries: Vec<HistoryEntryView>,
    /// Numero seleccionado por el usuario (1-based, 0 = ninguno).
    pub selected: usize,
}

/// Representacion visual de una entrada de historial.
#[derive(Debug, Clone)]
pub struct HistoryEntryView {
    /// Indice visible (1-based).
    pub index: usize,
    /// Texto de preview (truncado).
    pub preview: String,
    /// Fecha formateada.
    pub fecha: String,
    /// Texto original completo (para copiar).
    pub original: String,
}

/// Canal de comunicacion entre la logica de negocio y el UI thread.
pub struct UiChannels {
    /// Enviar comandos al UI (desde hotkeys/logica).
    pub tx: std::sync::mpsc::Sender<UiCommand>,
    /// Recibir comandos en el UI.
    pub rx: Receiver<UiCommand>,
}

impl UiChannels {
    /// Crea un nuevo par de canales.
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self { tx, rx }
    }
}

impl Default for UiChannels {
    fn default() -> Self {
        Self::new()
    }
}

/// Comandos que el UI puede recibir.
#[derive(Debug, Clone)]
pub enum UiCommand {
    /// Mostrar ventana de resultado.
    ShowResult(ResultData),
    /// Mostrar ventana de historial.
    ShowHistory(HistoryData),
    /// Mostrar ventana de ayuda.
    ShowHelp(String),
    /// Actualizar el estado (loading/error). Cadena vacia = sin estado.
    SetStatus(String),
    /// Cerrar todas las ventanas.
    CloseAll,
}

/// Pestanas de la ventana unificada.
#[derive(Debug, Clone, PartialEq)]
enum Tab {
    Result,
    History,
    Help,
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
    /// Configuracion
    config: Arc<Config>,
    /// Clipboard compartido
    clipboard: Arc<ClipboardManager>,
    /// Historial compartido
    history: Arc<Mutex<History>>,
    /// Sender UI
    ui_tx: std::sync::mpsc::Sender<UiCommand>,

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
    /// Texto de ayuda.
    help_text: String,
    /// Estado actual (loading, error). Vacio = sin estado.
    status: String,

    // --- Estado de UI ---
    /// Pestana activa.
    current_tab: Tab,
    /// Si la ventana esta visible.
    window_open: bool,
    /// Estado visible anterior (para evitar enviar ViewportCommand innecesarios).
    was_visible: bool,
    /// Clipboard cacheado para egui (evita crear uno nuevo en cada frame).
    egui_clipboard: Option<arboard::Clipboard>,
}

impl QuickActionsApp {
    /// Crea una nueva instancia.
    pub fn new(
        ui_rx: Receiver<UiCommand>,
        hotkey_rx: Receiver<HotkeyCommand>,
        config: Arc<Config>,
        clipboard: Arc<ClipboardManager>,
        history: Arc<Mutex<History>>,
        ui_tx: std::sync::mpsc::Sender<UiCommand>,
    ) -> Self {
        Self {
            ui_rx,
            hotkey_rx,
            config,
            clipboard,
            history,
            ui_tx,
            result_data: None,
            result_original: String::new(),
            result_resultado: String::new(),
            result_copied: false,
            history_data: None,
            help_text: actions::help_text(),
            status: String::new(),
            current_tab: Tab::Result,
            window_open: false,
            was_visible: false,
            egui_clipboard: None,
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
                }
                UiCommand::ShowHistory(data) => {
                    self.history_data = Some(data);
                    self.current_tab = Tab::History;
                    self.window_open = true;
                }
                UiCommand::ShowHelp(text) => {
                    self.help_text = text;
                    self.current_tab = Tab::Help;
                    self.window_open = true;
                }
                UiCommand::SetStatus(msg) => {
                    // NO abrir la ventana aqui. El status solo es visible
                    // si la ventana ya esta abierta (por ShowResult, etc.).
                    // Esto es CRITICO: abrir la ventana roba el foco del editor
                    // del usuario, lo que rompe Ctrl+C (copy) y Ctrl+V (paste).
                    self.status = msg;
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
            let config = Arc::clone(&self.config);
            let clipboard = Arc::clone(&self.clipboard);
            let history = Arc::clone(&self.history);
            let ui_tx = self.ui_tx.clone();

            std::thread::spawn(move || {
                if let Err(e) = actions::handle_command(cmd, &config, &clipboard, &history, &ui_tx) {
                    tracing::error!("Error en handler: {e}");
                }
            });
        }
    }

    /// Dibuja la pestana de resultado.
    fn draw_result_tab(&mut self, ui: &mut egui::Ui) {
        if self.result_data.is_none() {
            ui.label("No hay resultado ainda. Selecciona texto y pulsa Alt+T/R/Enter.");
            return;
        }

        let Some(data) = self.result_data.as_ref() else {
            return;
        };

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
                ui.label(
                    egui::RichText::new("Copiado!")
                        .color(egui::Color32::GREEN),
                );
            }
        });
    }

    /// Dibuja la pestana de historial.
    fn draw_history_tab(&mut self, ui: &mut egui::Ui) {
        let entries = self.history_data.as_ref().map(|d| d.entries.clone());

        match entries {
            None => {
                ui.label("No hay historial. Pulsa Alt+V para cargarlo.");
            }
            Some(ref entries) if entries.is_empty() => {
                ui.label("El historial esta vacio.");
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
            if let Ok(hist) = self.history.lock() {
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

    /// Dibuja la pestana de ayuda.
    fn draw_help_tab(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(&self.help_text);
        });
    }
}

impl eframe::App for QuickActionsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Guardar contexto la primera vez.
        if CONTEXT.get().is_none() {
            set_context(ctx);
        }

        // Cancelar cierre de ventana (evita que eframe cierre la app).
        // En su lugar, ocultamos la ventana.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.window_open = false;
        }

        ctx.set_pixels_per_point(1.3);

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
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(900.0, 700.0)));
                let monitor = ctx
                    .input(|i| i.viewport().monitor_size)
                    .unwrap_or(egui::vec2(1920.0, 1080.0));
                let pos = egui::pos2((monitor.x - 900.0) / 2.0, (monitor.y - 700.0) / 2.0);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else {
                // Ocultar: 1x1 fuera de pantalla
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1.0, 1.0)));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(-10000.0, -10000.0)));
            }
            self.was_visible = self.window_open;
        }

        // Dibujar contenido solo si la ventana esta visible
        if self.window_open {
            egui::CentralPanel::default().show(ctx, |ui| {
                // Barra de pestanas (sin boton X custom, la ventana nativa ya tiene cerrar)
                ui.horizontal(|ui| {
                    let tabs = [(Tab::Result, "Resultado"), (Tab::History, "Historial"), (Tab::Help, "Ayuda")];
                    for (tab, label) in tabs {
                        if ui
                            .selectable_label(self.current_tab == tab, label)
                            .clicked()
                        {
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
                }

                // Barra de estado inferior (loading/error)
                if !self.status.is_empty() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        let is_error = self.status.starts_with("Error");
                        let color = if is_error {
                            egui::Color32::from_rgb(200, 50, 50) // Rojo sutil
                        } else {
                            egui::Color32::from_rgb(100, 160, 100) // Verde sutil
                        };
                        // Spinner animado si esta procesando
                        if !is_error {
                            ui.spinner();
                        }
                        ui.label(egui::RichText::new(&self.status).color(color));
                    });
                }
            });
        }

        // Repaint: rapido con ventana (30ms), lento sin ella (5000ms).
        let repaint_ms = if self.window_open { 30 } else { 5000 };
        ctx.request_repaint_after(std::time::Duration::from_millis(repaint_ms));
    }
}