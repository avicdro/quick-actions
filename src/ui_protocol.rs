//! Protocolo de comunicacion logica <-> UI.
//!
//! Modulo cross-platform (la lib lo compila en Linux para tests): las
//! estructuras de datos y comandos no dependen de egui/Windows.
//! El render vive en `crate::ui` (windows-only).

use std::sync::Arc;

/// Duracion (segundos) de un toast de tipo Notice (U1).
pub const NOTICE_TOAST_SECS: f64 = 3.5;
/// Errores: mas tiempo para poder leer el mensaje.
pub const ERROR_TOAST_SECS: f64 = 6.0;

/// Callback para despertar el event loop del UI desde cualquier thread
/// (fase 1.3: reemplaza al static `CONTEXT` de `ui.rs`; el egui::Context
/// real se cablea en el primer frame via `AppState::set_ui_ctx` y antes
/// de eso el callback no hace nada).
pub type RepaintFn = Arc<dyn Fn() + Send + Sync>;

/// Clase de toast y su comportamiento de autodesaparicion (U1).
///
/// C6 (v0.5): la clase ya NO se adivina del texto (antes
/// `classify_status` deducia de `starts_with("Error")`/`ends_with("...")`
/// — cambiar un mensaje rompia spinner/expiry silenciosamente). Quien
/// envia el `StatusMsg` declara la clase; `ToastKind` queda para el
/// dibujo (icono) y el expiry.
/// - `Loading`: spinner, NO expira
/// - `Notice`: expira en 3.5s
/// - `Error`: expira en 6s
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

/// C6 (v0.5): mensaje de estado TIPADO — reemplaza al `SetStatus(String)`
/// por texto plano con convenciones magicas ("..." para loading, prefijo
/// "Error" para errores, "" para limpiar).
#[derive(Debug, Clone, PartialEq)]
pub enum StatusMsg {
    /// Oculta el toast (estado limpio).
    Clear,
    /// Peticion en curso: spinner, sin expiry.
    Loading(String),
    /// Aviso transitorio: auto-oculta a los `NOTICE_TOAST_SECS`.
    Notice(String),
    /// Error: auto-oculta a los `ERROR_TOAST_SECS`.
    Error(String),
}

impl StatusMsg {
    /// Texto visible del mensaje ("" en `Clear`).
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Clear => "",
            Self::Loading(t) | Self::Notice(t) | Self::Error(t) => t,
        }
    }

    /// Clase de toast asociada (`None` en `Clear`).
    #[must_use]
    pub fn kind(&self) -> Option<ToastKind> {
        match self {
            Self::Clear => None,
            Self::Loading(_) => Some(ToastKind::Loading),
            Self::Notice(_) => Some(ToastKind::Notice),
            Self::Error(_) => Some(ToastKind::Error),
        }
    }
}

#[cfg(test)]
mod toast_tests {
    use super::*;

    #[test]
    fn expiry_durations() {
        assert_eq!(ToastKind::Loading.expiry_secs(), None);
        assert_eq!(ToastKind::Notice.expiry_secs(), Some(NOTICE_TOAST_SECS));
        assert_eq!(ToastKind::Error.expiry_secs(), Some(ERROR_TOAST_SECS));
    }

    #[test]
    fn status_msg_texto_y_clase() {
        // C6: la clase es del TIPO, ya no se adivina del texto — un
        // "Error..." sigue siendo lo que su constructor declare.
        assert_eq!(StatusMsg::Clear.text(), "");
        assert_eq!(StatusMsg::Clear.kind(), None);
        assert_eq!(
            StatusMsg::Loading("Error...".into()).kind(),
            Some(ToastKind::Loading)
        );
        assert_eq!(
            StatusMsg::Error("Corrigiendo...".into()).kind(),
            Some(ToastKind::Error)
        );
        assert_eq!(
            StatusMsg::Notice("Modelo cambiado".into()).kind(),
            Some(ToastKind::Notice)
        );
        assert_eq!(StatusMsg::Notice("x".into()).text(), "x");
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

/// QW3 (v0.5): UNICO punto de construccion de vistas del historial
/// (antes el mapeo + truncado del preview estaba duplicado en
/// `actions.handle_show_history` y en el boton "Actualizar" de `ui.rs`).
///
/// Ademas CORRIGE el truncado: antes `&original[..60]` cortaba por BYTES
/// y paniqueaba si el byte 60 caia a mitad de caracter multi-byte
/// (acentos, emoji). Ahora trunca por chars, UTF-8 safe.
#[must_use]
pub fn history_views(entries: &[crate::history::HistoryEntry]) -> Vec<HistoryEntryView> {
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let preview: String = if e.original.chars().count() > 60 {
                let mut p: String = e.original.chars().take(60).collect();
                p.push_str("...");
                p
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
        .collect()
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
    /// Actualizar el estado (toast): C6 tipa el mensaje (clase declarada
    /// por quien envia); `None` en kind = limpiar (`Clear`).
    SetStatus(StatusMsg),
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
    /// C4 (v0.5): salir de la app de forma LIMPIA — el UI deja de
    /// interceptar el close, eframe termina y el `WorkerGuard` del log
    /// flusha al caer. Envia el tray al pedir "Salir" (ya no
    /// `std::process::exit`).
    Shutdown,
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
