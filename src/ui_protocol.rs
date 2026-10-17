//! Protocolo de comunicacion logica <-> UI.
//!
//! Modulo cross-platform (la lib lo compila en Linux para tests): las
//! estructuras de datos y comandos no dependen de egui/Windows.
//! El render vive en `crate::ui` (windows-only).

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
    /// Pausar/resumir los atajos (toast persistente "Pausado").
    SetPaused(bool),
    /// Aviso de nueva version (pestaña Resultado + enlace GitHub).
    ShowUpdate {
        version: String,
        url: String,
        body: String,
    },
    /// Cerrar todas las ventanas.
    CloseAll,
}

/// Canal de comunicacion entre la logica de negocio y el UI thread.
pub struct UiChannels {
    /// Enviar comandos al UI (desde hotkeys/logica).
    pub tx: std::sync::mpsc::Sender<UiCommand>,
    /// Recibir comandos en el UI.
    pub rx: std::sync::mpsc::Receiver<UiCommand>,
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
