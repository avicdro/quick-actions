use arboard::Clipboard;
use std::sync::Mutex;

use crate::error::{AppError, Result};

/// Gestor del portapapeles con backup y restore.
///
/// Se usa un `Mutex` interno porque `arboard::Clipboard` no es `Sync`.
/// En la practica, todas las operaciones de clipboard se hacen desde
/// un unico thread de UI, asi que el lock nunca contiende.
pub struct ClipboardManager {
    clipboard: Mutex<Clipboard>,
    backup: Mutex<Option<String>>,
}

impl ClipboardManager {
    /// Crea un nuevo gestor de portapapeles.
    ///
    /// # Errors
    ///
    /// Devuelve error si no puede acceder al portapapeles del sistema.
    pub fn new() -> Result<Self> {
        let clipboard = Clipboard::new()
            .map_err(|e| AppError::Clipboard(format!("No se pudo acceder al portapapeles: {e}")))?;
        Ok(Self {
            clipboard: Mutex::new(clipboard),
            backup: Mutex::new(None),
        })
    }

    /// Guarda el contenido actual del portapapeles como backup.
    pub fn backup(&self) -> Result<()> {
        let text = self.get_text()?;
        let mut backup = self
            .backup
            .lock()
            .map_err(|_| AppError::Clipboard("Error de lock en backup".to_string()))?;
        *backup = Some(text);
        Ok(())
    }

    /// Restaura el portapapeles desde el backup.
    pub fn restore(&self) -> Result<()> {
        let backup = self
            .backup
            .lock()
            .map_err(|_| AppError::Clipboard("Error de lock en restore".to_string()))?;
        if let Some(ref text) = *backup {
            self.set_text(text)?;
        }
        Ok(())
    }

    /// Obtiene el texto actual del portapapeles.
    ///
    /// Si el portapapeles esta vacio o no tiene texto, devuelve `Ok("")`
    /// en lugar de error. Solo devuelve error para fallos reales.
    pub fn get_text(&self) -> Result<String> {
        let mut clipboard = self
            .clipboard
            .lock()
            .map_err(|_| AppError::Clipboard("Error de lock en get_text".to_string()))?;

        match clipboard.get_text() {
            Ok(text) => Ok(text),
            Err(e) => {
                let err_msg = format!("{e}");
                // Si el clipboard esta vacio o no tiene texto, no es un error
                if err_msg.contains("empty")
                    || err_msg.contains("not available")
                    || err_msg.contains("no content")
                {
                    Ok(String::new())
                } else {
                    Err(AppError::Clipboard(format!(
                        "Error leyendo portapapeles: {e}"
                    )))
                }
            }
        }
    }

    /// Establece el texto del portapapeles.
    pub fn set_text(&self, text: &str) -> Result<()> {
        let mut clipboard = self
            .clipboard
            .lock()
            .map_err(|_| AppError::Clipboard("Error de lock en set_text".to_string()))?;
        clipboard
            .set_text(text)
            .map_err(|e| AppError::Clipboard(format!("Error escribiendo portapapeles: {e}")))
    }

    /// Limpia el portapapeles (establece texto vacio).
    pub fn clear(&self) -> Result<()> {
        self.set_text("")
    }
}
