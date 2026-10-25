//! S1 — Instancia unica: mutex de Windows + foco de la instancia viva.
//!
//! Tray + hook de teclado + GUI DEBEN ser unicos: abrir el exe dos veces
//! produce dos trays, dos hooks y checks de update duplicados. El guard
//! canonico en Windows es un NAMED MUTEX:
//!
//! 1. Al arranque, `CreateMutexW("Local\QuickActions::single-instance")`.
//! 2. Si `GetLastError() == ERROR_ALREADY_EXISTS` -> ya hay instancia:
//!    - si existe el marker `update-applied.tmp` (raza del auto-update U7:
//!      el proceso viejo hace spawn del nuevo y muere ~300ms despues) ->
//!      reintentar hasta 5s; al morir el viejo el mutex se libera y el
//!      nuevo arranca normalmente y muestra su toast de version,
//!    - si no (doble clic real) -> enfocar la GUI de la instancia viva
//!      (solo si el usuario la tiene abierta de verdad) y devolver `false`
//!      para una salida limpia (flush de logs incluido).
//!
//! El mutex vive hasta que el proceso muere: `windows-rs` HANDLE no tiene
//! Drop, y se leakea a proposito — el SO lo limpia al terminar. Nombre con
//! namespace `Local\`: por-sesion de login (autostart y usuario usan la
//! misma sesion; sin colisiones con otros usuarios ni con servicios).
//!
//! Fail-open: si `CreateMutexW` falla por cualquier motivo raro, se loguea
//! y se arranca igual (peor caso: dos instancias, como siempre hasta ahora)
//! — nunca bloquear el arranque de la app por un guard.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;

/// Nombre canonico del mutex (namespace `Local\` = por sesion de login).
const MUTEX_NAME: PCWSTR = w!("Local\\QuickActions::single-instance");

/// Ventana total de gracia para la raza del auto-update (el viejo muere
/// ~300ms tras el spawn; 5s cubre CPUs lentas con margen).
const UPDATE_RACE_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
/// Paso entre reintentos de adquisicion del mutex.
const RETRY_STEP: std::time::Duration = std::time::Duration::from_millis(150);

/// Adquiere el mutex o, si ya hay instancia viva, enfoca su GUI y devuelve
/// `false` para que el llamador salga (return: el WorkerGuard del logger
/// flusha al drop — `process::exit` mataria las lineas pendientes).
///
/// - `true`  -> somos la instancia unica; proseguir con el arranque.
/// - `false` -> otra instancia vive; salir limpio (exit code 0).
pub fn acquire(exe_dir: &std::path::Path) -> bool {
    let marker = exe_dir.join("update-applied.tmp");
    let deadline = std::time::Instant::now() + UPDATE_RACE_WAIT;

    loop {
        match try_acquire() {
            Ok(true) => return true, // primeras y duenas del mutex: seguir
            Err(e) => {
                tracing::warn!("S1: CreateMutex fallo ({e}); arranco sin guard (fail-open).");
                return true;
            }
            Ok(false) => {}
        }

        // Ya existe una instancia. ¿Es la raza del auto-update (U7)?
        // El marker solo existe en ese instante (lo escribe el updater
        // justo antes del spawn y el bootstrap lo borra al leerlo).
        if marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(RETRY_STEP);
            continue;
        }
        if marker.exists() {
            // Esperamos todos los reintentos y el proceso nuevo nunca tomo
            // el mutex: no dejar el marker envenenando futuros arranques.
            let _ = std::fs::remove_file(&marker);
            tracing::warn!(
                "S1: marker update-applied.tmp eliminado tras esperar {UPDATE_RACE_WAIT:?}."
            );
        }

        // Doble clic real: enfocar la GUI si esta abierta de verdad.
        if focus_existing_gui() {
            tracing::info!("S1: instancia ya en ejecucion; GUI enfocada — saliendo.");
        } else {
            tracing::info!("S1: instancia ya en ejecucion (GUI oculta en tray) — saliendo.");
        }
        return false;
    }
}

/// Intento de adquisicion:
/// - `Ok(true)`  -> mutex nuestro para siempre (handle leak: vida del proceso).
/// - `Ok(false)` -> ya existia (handle propio cerrado inmediatamente).
/// - `Err(e)`    -> fallo de CreateMutex (fail-open en el llamador).
fn try_acquire() -> Result<bool, windows::core::Error> {
    match unsafe { CreateMutexW(None, true, MUTEX_NAME) } {
        Ok(handle) => {
            // GetLastError SIEMPRE inmediatamente despues de la llamada.
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                let _ = unsafe { CloseHandle(handle) };
                Ok(false)
            } else {
                // windows-rs HANDLE es Copy SIN Drop: salir de scope NO cierra
                // el handle — el mutex queda tomado por vida del proceso y el
                // SO lo libera al morir. Binding explicito para dejarlo claro.
                let _handle_alive_for_process = handle;
                Ok(true)
            }
        }
        Err(e) => Err(e),
    }
}

/// Enfoca la ventana principal de OTRO proceso (titulada "Quick Actions")
/// si y solo si el usuario la tiene abierta de verdad: visible y SIN modo
/// tray (offscreen + WS_EX_TOOLWINDOW, V4fix). En modo tray no se toca
/// nada — mostrar la GUI desde fuera desincronizaria el libro de egui.
///
/// Secuencia identica a `present_main_window()` (V6): restore si iconica,
/// poke TOPMOST→NOTOPMOST y SetForegroundWindow best-effort.
fn focus_existing_gui() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, FindWindowW, GetWindowLongPtrW, IsIconic, IsWindowVisible,
        SetForegroundWindow, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_NOTOPMOST, HWND_TOPMOST,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOWNOACTIVATE,
        WS_EX_TOOLWINDOW,
    };

    // El titulo solo es "Quick Actions" (con espacio) desde el primer frame
    // (ViewportCommand::Title); si la instancia viva va a medio boot, no hay
    // nada que enfocar — salir silenciosamente es el camino seguro.
    let Ok(hwnd) = (unsafe { FindWindowW(None, w!("Quick Actions")) }) else {
        return false;
    };

    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ex & (WS_EX_TOOLWINDOW.0 as isize) != 0 {
            return false; // modo tray: la 1x1 offscreen; no confundir
        }

        // Restaurar si acaso estaba minimizada; si no, no-op seguro.
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        // Poke de z-order: TOPMOST una pasada + NOTOPMOST -> top del z-order
        // (incluida sobre un fullscreen-borderless) sin quedar always-on-top.
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
        let _ = SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd); // best-effort (puede ser denegado)
        true
    }
}
