use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

/// Configuracion de la aplicacion, cargada desde `config.toml`.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Config {
    /// Configuracion de Ollama.
    pub ollama: OllamaConfig,
    /// Configuracion de la interfaz de usuario.
    pub ui: UiConfig,
    /// Configuracion de hotkeys.
    pub hotkeys: HotkeysConfig,
    /// Rutas de aplicaciones externas.
    pub paths: PathsConfig,
    /// Configuracion futura del launcher.
    pub launcher: LauncherConfig,
    /// Configuracion futura de notificaciones.
    pub notifications: NotificationsConfig,
}

/// Configuracion del servicio Ollama local.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct OllamaConfig {
    /// Nombre del modelo (ej: "gemma4:cloud").
    pub model: String,
    /// URL del endpoint de generacion.
    pub endpoint: String,
    /// Timeout en segundos para las peticiones.
    pub timeout_seconds: u64,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            model: "gemma4:cloud".to_string(),
            endpoint: "http://localhost:11434/api/generate".to_string(),
            timeout_seconds: 60,
        }
    }
}

/// Configuracion visual de las ventanas.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct UiConfig {
    /// Tamano de fuente (px aproximado para egui).
    pub font_size: u32,
    /// Ancho por defecto de ventanas de resultado.
    pub window_width: u32,
    /// Alto por defecto de ventanas de resultado.
    pub window_height: u32,
    /// Maximo de entradas en el historial de correcciones.
    pub history_max_entries: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            font_size: 16,
            window_width: 800,
            window_height: 600,
            history_max_entries: 50,
        }
    }
}

/// Configuracion de los atajos de teclado.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct HotkeysConfig {
    /// Si los atajos globales estan activos.
    pub enabled: bool,
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Rutas a ejecutables externos.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct PathsConfig {
    /// Ruta al ejecutable de Chrome.
    pub chrome: String,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            chrome: "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe".to_string(),
        }
    }
}

/// Configuracion futura del launcher de apps.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct LauncherConfig {
    /// Si el launcher esta activo.
    #[allow(dead_code)]
    pub enabled: bool,
}

/// Configuracion futura del overlay de notificaciones.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct NotificationsConfig {
    /// Si las notificaciones estan activas.
    #[allow(dead_code)]
    pub enabled: bool,
}

impl Config {
    /// Carga la configuracion desde un archivo TOML.
    ///
    /// # Errors
    ///
    /// Devuelve `AppError::Config` si no puede leer o parsear el archivo.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| AppError::Config(format!("No se pudo leer config.toml: {e}")))?;

        // Usar serde con defaults para campos opcionales
        let config: Config = toml::from_str(&content)
            .map_err(|e| AppError::Config(format!("Error parseando config.toml: {e}")))?;
        Ok(config)
    }

    /// Carga la configuracion. Si no existe, crea una por defecto junto al exe.
    pub fn load_or_create_default() -> Result<Self> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));

        let local_config = exe_dir.join("config.toml");

        // Si existe, cargarlo
        if local_config.exists() {
            return Self::load(&local_config);
        }

        // Si no existe, crearlo con defaults
        let default_content = Self::default_toml();
        std::fs::write(&local_config, &default_content)
            .map_err(|e| AppError::Config(format!("No se pudo crear config.toml: {e}")))?;

        tracing::info!("Config.toml creado con defaults en: {:?}", local_config);

        Self::load(&local_config)
    }

    /// Devuelve el contenido TOML por defecto.
    fn default_toml() -> String {
        r#"# quick-actions configuration
# Creado automaticamente. Puedes editar este archivo y reiniciar la app.

[ollama]
model = "gemma4:cloud"
endpoint = "http://localhost:11434/api/generate"
timeout_seconds = 60

[ui]
font_size = 16
window_width = 800
window_height = 600
history_max_entries = 50

[hotkeys]
enabled = true

[paths]
chrome = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"

[launcher]
enabled = false

[notifications]
enabled = false
"#
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_values() {
        let c = Config::default();
        assert_eq!(c.ollama.model, "gemma4:cloud");
        assert_eq!(c.ui.font_size, 16);
        assert!(c.hotkeys.enabled);
    }

    #[test]
    fn config_parse_full_toml() {
        let toml = r#"
[ollama]
model = "qwen3"
endpoint = "http://localhost:11434/api/generate"
timeout_seconds = 120

[ui]
font_size = 20
window_width = 1024
window_height = 768
history_max_entries = 100

[hotkeys]
enabled = false

[paths]
chrome = "C:\\chrome.exe"

[launcher]
enabled = true

[notifications]
enabled = true
"#;
        let c: Config = toml::from_str(toml).unwrap();
        assert_eq!(c.ollama.model, "qwen3");
        assert_eq!(c.ollama.timeout_seconds, 120);
        assert_eq!(c.ui.font_size, 20);
        assert!(!c.hotkeys.enabled);
        assert!(c.launcher.enabled);
        assert!(c.notifications.enabled);
    }

    #[test]
    fn config_parse_partial_toml_uses_defaults() {
        // Solo [ollama], el resto usa defaults
        let toml = r#"
[ollama]
model = "test-model"
"#;
        let c: Config = toml::from_str(toml).unwrap();
        assert_eq!(c.ollama.model, "test-model");
        // Defaults del resto
        assert_eq!(c.ollama.endpoint, "http://localhost:11434/api/generate");
        assert_eq!(c.ui.font_size, 16);
        assert!(c.hotkeys.enabled);
    }
}