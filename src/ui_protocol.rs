//! Protocolo de comunicacion logica <-> UI.
//!
//! Modulo cross-platform (la lib lo compila en Linux para tests): las
//! estructuras de datos y comandos no dependen de egui/Windows.
//! El render vive en `crate::ui` (windows-only).

/// Duracion (segundos) de un toast de tipo Notice (U1).
pub const NOTICE_TOAST_SECS: f64 = 3.5;
/// Errores: mas tiempo para poder leer el mensaje.
pub const ERROR_TOAST_SECS: f64 = 6.0;

/// Clase de toast y su comportamiento de autodesaparicion (U1).
///
/// El texto plano de `SetStatus` es el vehiculo, la clase deduce:
/// - `Loading`: mensaje termina en `...` (spinner, NO expira)
/// - `Error`: mensaje empieza con "Error" (expira en 6s)
/// - `Notice`: cualquier otro aviso (expira en 3.5s)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    /// Spinner mientras corre una peticion (sin expiry).
    Loading,
    /// Aviso transitorio (auto-oculta a los NOTICE_TOAST_SECS).
    Notice,
    /// Mensaje de error (auto-oculta a los ERROR_TOAST_SECS).
    Error,
}

impl ToastKind {
    /// Segundos hasta auto-ocultarse; `None` = visible hasta nuevo aviso.
    #[must_use]
    pub fn expiry_secs(self) -> Option<f64> {
        match self {
            Self::Loading => None,
            Self::Notice => Some(NOTICE_TOAST_SECS),
            Self::Error => Some(ERROR_TOAST_SECS),
        }
    }
}

/// Clasifica un mensaje de status en `ToastKind`.
///
/// Reglas simples y deterministas sobre el texto plano (los senders nunca
/// cambian su formato: los "Corrigiendo.../Traduciendo.../..." mantienen
/// los tres puntos de seria loading).
#[must_use]
pub fn classify_status(msg: &str) -> ToastKind {
    if msg.starts_with("Error") {
        ToastKind::Error
    } else if msg.trim_end().ends_with("...") {
        ToastKind::Loading
    } else {
        ToastKind::Notice
    }
}

#[cfg(test)]
mod toast_tests {
    use super::*;

    #[test]
    fn classify_statuses() {
        // Loading: suffix "..." (sin expiry)
        assert_eq!(classify_status("Corrigiendo..."), ToastKind::Loading);
        assert_eq!(classify_status("Traduciendo..."), ToastKind::Loading);
        // Error: prefijo "Error" gana",
        assert_eq!(classify_status("Error: x"), ToastKind::Error);
        assert_eq!(classify_status("Error..."), ToastKind::Error);
        // Notice: display de
        assert_eq!(classify_status("Modelo cambiado"), ToastKind::Notice);
        assert_eq!(classify_status("Auto-arranque activado"), ToastKind::Notice);
        assert_eq!(classify_status(""), ToastKind::Notice);
    }

    #[test]
    fn expiry_durations() {
        assert_eq!(ToastKind::Loading.expiry_secs(), None);
        assert_eq!(ToastKind::Notice.expiry_secs(), Some(NOTICE_TOAST_SECS));
        assert_eq!(ToastKind::Error.expiry_secs(), Some(ERROR_TOAST_SECS));
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
    /// Pausar/resumir los atajos (P2).
    SetPaused(bool),
    /// U4: abrir la ventana en la pestana Ajustes (menu del tray).
    ShowAjustes,
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
