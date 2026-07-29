use thiserror::Error;

/// Errores tipados de la aplicacion quick-actions.
///
/// Se usa `thiserror` para derivar `Display` y compatibilidad con ` anyhow::Error`.
#[derive(Debug, Error)]
pub enum AppError {
    /// Error al interactuar con el portapapeles.
    #[error("Clipboard error: {0}")]
    Clipboard(String),

    /// Error al comunicarse con Ollama.
    #[error("Ollama API error: {0}")]
    Ollama(String),

    /// Error al leer o escribir configuracion.
    #[error("Config error: {0}")]
    Config(String),

    /// Error al serializar o deserializar JSON.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Error de red (HTTP).
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Error al simular pulsaciones de teclado.
    #[error("Keyboard simulation error: {0}")]
    Keyboard(String),

    /// Error de E/S general.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Error generico no categorizado.
    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// Crea un error generico a partir de un mensaje.
    pub fn msg<S: Into<String>>(msg: S) -> Self {
        Self::Other(msg.into())
    }
}

/// Tipo resultado estandar de la aplicacion.
pub type Result<T> = std::result::Result<T, AppError>;
