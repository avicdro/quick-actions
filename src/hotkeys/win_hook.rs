//! Hook de teclado de bajo nivel (WH_KEYBOARD_LL) — Windows.
//!
//! Fase 1.3: sin statics. Todo el estado vive en un `HookState` de este
//! thread: los callbacks `extern "system"` no pueden capturar (contrato C
//! de HOOKPROC) y `SetWindowsHookExW` no permite pasar datos de usuario a
//! WH_KEYBOARD_LL, pero Windows entrega los eventos del LL hook EN el
//! thread que lo instala (por eso el thread necesita message pump), asi
//! que el `thread_local` de ese thread es el sitio natural.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::hotkeys::{CapsMode, HotkeyCommand, Modifier};
use crate::ui_protocol::RepaintFn;

const VK_CAPITAL: u32 = 0x14; // CapsLock
const VK_MENU: u32 = 0x12; // Alt key

/// Estado del thread del hook (ver doc del modulo).
struct HookState {
    /// Canal hacia el thread principal (hotkey -> app).
    sender: mpsc::Sender<HotkeyCommand>,
    /// P2: `true` = passthrough total de TODO el teclado (toggle tray).
    suspended: Arc<AtomicBool>,
    /// P7: modificador configurado (capturado al arrancar).
    modifier: Modifier,
    /// P7: modo del CapsLock (capturado al arrancar).
    caps_mode: CapsMode,
    /// Latch del modo `toggle` (P7): tap de CapsLock lo enciende; la proxima
    /// tecla de atajo dispara y lo apaga; cualquier otra tecla lo apaga.
    /// Plain bool: solo lo toca este thread (unico acceso por frame).
    latched: bool,
    /// En modo toggle nos comemos el keydown de CapsLock: tambien hay que
    /// comerse su keyup para no dejar colado un up de una down que nunca
    /// llego al sistema.
    swallow_caps_up: bool,
    /// Despierta el event loop del UI cuando se dispara un atajo.
    repaint: RepaintFn,
}

thread_local! {
    static HOOK_STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
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

/// Pasa el evento al siguiente hook de la cadena.
fn pass_through(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    // SAFETY: contrato del hook de Windows para eventos no procesados.
    unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
}

/// Inicia el message pump con el hook de teclado de bajo nivel.
///
/// Bloquea el thread actual. Debe ejecutarse en un thread dedicado.
///
/// Fase 1.3: recibe el estado inyectado (antes las init_* + statics).
pub fn run_hook_loop(
    sender: mpsc::Sender<HotkeyCommand>,
    suspended: Arc<AtomicBool>,
    modifier: Modifier,
    caps_mode: CapsMode,
    repaint: RepaintFn,
) {
    let (mod_name, mode_desc) = match modifier {
        Modifier::CapsLock => match caps_mode {
            CapsMode::Hold => ("CapsLock", "hold: mantener + tecla"),
            CapsMode::Toggle => ("CapsLock", "toggle: tap queda latched"),
        },
        Modifier::Alt => ("Alt", "hold: mantener + tecla"),
    };
    tracing::info!(
        "Hook de teclado instalándose. Modificador: {mod_name} ({mode_desc}). Atajos: +G/T/R/Enter/V/H"
    );

    HOOK_STATE.with_borrow_mut(|slot| {
        *slot = Some(HookState {
            sender,
            suspended,
            modifier,
            caps_mode,
            latched: false,
            swallow_caps_up: false,
            repaint,
        });
    });

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

    // try_with: si este callback somehow correra en otro thread (no debe),
    // mejor passthrough que panic.
    HOOK_STATE
        .try_with(|slot| {
            let mut slot = slot.borrow_mut();
            match slot.as_mut() {
                None => pass_through(n_code, w_param, l_param),
                Some(state) => handle_event(state, n_code, w_param, l_param),
            }
        })
        .unwrap_or_else(|_| pass_through(n_code, w_param, l_param))
}

/// Logica del evento con el estado del thread del hook (fase 1.3).
fn handle_event(state: &mut HookState, n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    // Pausado (P2): passthrough total, como si el hook ni estuviera.
    // De paso reseteamos el latch para no revivir estados viejos.
    if state.suspended.load(Ordering::Relaxed) {
        state.latched = false;
        state.swallow_caps_up = false;
        return pass_through(n_code, w_param, l_param);
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

    match state.modifier {
        Modifier::Alt => {
            // Comportamiento previo a P7: solo keydown con Alt presionado.
            if !is_keydown {
                return pass_through(n_code, w_param, l_param);
            }
            if !key_held(VK_MENU) {
                return pass_through(n_code, w_param, l_param);
            }
            if let Some(cmd) = mapped_command(vk_code) {
                fire_command(state, cmd);
                return LRESULT(1);
            }
            pass_through(n_code, w_param, l_param)
        }

        Modifier::CapsLock if state.caps_mode == CapsMode::Hold => {
            // CapsLock pasa intacto al sistema (alterna mayusculas); solo
            // keydown de tecla_atajo con CapsLock fisicamente presionado.
            if !is_keydown || vk_code == VK_CAPITAL {
                return pass_through(n_code, w_param, l_param);
            }
            if !key_held(VK_CAPITAL) {
                return pass_through(n_code, w_param, l_param);
            }
            if let Some(cmd) = mapped_command(vk_code) {
                fire_command(state, cmd);
                return LRESULT(1);
            }
            pass_through(n_code, w_param, l_param)
        }

        // CapsLock + toggle
        Modifier::CapsLock if vk_code == VK_CAPITAL => {
            if is_keydown {
                // Primer tap: latch. Segundo tap: apaga.
                if state.latched {
                    state.latched = false;
                    tracing::debug!("CapsLock toggle: latch apagado (2do tap).");
                } else {
                    state.latched = true;
                }
                state.swallow_caps_up = true;
                // Consumimos el keydown: el sistema nunca alterna mayusculas.
                return LRESULT(1);
            }
            if is_keyup && state.swallow_caps_up {
                state.swallow_caps_up = false;
                return LRESULT(1);
            }
            pass_through(n_code, w_param, l_param)
        }

        // CapsLock + toggle: teclas normales
        Modifier::CapsLock => {
            if !is_keydown {
                return pass_through(n_code, w_param, l_param);
            }
            let was_latched = std::mem::take(&mut state.latched);
            if was_latched {
                if let Some(cmd) = mapped_command(vk_code) {
                    // La tecla latcheada dispara y apaga el latch.
                    fire_command(state, cmd);
                    return LRESULT(1);
                }
                // Tecla no mapeada: apaga el latch y pasa al sistema
                // (seguimos escribiendo en la app normalmente).
            }
            pass_through(n_code, w_param, l_param)
        }
    }
}

fn fire_command(state: &mut HookState, cmd: HotkeyCommand) {
    let _ = state.sender.send(cmd);
    // Despertar el event loop del UI (antes: static CONTEXT + ui::request_repaint).
    (state.repaint)();
}
