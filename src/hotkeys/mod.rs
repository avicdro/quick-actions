use std::sync::mpsc::Sender;

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
