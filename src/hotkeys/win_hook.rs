use std::sync::mpsc::Sender;
use std::sync::OnceLock;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
};

use crate::hotkeys::HotkeyCommand;

/// Canal global para enviar comandos desde el hook al thread principal.
static SENDER: OnceLock<Sender<HotkeyCommand>> = OnceLock::new();

/// Inicializa el sender global. Debe llamarse antes de [`run_hook_loop`].
pub fn init_sender(sender: Sender<HotkeyCommand>) {
    let _ = SENDER.set(sender);
}

fn get_sender() -> Option<&'static Sender<HotkeyCommand>> {
    SENDER.get()
}

/// Inicia el message pump con el hook de teclado de bajo nivel.
///
/// Bloquea el thread actual. Debe ejecutarse en un thread dedicado.
pub fn run_hook_loop() {
    if SENDER.get().is_none() {
        tracing::error!("Sender no inicializado. Llama a init_sender() antes de run_hook_loop().");
        return;
    }

    // SAFETY: SetWindowsHookExW con WH_KEYBOARD_LL instala un hook global.
    // - `None` para hmod es correcto para hooks globales (low-level).
    // - `0` para dwThreadId significa "todos los threads" (hook global).
    // - El callback `keyboard_proc` cumple el contrato de HOOKPROC.
    // Requiere que el thread tenga un message pump (GetMessageW) para
    // que Windows entregue los eventos al callback.
    let hook: HHOOK = match unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_proc),
            None,
            0,
        )
    } {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("No se pudo instalar el hook de teclado: {e}");
            return;
        }
    };

    tracing::info!("Hook de teclado instalado. Alt = modificador. Atajos: Alt+G/T/R/Enter/V/H");

    // SAFETY: GetMessageW bloquea hasta que llega un mensaje.
    // El loop termina cuando GetMessageW devuelve false (WM_QUIT).
    // UnhookWindowsHookEx limpia el hook antes de salir.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = UnhookWindowsHookEx(hook);
    }

    tracing::info!("Hook de teclado desinstalado.");
}

/// Callback del hook de teclado de bajo nivel.
///
/// Usa Alt como modificador (no requiere supresion, GetAsyncKeyState lo detecta
/// confiablemente, y no secuestra el teclado).
extern "system" fn keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code < 0 {
        // SAFETY: n_code < 0 significa que el hook no debe procesar el evento.
        // Pasamos directamente al siguiente hook de la cadena.
        return unsafe { CallNextHookEx(None, n_code, w_param, l_param) };
    }

    // SAFETY: l_param apunta a un KBDLLHOOKSTRUCT valido proporcionado por
    // Windows. El puntero es valido durante toda la duracion del callback.
    let kb_struct = unsafe { &*(l_param.0 as *const KBDLLHOOKSTRUCT) };
    let vk_code = kb_struct.vkCode;

    const VK_MENU: u32 = 0x12; // Alt key

    let is_keydown = w_param.0 == WM_KEYDOWN as usize || w_param.0 == WM_SYSKEYDOWN as usize;

    // Solo procesar keydown (ignorar keyup)
    if !is_keydown {
        // SAFETY: Pasar el evento al siguiente hook sin procesarlo.
        return unsafe { CallNextHookEx(None, n_code, w_param, l_param) };
    }

    // SAFETY: GetAsyncKeyState lee el estado fisico de la tecla desde el
    // driver de teclado. Es seguro de llamar desde cualquier thread.
    let alt_held = unsafe {
        (GetAsyncKeyState(VK_MENU as i32) as u16) & 0x8000 != 0
    };

    if !alt_held {
        // Alt no presionado: pasar la tecla al sistema normalmente
        // SAFETY: Pasar el evento al siguiente hook sin procesarlo.
        return unsafe { CallNextHookEx(None, n_code, w_param, l_param) };
    }

    // Alt esta presionado: verificar si es una tecla mapeada
    let command = match vk_code {
        0x47 => Some(HotkeyCommand::Correct),   // G
        0x54 => Some(HotkeyCommand::Translate),  // T
        0x52 => Some(HotkeyCommand::Summarize), // R
        0x0D => Some(HotkeyCommand::Generate),   // Enter
        0x56 => Some(HotkeyCommand::ShowHistory), // V
        0x48 => Some(HotkeyCommand::ShowHelp),   // H
        _ => None,
    };

    if let Some(cmd) = command {
        if let Some(sender) = get_sender() {
            let _ = sender.send(cmd);
        }
        crate::ui::request_repaint();
        return LRESULT(1); // Consumir la tecla mapeada
    }

    // Alt + tecla no mapeada: pasar al sistema (puede ser util para otras apps)
    // SAFETY: Pasar el evento al siguiente hook en la cadena.
    unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
}
