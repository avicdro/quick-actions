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
    /// Configuracion de la app en general (auto-arranque, etc).
    pub app: AppConfig,
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
    /// Modificador de atajos: `"capslock"` (default) o `"alt"`.
    pub modifier: String,
    /// Comportamiento del modificador CapsLock: `"hold"` (default:
    /// mantener presionado para disparar, CapsLock nativo intacto) o
    /// `"toggle"` (tap queda latched; proxima tecla de atajo dispara y
    /// descapacita — CapsLock ya no alterna mayusculas nativamente).
    /// Solo aplica con modifier capslock.
    pub capsmode: String,
    /// U3, pausa modular: interruptor por atajo. `false` desactiva solo
    /// esa accion (la pausa total de P2, `enabled`, tiene prioridad).
    pub correct: bool,
    pub translate: bool,
    pub summarize: bool,
    pub generate: bool,
    pub history: bool,
    pub help: bool,
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            // Preferencia del usuario (v0.4/U2): Alt como modificador por
            // defecto; CapsLock+hold/toggle queda disponible por config.
            modifier: "alt".to_string(),
            capsmode: "hold".to_string(),
            // U3: todos los atajos habilitados por defecto.
            correct: true,
            translate: true,
            summarize: true,
            generate: true,
            history: true,
            help: true,
        }
    }
}

/// Configuracion general de la app.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct AppConfig {
    /// Iniciar automaticamente con Windows (entrada en
    /// `HKCU\...\CurrentVersion\Run`). No interfiere con toggles manuales
    /// desde el tray (ver docs/plans/improvements-v0.3.md, P1).
    pub autostart: bool,
    /// Verificar updates al arrancar (GET /releases/latest de GitHub).
    pub check_updates: bool,
    /// Repo a consultar para updates (`dueño/repo`). Vacio = sin check.
    pub repository: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            autostart: false,
            // Opt-out: por defecto SI chequea updates.
            check_updates: true,
            repository: "avicdro/quick-actions".to_string(),
        }
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
#[serde(default)]
pub struct LauncherConfig {
    /// Si el launcher esta activo.
    #[allow(dead_code)]
    pub enabled: bool,
}

/// Configuracion futura del overlay de notificaciones.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
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

#[hotkeys]
enabled = true
modifier = "alt"        # "alt" (default) o "capslock"
capsmode = "hold"       # solo capslock: "hold" (mantener) o "toggle" (latched)

# Pausa modular (U3): false desactiva un atajo concreto sin tocar el resto.
# La pausa total ([hotkeys] enabled o el menu del tray) tiene prioridad.
correct = true
translate = true
summarize = true
generate = true
history = true
help = true

[app]
autostart = false
check_updates = true
repository = "avicdro/quick-actions"

[paths]
chrome = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"

[launcher]
enabled = false

[notifications]
enabled = false
"#
        .to_string()
    }

    /// Actualiza `[app] autostart` en `config.toml` preservando el resto del
    /// archivo (comentarios y formato via toml_edit).
    ///
    /// Si la seccion `[app]` no existe, se crea al final del archivo.
    ///
    /// # Errors
    ///
    /// Devuelve `AppError::Config` si falla leer, escribir, o si el TOML es
    /// invalido (en ese caso no modifica nada).
    pub fn write_autostart(path: &Path, enabled: bool) -> Result<()> {
        Self::write_config_key(path, "app", "autostart", enabled)
    }

    /// Actualiza `[hotkeys] <campo>` en `config.toml` (pausa modular U3,
    /// `field` = "correct"/"translate"/...).
    ///
    /// # Errors
    ///
    /// Si falla leer/escribir o el TOML es invalido.
    pub fn write_hotkey(path: &Path, field: &str, enabled: bool) -> Result<()> {
        Self::write_config_key(path, "hotkeys", field, enabled)
    }

    /// Actualiza `[ollama] model` en `config.toml` (misma semantica que
    /// [`Config::write_autostart`]).
    ///
    /// # Errors
    ///
    /// Si falla leer/escribir o el TOML es invalido.
    pub fn write_ollama_model(path: &Path, model: &str) -> Result<()> {
        Self::write_config_key(path, "ollama", "model", model)
    }

    /// Setea `doc[section][key] = value` preservando el resto del documento.
    fn write_config_key(
        path: &Path,
        section: &str,
        key: &str,
        value: impl Into<toml_edit::Value>,
    ) -> Result<()> {
        let content = if path.exists() {
            std::fs::read_to_string(path)
                .map_err(|e| AppError::Config(format!("No se pudo leer config.toml: {e}")))?
        } else {
            Self::default_toml()
        };

        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::Config(format!("Error parseando config.toml: {e}")))?;

        doc[section][key] = toml_edit::value(value);

        std::fs::write(path, doc.to_string())
            .map_err(|e| AppError::Config(format!("No se pudo escribir config.toml: {e}")))?;

        tracing::info!("config.toml: [{section}] {key} actualizado");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_config_toml_parses() {
        // Garantia: el template que build.sh distribuye SIEMPRE parsea.
        // (P5: el template roto con [launcher]/[notifications] vacios hicieron
        // fallar el arranque con dialog de error.)
        let raw = include_str!("../config.toml");
        let parsed = toml::from_str::<Config>(raw);
        assert!(parsed.is_ok(), "config.toml del repo invalido: {parsed:?}");
    }

    #[test]
    fn empty_future_sections_default_disabled() {
        // Config legada con secciones presentes pero sin campos: no rompe.
        let c = toml::from_str::<Config>("[launcher]\n[notifications]\n")
            .expect("secciones vacias deben usar defaults");
        assert!(!c.launcher.enabled);
        assert!(!c.notifications.enabled);
    }

    #[test]
    fn config_default_values() {
        let c = Config::default();
        assert_eq!(c.ollama.model, "gemma4:cloud");
        assert_eq!(c.ui.font_size, 16);
        assert!(c.hotkeys.enabled);
        // P7 + U2 (default del usuario: Alt)
        assert_eq!(c.hotkeys.modifier, "alt");
        assert_eq!(c.hotkeys.capsmode, "hold");
        // P1/P4
        assert!(!c.app.autostart);
        assert!(c.app.check_updates);
        assert_eq!(c.app.repository, "avicdro/quick-actions");
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

    #[test]
    fn config_app_autostart_default_false() {
        let c = Config::default();
        assert!(!c.app.autostart);
    }

    #[test]
    fn config_parse_app_section() {
        let toml = r#"
[app]
autostart = true
"#;
        let c: Config = toml::from_str(toml).unwrap();
        assert!(c.app.autostart);
    }

    #[test]
    fn write_autostart_flips_value_and_preserves_comments() {
        let dir = std::env::temp_dir().join("qa-test-write-autostart");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");

        // 1. Archivo con comentarios y formato propio
        let original = r#"# mi config personalizada
[ollama]
model = "mi-modelo" # modelo chico

[app]
autostart = false
"#;
        std::fs::write(&path, original).unwrap();

        Config::write_autostart(&path, true).unwrap();
        let out = std::fs::read_to_string(&path).unwrap();

        assert!(out.contains("# mi config personalizada"));
        assert!(out.contains("# modelo chico"));
        assert!(out.contains("autostart = true"));
        assert!(c_of(&path).app.autostart);

        // 2. Volver a false
        Config::write_autostart(&path, false).unwrap();
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("autostart = false"));
        assert!(out.contains("model = \"mi-modelo\""));

        // 3. Archivo sin seccion [app]: se crea y se agrega la clave
        let no_app = "[hotkeys]\nenabled = true\n";
        std::fs::write(&path, no_app).unwrap();
        Config::write_autostart(&path, true).unwrap();
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("[hotkeys]"));
        assert!(out.contains("autostart = true"));
        assert!(c_of(&path).hotkeys.enabled);

        // 4. Archivo inexistente: crea default con el flag pedido
        let missing = dir.join("no-existe.toml");
        Config::write_autostart(&missing, true).unwrap();
        assert!(missing.exists());
        assert!(std::fs::read_to_string(&missing)
            .unwrap()
            .contains("autostart = true"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_ollama_model_preserves_and_flips() {
        let dir = std::env::temp_dir().join("qa-test-write-model");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");

        let original = r#"# comentario
[ollama]
model = "gemma4:cloud" # modelo actual
timeout_seconds = 90

[app]
autostart = false
"#;
        std::fs::write(&path, original).unwrap();

        Config::write_ollama_model(&path, "qwen3:32b").unwrap();
        let out = std::fs::read_to_string(&path).unwrap();

        assert!(out.contains("# comentario"));
        assert!(out.contains("model = \"qwen3:32b\""));
        assert!(out.contains("timeout_seconds = 90"));
        assert!(out.contains("[app]"));
        // Nota: el comentario EN-LINEA de la linea reemplazada ("# modelo
        // actual") NO se preserva (es decor del item reemplazado); el resto
        // del archivo si (comentarios propios + secciones). Documentado en
        // docs/plans/improvements-v0.3.md P3.

        // Y el TOML parsea con el modelo nuevo
        let c: Config = toml::from_str(&out).unwrap();
        assert_eq!(c.ollama.model, "qwen3:32b");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn c_of(path: &Path) -> Config {
        let s = std::fs::read_to_string(path).unwrap();
        toml::from_str(&s).unwrap()
    }
}
