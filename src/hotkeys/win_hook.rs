use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::hotkeys::{CapsMode, HotkeyCommand, Modifier};

/// Canal global para enviar comandos desde el hook al thread principal.
static SENDER: OnceLock<Sender<HotkeyCommand>> = OnceLock::new();

/// Cuando es `true`, el hook hace passthrough total de TODO el teclado
/// (no intercepta atajos). Toggle desde el tray ("Pausar" — P2).
static SUSPENDED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Latch del modo `toggle` (P7): tap de CapsLock lo enciende; la proxima
/// tecla de atajo dispara y lo apaga; cualquier otra tecla lo apaga.
static LATCHED: AtomicBool = AtomicBool::new(false);

/// En modo toggle nos comemos el keydown de CapsLock: tambien hay que
/// comerse su keyup para no dejar colado un up de una down que nunca
/// llego al sistema.
static SWALLOW_CAPS_UP: AtomicBool = AtomicBool::new(false);

const VK_CAPITAL: u32 = 0x14; // CapsLock
const VK_MENU: u32 = 0x12; // Alt key

/// Inicializa el sender global. Debe llamarse antes de [`run_hook_loop`].
pub fn init_sender(sender: Sender<HotkeyCommand>) {
    let _ = SENDER.set(sender);
}

/// Registra el flag compartido de pausa. Antes de [`run_hook_loop`].
pub fn init_suspended(flag: Arc<AtomicBool>) {
    let _ = SUSPENDED.set(flag);
}

fn get_sender() -> Option<&'static Sender<HotkeyCommand>> {
    SENDER.get()
}

fn is_suspended() -> bool {
    SUSPENDED.get().is_some_and(|f| f.load(Ordering::Relaxed))
}

/// Tabla de teclas mapeadas: vk -> comando.
fn mapped_command(vk_code: u32) -> Option<HotkeyCommand> {
    match vk_code {
        0x47 => Some(HotkeyCommand::Correct),     // G
        0x54 => Some(HotkeyCommand::Translate),   // T
        0x52 => Some(HotkeyCommand::Summarize),   // R
        0x0D => Some(HotkeyCommand::Generate),    // Enter
        0x56 => Some(HotkeyCommand::ShowHistory), // V
        0x48 => Some(HotkeyCommand::ShowHelp),    // H
        _ => None,
    }
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
    let hook: HHOOK =
        match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) } {
            Ok(h) => h,
            Err(e) => {
                tracing::error!("No se pudo instalar el hook de teclado: {e}");
                return;
            }
        };

    let (mod_name, mode_desc) = match crate::hotkeys::modifier() {
        Modifier::CapsLock => match crate::hotkeys::caps_mode() {
            CapsMode::Hold => ("CapsLock", "hold: mantener + tecla"),
            CapsMode::Toggle => ("CapsLock", "toggle: tap queda latched"),
        },
        Modifier::Alt => ("Alt", "hold: mantener + tecla"),
    };
    tracing::info!(
        "Hook de teclado instalado. Modificador: {mod_name} ({mode_desc}). Atajos: +G/T/R/Enter/V/H"
    );

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

/// Callback del hook de teclado de bajo nivel (LL hook).
///
/// Modificador configurable (P7):
/// - `Alt` (config anterior): hold; GetAsyncKeyState detecta presion.
/// - `CapsLock + hold`: CapsLock pasa al sistema sin cambios ( alterna
///   mayusculas nativamente). Los atajos disparan mientras se mantiene.
/// - `CapsLock + toggle`: nos comemos CapsLock (down/up); el tap queda
///   latched y la proxima tecla de atajo dispara. Cualquier otra tecla
///   apaga el latch pasando al sistema normalmente.
extern "system" fn keyboard_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code < 0 {
        // SAFETY: n_code < 0 significa que el hook no debe procesar el evento.
        // Pasamos directamente al siguiente hook de la cadena.
        return unsafe { CallNextHookEx(None, n_code, w_param, l_param) };
    }

    // Pausado (P2): passthrough total, como si el hook ni estuviera.
    // De paso reseteamos el latch para no revivir estados viejos.
    if is_suspended() {
        LATCHED.store(false, Ordering::Relaxed);
        SWALLOW_CAPS_UP.store(false, Ordering::Relaxed);
        // SAFETY: Pasar el evento al siguiente hook de la cadena.
        return unsafe { CallNextHookEx(None, n_code, w_param, l_param) };
    }

    // SAFETY: l_param apunta a un KBDLLHOOKSTRUCT valido proporcionado por
    // Windows. El puntero es valido durante toda la duracion del callback.
    let kb_struct = unsafe { &*(l_param.0 as *const KBDLLHOOKSTRUCT) };
    let vk_code = kb_struct.vkCode;

    let is_keydown = w_param.0 == WM_KEYDOWN as usize || w_param.0 == WM_SYSKEYDOWN as usize;
    let is_keyup = w_param.0 == WM_KEYUP as usize || w_param.0 == WM_SYSKEYUP as usize;

    // SAFETY: GetAsyncKeyState lee el estado fisico de la tecla desde el
    // driver de teclado. Es seguro de llamar desde cualquier thread.
    let key_held = |vk: u32| unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 };

    match crate::hotkeys::modifier() {
        Modifier::Alt => {
            // Comportamiento previo a P7: solo keydown con Alt presionado.
            if !is_keydown {
                return unsafe { CallNextHookEx(None, n_code, w_param, l_param) };
            }
            if !key_held(VK_MENU) {
                return unsafe { CallNextHookEx(None, n_code, w_param, l_param) };
            }
            if let Some(cmd) = mapped_command(vk_code) {
                fire_command(cmd);
                return LRESULT(1);
            }
            unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
        }

        Modifier::CapsLock if crate::hotkeys::caps_mode() == CapsMode::Hold => {
            // CapsLock pasa intacto al sistema (alterna mayusculas); solo
            // keydown de tecla_atajo con CapsLock fisicamente presionado.
            if !is_keydown || vk_code == VK_CAPITAL {
                return unsafe { CallNextHookEx(None, n_code, w_param, l_param) };
            }
            if !key_held(VK_CAPITAL) {
                return unsafe { CallNextHookEx(None, n_code, w_param, l_param) };
            }
            if let Some(cmd) = mapped_command(vk_code) {
                fire_command(cmd);
                return LRESULT(1);
            }
            unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
        }

        // CapsLock + toggle
        Modifier::CapsLock if vk_code == VK_CAPITAL => {
            if is_keydown {
                // Primer tap: latch. Segundo tap: apaga.
                if LATCHED.swap(false, Ordering::Relaxed) {
                    tracing::debug!("CapsLock toggle: latch apagado (2do tap).");
                } else {
                    LATCHED.store(true, Ordering::Relaxed);
                }
                SWALLOW_CAPS_UP.store(true, Ordering::Relaxed);
                // Consumimos el keydown: el sistema nunca alterna mayusculas.
                return LRESULT(1);
            }
            if is_keyup && SWALLOW_CAPS_UP.swap(false, Ordering::Relaxed) {
                return LRESULT(1);
            }
            unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
        }

        // CapsLock + toggle: teclas normales
        Modifier::CapsLock => {
            if !is_keydown {
                return unsafe { CallNextHookEx(None, n_code, w_param, l_param) };
            }
            let was_latched = LATCHED.swap(false, Ordering::Relaxed);
            if was_latched {
                if let Some(cmd) = mapped_command(vk_code) {
                    // La tecla latcheada dispara y apaga el latch.
                    fire_command(cmd);
                    return LRESULT(1);
                }
                // Tecla no mapeada: apaga el latch y pasa al sistema
                // (seguimos escribiendo en la app normalmente).
            }
            unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
        }
    }
}

fn fire_command(cmd: HotkeyCommand) {
    if let Some(sender) = get_sender() {
        let _ = sender.send(cmd);
    }
    crate::ui::request_repaint();
}
