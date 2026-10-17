//! System tray icon manual con WinAPI (Windows).
//!
//! Implementacion directa usando Shell_NotifyIconW + TrackPopupMenu.
//! No depende de crates externas de tray.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage, RegisterClassExW, SetForegroundWindow,
    ShowWindow, TrackPopupMenu, TranslateMessage, IDC_ARROW, IDI_APPLICATION, MF_CHECKED,
    MF_STRING, MSG, SW_HIDE, TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TRACK_POPUP_MENU_FLAGS, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP,
    WM_QUIT, WM_RBUTTONUP, WM_USER, WNDCLASSEXW,
};

use crate::ui::UiCommand;

const WM_TRAYICON: u32 = WM_USER + 1;
const ID_TRAY_SHOW_HELP: u32 = 1001;
const ID_TRAY_CONFIG: u32 = 1002;
const ID_TRAY_EXIT: u32 = 1003;
const ID_TRAY_AUTOSTART: u32 = 1004;
const ID_TRAY_PAUSE: u32 = 1005;

/// Datos globales del tray icon (solo hay una instancia).
struct TrayData {
    ui_tx: Sender<UiCommand>,
    config_path: std::path::PathBuf,
    suspended: Arc<AtomicBool>,
}

static TRAY_DATA: OnceLock<TrayData> = OnceLock::new();

/// Toggle "Iniciar con Windows" (compartido por WM_COMMAND y TPM_RETURNCMD).
/// Escribe el registro y refleja el estado en config.toml.
/// Toggle pausa de atajos (compartido por WM_COMMAND y TPM_RETURNCMD).
/// Alterna el flag compartido y avisa al UI (toast persistente).
fn toggle_pause() {
    if let Some(data) = TRAY_DATA.get() {
        let next = !data.suspended.load(Ordering::Relaxed);
        data.suspended.store(next, Ordering::Relaxed);
        let _ = data.ui_tx.send(UiCommand::SetPaused(next));
        crate::ui::request_repaint();
        tracing::info!(
            "Atajos {} desde el tray.",
            if next { "PAUSADOS" } else { "REACTIVADOS" }
        );
    }
}

/// Toggle "Iniciar con Windows" (compartido por WM_COMMAND y TPM_RETURNCMD).
/// Escribe el registro y refleja el estado en config.toml.
fn toggle_autostart() {
    let next_on = !crate::autostart::is_enabled();
    match crate::autostart::set_enabled(next_on) {
        Ok(()) => {
            if let Some(data) = TRAY_DATA.get() {
                if let Err(e) = crate::config::Config::write_autostart(&data.config_path, next_on) {
                    tracing::warn!("Registro actualizado pero config no se guardo: {e}");
                }
            }
            tracing::info!(
                "Auto-arranque {} desde el tray.",
                if next_on { "ACTIVADO" } else { "DESACTIVADO" }
            );
        }
        Err(e) => tracing::error!("Fallo toggle auto-arranque: {e}"),
    }
}

/// Inicia el tray icon en un thread dedicado.
///
/// # Arguments
///
/// * `ui_tx` - Canal para enviar comandos al UI (mostrar ayuda).
/// * `config_path` - Ruta al config.toml (para abrir con notepad).
/// * `suspended` - Flag compartido de pausa de atajos (P2).
pub fn init_tray(
    ui_tx: Sender<UiCommand>,
    config_path: std::path::PathBuf,
    suspended: Arc<AtomicBool>,
) {
    // Guardar datos globales
    let _ = TRAY_DATA.set(TrayData {
        ui_tx,
        config_path,
        suspended,
    });

    std::thread::spawn(move || unsafe {
        run_tray_loop();
    });
}

/// Inicia el icono de bandeja. Requiere un message pump en el thread.
///
/// # Safety
///
/// Esta funcion es `unsafe` porque llama a multiples funciones inseguras
/// de la API de Windows (RegisterClassExW, CreateWindowExW, Shell_NotifyIconW,
/// GetMessageW). Requiere que se llame desde un thread con message pump.
unsafe fn run_tray_loop() {
    // SAFETY: GetModuleHandleW(None) devuelve el handle del modulo actual.
    // Es seguro de llamar. Si falla, usamos un HMODULE nulo (aceptable para
    // registrar la clase de ventana).
    let hinstance =
        GetModuleHandleW(None).unwrap_or(windows::Win32::Foundation::HMODULE(std::ptr::null_mut()));

    let class_name = windows::core::w!("QuickActionsTrayWnd");
    let wnd_class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        hInstance: windows::Win32::Foundation::HINSTANCE(hinstance.0),
        lpszClassName: class_name,
        lpfnWndProc: Some(tray_wnd_proc),
        hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(None, IDC_ARROW).unwrap_or(
            windows::Win32::UI::WindowsAndMessaging::HCURSOR(std::ptr::null_mut()),
        ),
        hbrBackground: HBRUSH(std::ptr::null_mut()),
        ..std::mem::zeroed()
    };

    let atom = RegisterClassExW(&wnd_class);
    if atom == 0 {
        tracing::error!("RegisterClassExW fallo.");
        return;
    }

    let hwnd = match CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class_name,
        windows::core::w!("QuickActionsTray"),
        WINDOW_STYLE(0),
        0,
        0,
        0,
        0,
        None,
        None,
        hinstance,
        None,
    ) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("CreateWindowExW fallo: {e}");
            return;
        }
    };

    let _ = ShowWindow(hwnd, SW_HIDE);

    // SAFETY: LoadIconW(None, IDI_APPLICATION) carga un icono del sistema.
    // Si falla, logeamos el error pero continuamos (el icono puede ser nulo).
    let hicon = match LoadIconW(None, IDI_APPLICATION) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("LoadIconW fallo, usando icono nulo: {e}");
            windows::Win32::UI::WindowsAndMessaging::HICON(std::ptr::null_mut())
        }
    };

    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAYICON,
        hIcon: hicon,
        ..std::mem::zeroed()
    };

    let tooltip = windows::core::w!("Quick Actions — Atajos de IA");
    let tip_bytes = tooltip.as_wide();
    for (i, &b) in tip_bytes.iter().enumerate().take(127) {
        nid.szTip[i] = b;
    }

    if Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
        tracing::info!("Tray icon instalado correctamente.");
    } else {
        tracing::error!("Shell_NotifyIconW fallo.");
        return;
    }

    // Message pump
    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, None, 0, 0).into() {
        if msg.message == WM_QUIT {
            break;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    // Eliminar icono
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    tracing::info!("Tray icon desinstalado.");
}

/// Window procedure para la ventana oculta del tray.
extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_TRAYICON => match l_param.0 as u32 {
                WM_LBUTTONUP => {
                    if let Some(data) = TRAY_DATA.get() {
                        let _ = data
                            .ui_tx
                            .send(UiCommand::ShowHelp(crate::actions::help_text()));
                    }
                    LRESULT(0)
                }

                WM_RBUTTONUP => {
                    show_tray_menu(hwnd);
                    LRESULT(0)
                }

                _ => DefWindowProcW(hwnd, msg, w_param, l_param),
            },

            WM_COMMAND => {
                let id = w_param.0 as u32;
                match id {
                    ID_TRAY_SHOW_HELP => {
                        if let Some(data) = TRAY_DATA.get() {
                            let _ = data
                                .ui_tx
                                .send(UiCommand::ShowHelp(crate::actions::help_text()));
                        }
                    }
                    ID_TRAY_CONFIG => {
                        if let Some(data) = TRAY_DATA.get() {
                            let _ = std::process::Command::new("notepad")
                                .arg(&data.config_path)
                                .spawn();
                        }
                    }
                    ID_TRAY_AUTOSTART => {
                        toggle_autostart();
                    }
                    ID_TRAY_PAUSE => {
                        toggle_pause();
                    }
                    ID_TRAY_EXIT => {
                        tracing::info!("Saliendo via tray menu...");
                        PostQuitMessage(0);
                        std::process::exit(0);
                    }
                    _ => {}
                }
                LRESULT(0)
            }

            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }

            _ => DefWindowProcW(hwnd, msg, w_param, l_param),
        }
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let hmenu = match CreatePopupMenu() {
        Ok(h) => h,
        Err(_) => return,
    };

    let _ = AppendMenuW(
        hmenu,
        MF_STRING,
        ID_TRAY_SHOW_HELP as usize,
        windows::core::w!("Ayuda"),
    );
    let _ = AppendMenuW(
        hmenu,
        MF_STRING,
        ID_TRAY_CONFIG as usize,
        windows::core::w!("Configuracion"),
    );
    let autostart_on = crate::autostart::is_enabled();
    let autostart_flags = if autostart_on {
        windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS(MF_STRING.0 | MF_CHECKED.0)
    } else {
        MF_STRING
    };
    let _ = AppendMenuW(
        hmenu,
        autostart_flags,
        ID_TRAY_AUTOSTART as usize,
        windows::core::w!("Iniciar con Windows"),
    );
    let paused = TRAY_DATA
        .get()
        .is_some_and(|d| d.suspended.load(Ordering::Relaxed));
    let pause_flags = if paused {
        windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS(MF_STRING.0 | MF_CHECKED.0)
    } else {
        MF_STRING
    };
    let _ = AppendMenuW(
        hmenu,
        pause_flags,
        ID_TRAY_PAUSE as usize,
        windows::core::w!("Pausar atajos"),
    );
    let _ = AppendMenuW(hmenu, MF_STRING, 0, windows::core::w!(""));
    let _ = AppendMenuW(
        hmenu,
        MF_STRING,
        ID_TRAY_EXIT as usize,
        windows::core::w!("Salir"),
    );

    let mut pt = std::mem::zeroed();
    let _ = GetCursorPos(&mut pt);

    let _ = SetForegroundWindow(hwnd);

    let flags = TRACK_POPUP_MENU_FLAGS(
        TPM_LEFTALIGN.0 | TPM_RIGHTBUTTON.0 | TPM_RETURNCMD.0 | TPM_NONOTIFY.0,
    );
    let cmd = TrackPopupMenu(hmenu, flags, pt.x, pt.y, 0, hwnd, None);

    let _ = DestroyMenu(hmenu);

    // TPM_RETURNCMD: el valor de retorno es el ID del menu item seleccionado
    let menu_id = cmd.0 as u32;
    match menu_id {
        ID_TRAY_SHOW_HELP => {
            if let Some(data) = TRAY_DATA.get() {
                let _ = data
                    .ui_tx
                    .send(UiCommand::ShowHelp(crate::actions::help_text()));
            }
        }
        ID_TRAY_CONFIG => {
            if let Some(data) = TRAY_DATA.get() {
                let _ = std::process::Command::new("notepad")
                    .arg(&data.config_path)
                    .spawn();
            }
        }
        ID_TRAY_AUTOSTART => {
            toggle_autostart();
        }
        ID_TRAY_PAUSE => {
            toggle_pause();
        }
        ID_TRAY_EXIT => {
            tracing::info!("Saliendo via tray menu...");
            PostQuitMessage(0);
            std::process::exit(0);
        }
        _ => {}
    }
}
