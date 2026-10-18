use std::sync::{mpsc::Sender, OnceLock};

/// Comandos que pueden enviarse desde el hook de teclado al thread principal.
#[derive(Debug, Clone)]
pub enum HotkeyCommand {
    /// Corregir ortografia de la seleccion actual.
    Correct,
    /// Traducir la seleccion actual.
    Translate,
    /// Resumir la seleccion actual.
    Summarize,
    /// Generar respuesta sobre la seleccion actual.
    Generate,
    /// Mostrar historial de correcciones.
    ShowHistory,
    /// Mostrar ventana de ayuda.
    ShowHelp,
}

/// Tecla modificadora de los atajos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    /// CapsLock (default; ver README).
    CapsLock,
    /// Alt (comportamiento previo a P7).
    Alt,
}

impl Modifier {
    /// Parse configurable: `"alt"` -> Alt; cualquier otra cosa -> CapsLock.
    pub fn from_config(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "alt" => Self::Alt,
            _ => Self::CapsLock,
        }
    }
}

/// Comportamiento del modificador CapsLock (P7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapsMode {
    /// Mantener presionado y disparar (default; CapsLock nativo intacto).
    Hold,
    /// Tap queda "latched"; la proxima tecla de atajo dispara y apaga.
    Toggle,
}

impl CapsMode {
    /// Parse configurable: `"toggle"` -> Toggle; cualquier otra cosa -> Hold.
    pub fn from_config(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "toggle" => Self::Toggle,
            _ => Self::Hold,
        }
    }
}

static MODIFIER: OnceLock<Modifier> = OnceLock::new();
static CAPS_MODE: OnceLock<CapsMode> = OnceLock::new();

/// Configura modificador y modo de CapsLock (de `HotkeysConfig`).
///
/// Debe llamarse **antes** de [`start_listener`]; si no, quedan defaults.
pub fn init_caps(modifier: Modifier, caps_mode: CapsMode) {
    let _ = MODIFIER.set(modifier);
    let _ = CAPS_MODE.set(caps_mode);
}

/// Modificador vigente (defaults si nadie llamó [`init_caps`]).
pub fn modifier() -> Modifier {
    MODIFIER.get().copied().unwrap_or(Modifier::CapsLock)
}

/// Modo de CapsLock vigente (defaults si nadie llamó [`init_caps`]).
pub fn caps_mode() -> CapsMode {
    CAPS_MODE.get().copied().unwrap_or(CapsMode::Hold)
}

/// Inicializa el canal global para que el hook pueda enviar comandos.
///
/// Debe llamarse **antes** de [`start_listener`].
pub fn init_sender(sender: Sender<HotkeyCommand>) {
    #[cfg(windows)]
    win_hook::init_sender(sender);

    #[cfg(not(windows))]
    {
        let _ = sender;
    }
}

/// Registra el flag compartido de pausa (ver [`crate::ui::UiCommand::SetPaused`]).
///
/// Debe llamarse **antes** de [`start_listener`].
pub fn init_suspended(flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    #[cfg(windows)]
    win_hook::init_suspended(flag);

    #[cfg(not(windows))]
    {
        let _ = flag;
    }
}

/// Inicia el listener de atajos globales en un thread dedicado.
///
/// # Platform
///
/// Solo Windows. Usa `WH_KEYBOARD_LL` (low-level keyboard hook).
///
/// [`init_sender`] debe haberse llamado previamente.
pub fn start_listener() {
    #[cfg(windows)]
    {
        std::thread::spawn(move || {
            win_hook::run_hook_loop();
        });
    }

    #[cfg(not(windows))]
    {
        tracing::warn!("Hotkeys no soportados en esta plataforma.");
    }
}

#[cfg(windows)]
mod win_hook;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_parse() {
        assert_eq!(Modifier::from_config("capslock"), Modifier::CapsLock);
        assert_eq!(Modifier::from_config("hold"), Modifier::CapsLock);
        assert_eq!(Modifier::from_config("ALT"), Modifier::Alt);
        assert_eq!(Modifier::from_config(" alt "), Modifier::Alt);
        assert_eq!(Modifier::from_config("asdasd"), Modifier::CapsLock);
        assert_eq!(Modifier::from_config(""), Modifier::CapsLock);
    }

    #[test]
    fn caps_mode_parse() {
        assert_eq!(CapsMode::from_config("hold"), CapsMode::Hold);
        assert_eq!(CapsMode::from_config("toggle"), CapsMode::Toggle);
        assert_eq!(CapsMode::from_config("TOGGLE "), CapsMode::Toggle);
        assert_eq!(CapsMode::from_config("alt"), CapsMode::Hold);
        assert_eq!(CapsMode::from_config(""), CapsMode::Hold);
    }
}
