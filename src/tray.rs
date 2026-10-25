//! System tray icon manual con WinAPI (Windows).
//!
//! Implementacion directa usando Shell_NotifyIconW + TrackPopupMenu.
//! No depende de crates externas de tray.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, GetWindowLongPtrW, LoadIconW, LoadImageW, PostQuitMessage,
    RegisterClassExW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TrackPopupMenu,
    TranslateMessage, GWLP_USERDATA, HICON, IDC_ARROW, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTSIZE,
    LR_SHARED, MF_CHECKED, MF_SEPARATOR, MF_STRING, MSG, SW_HIDE, TPM_LEFTALIGN, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TRACK_POPUP_MENU_FLAGS, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_QUIT, WM_RBUTTONUP, WM_USER, WNDCLASSEXW,
};

use crate::app_state::AppState;
use crate::ui_protocol::UiCommand;

const WM_TRAYICON: u32 = WM_USER + 1;
const ID_TRAY_SHOW_HELP: u32 = 1001;
const ID_TRAY_CONFIG: u32 = 1002;
const ID_TRAY_EXIT: u32 = 1003;
const ID_TRAY_AUTOSTART: u32 = 1004;
const ID_TRAY_PAUSE: u32 = 1005;
/// Primer ID reservado para items del submenu de modelos (2100.., max 15).
const ID_MODELO_BASE: u32 = 2100;
const MAX_MODELOS_MENU: usize = 15;

/// Primer ID del submenu de atajos (U3): items en ID_ATAJOS_BASE+1..=+6,
/// uno por `HOTKEY_ACTIONS` en orden.
const ID_ATAJOS_BASE: u32 = 1200;
/// Submenu atajos: "Activar todos".
const ID_ATAJOS_TODOS: u32 = 1207;
/// Submenu atajos: "Desactivar todos".
const ID_ATAJOS_NINGUNO: u32 = 1208;

/// Indice de `HOTKEY_ACTIONS` para un id del submenu de atajos (None para
/// todos/ninguno/invalidos).
fn atajo_index_for_id(id: u32) -> Option<usize> {
    let off = id.checked_sub(ID_ATAJOS_BASE)?;
    match off {
        1..=6 => Some((off - 1) as usize),
        _ => None,
    }
}

/// Datos del tray cableados al wnd_proc (fase 1.3).
///
/// El `tray_wnd_proc` es `extern "system"` y no puede capturar nada, y
/// WH_KEYBOARD_LL-style globals no hacen falta: se le pasa el estado a la
/// ventana via `GWLP_USERDATA` (el truco canonico de Win32 para wnd procs).
struct TrayData {
    state: Arc<AppState>,
}

/// Recupera el estado cableado en la ventana del tray (None si aun no
/// se cableo, p.ej. en mensajes tempranos del wnd proc).
fn tray_state(hwnd: HWND) -> Option<&'static TrayData> {
    let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if raw == 0 {
        return None;
    }
    // SAFETY: el puntero se apunto con Box::leak en run_tray_loop, que
    // vive hasta el fin del proceso (el tray nunca se destruye antes).
    Some(unsafe { &*(raw as *const TrayData) })
}

/// Actions del submenu de atajos (U3): toggle individual o all-or-none.
///
/// Persiste en `[hotkeys]` via toml_edit y aplica en el `RwLock` compartido
/// para efecto inmediato (sin reinicio). Feedback con toast Notice.
fn handle_atajos_menu(state: &AppState, id: u32) {
    let hk_now = state.config.read().ok().map(|c| c.hotkeys.clone());
    let Some(hk_now) = hk_now else { return };

    let changes: Vec<(crate::hotkeys::HotkeyCommand, bool)> = if id == ID_ATAJOS_TODOS {
        crate::hotkeys::HOTKEY_ACTIONS
            .iter()
            .map(|(cmd, _, _)| (*cmd, true))
            .collect()
    } else if id == ID_ATAJOS_NINGUNO {
        crate::hotkeys::HOTKEY_ACTIONS
            .iter()
            .map(|(cmd, _, _)| (*cmd, false))
            .collect()
    } else if let Some(idx) = atajo_index_for_id(id) {
        let (cmd, _, _) = crate::hotkeys::HOTKEY_ACTIONS[idx];
        vec![(cmd, !crate::hotkeys::is_hotkey_enabled(cmd, &hk_now))]
    } else {
        return;
    };

    // Persistir y aplicar; el primer fallo corta el resto.
    for (cmd, on) in &changes {
        if let Err(e) =
            crate::config::Config::write_hotkey(&state.config_path, cmd.config_field(), *on)
        {
            tracing::warn!("No se pudo persistir [hotkeys] {}: {e}", cmd.config_field());
            let msg = format!("Error: ajuste '{}' no guardado", cmd.config_field());
            let _ = state.ui_tx.send(UiCommand::SetStatus(msg));
            state.repaint();
            return;
        }
        if let Ok(mut c) = state.config.write() {
            *crate::hotkeys::hotkey_switch(*cmd, &mut c.hotkeys) = *on;
        }
    }

    // Toast: cambido unico -> detalle; all-or-none -> resumen.
    let msg = if changes.len() == 1 {
        let (cmd, on) = changes[0];
        format!(
            "Atajo '{}+{}' {}",
            state.hook_modifier.name(),
            cmd.key_label(),
            if on { "activado" } else { "desactivado" }
        )
    } else {
        let on_count = changes.iter().filter(|(_, on)| *on).count();
        format!("Atajos: {on_count}/{} activados", changes.len())
    };
    let _ = state.ui_tx.send(UiCommand::SetStatus(msg));
    state.repaint();
    tracing::info!("Menu atajos: {} cambio(s) aplicado(s).", changes.len());
}

/// Toggle pausa de atajos (compartido por WM_COMMAND y TPM_RETURNCMD).
/// Alterna el flag compartido y avisa al UI (toast persistente).
fn toggle_pause(state: &AppState) {
    let next = !state.suspendido.load(Ordering::Relaxed);
    state.suspendido.store(next, Ordering::Relaxed);
    let _ = state.ui_tx.send(UiCommand::SetPaused(next));
    state.repaint();
    tracing::info!(
        "Atajos {} desde el tray.",
        if next { "PAUSADOS" } else { "REACTIVADOS" }
    );
}

/// Cambia el modelo de Ollama (P3): persiste en config.toml y hot-reload
/// a la config compartida (las proximas acciones usan el nuevo ya).
fn set_ollama_model(state: &AppState, model: &str) {
    // 1. Persistir config.toml (toml_edit preserva comentarios/formato)
    if let Err(e) = crate::config::Config::write_ollama_model(&state.config_path, model) {
        tracing::error!("No se pudo persistir modelo en config.toml: {e}");
        return;
    }

    // 2. Actualizar la config en memoria (hot-reload)
    match state.config.write() {
        Ok(mut c) => {
            c.ollama.model = model.to_string();
            tracing::info!("Modelo cambiado a '{model}' (vigente para proximas acciones).");
        }
        Err(_) => tracing::error!("config RwLock envenenado al setear modelo"),
    }
}

/// Toggle "Iniciar con Windows" (compartido por WM_COMMAND y TPM_RETURNCMD).
/// Escribe el registro y refleja el estado en config.toml.
fn toggle_autostart(state: &AppState) {
    let next_on = !crate::autostart::is_enabled();
    match crate::autostart::set_enabled(next_on) {
        Ok(()) => {
            if let Err(e) = crate::config::Config::write_autostart(&state.config_path, next_on) {
                tracing::warn!("Registro actualizado pero config no se guardo: {e}");
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
/// Fase 1.3: ya no hay `TRAY_DATA` static — el thread recibe el
/// `Arc<AppState>` y se lo cuelga a su ventana via `GWLP_USERDATA`.
pub fn init_tray(state: Arc<AppState>) {
    std::thread::spawn(move || unsafe {
        run_tray_loop(state);
    });
}

/// Inicia el icono de bandeja. Requiere un message pump en el thread.
///
/// # Safety
///
/// Esta funcion es `unsafe` porque llama a multiples funciones inseguras
/// de la API de Windows (RegisterClassExW, CreateWindowExW, Shell_NotifyIconW,
/// GetMessageW). Requiere que se llame desde un thread con message pump.
unsafe fn run_tray_loop(state: Arc<AppState>) {
    // SAFETY: GetModuleHandleW(None) devuelve el handle del modulo actual.
    // Es seguro de llamar. Si falla, usamos un HMODULE nulo (aceptable para
    // registrar la clase de ventana).
    let hinstance =
        GetModuleHandleW(None).unwrap_or(windows::Win32::Foundation::HMODULE(std::ptr::null_mut()));

    // === Icono: recurso embebido del propio exe (build.rs via windres) ===
    // SAFETY: LoadImageW sobre recursos locales del mostrador es seguro; en
    // el peor caso cae al icono del sistema definido (comportamiento previo).
    let hicon = match LoadImageW(
        windows::Win32::Foundation::HINSTANCE(hinstance.0),
        // MAKEINTRESOURCE(1): id del ico generado en el build.rs
        windows::core::PCWSTR(1usize as *const u16),
        IMAGE_ICON,
        0,
        0,
        LR_DEFAULTSIZE | LR_SHARED,
    ) {
        Ok(handle) => HICON(handle.0),
        Err(e) => {
            tracing::debug!("Icono embebido no disponible ({e}); icono del sistema.");
            LoadIconW(None, IDI_APPLICATION).unwrap_or_else(|_| HICON(std::ptr::null_mut()))
        }
    };

    let class_name = windows::core::w!("QuickActionsTrayWnd");
    let wnd_class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        hInstance: windows::Win32::Foundation::HINSTANCE(hinstance.0),
        lpszClassName: class_name,
        lpfnWndProc: Some(tray_wnd_proc),
        hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(None, IDC_ARROW).unwrap_or(
            windows::Win32::UI::WindowsAndMessaging::HCURSOR(std::ptr::null_mut()),
        ),
        hIcon: hicon,
        hIconSm: hicon,
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

    // Fase 1.3: cablear el estado al wnd_proc via GWLP_USERDATA (los wnd
    // procs extern "system" no pueden capturar). Leak a proposito: el tray
    // vive hasta el exit del proceso; el refcount extra queda sin liberar
    // y el sistema lo recoge al morir el proceso.
    let leaked = Box::leak(Box::new(TrayData {
        state: Arc::clone(&state),
    }));
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, (leaked as *const TrayData) as isize);

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
                    if let Some(data) = tray_state(hwnd) {
                        let state = &data.state;
                        let _ = state
                            .ui_tx
                            .send(UiCommand::ShowHelp(crate::actions::help_text(
                                state.hook_modifier,
                                state.hook_capsmode,
                            )));
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
                        if let Some(data) = tray_state(hwnd) {
                            let state = &data.state;
                            let _ =
                                state
                                    .ui_tx
                                    .send(UiCommand::ShowHelp(crate::actions::help_text(
                                        state.hook_modifier,
                                        state.hook_capsmode,
                                    )));
                        }
                    }
                    ID_TRAY_CONFIG => {
                        if let Some(data) = tray_state(hwnd) {
                            // U4: abrir la pestana Ajustes de la GUI (no
                            // un editor de texto);
                            let _ = data.state.ui_tx.send(UiCommand::ShowAjustes);
                            data.state.repaint();
                        }
                    }
                    ID_TRAY_AUTOSTART => {
                        if let Some(data) = tray_state(hwnd) {
                            toggle_autostart(&data.state);
                        }
                    }
                    ID_TRAY_PAUSE => {
                        if let Some(data) = tray_state(hwnd) {
                            toggle_pause(&data.state);
                        }
                    }
                    id if (ID_MODELO_BASE..ID_MODELO_BASE + MAX_MODELOS_MENU as u32)
                        .contains(&id) =>
                    {
                        if let Some(data) = tray_state(hwnd) {
                            let models = data
                                .state
                                .ollama_models
                                .read()
                                .ok()
                                .map(|m| m.clone())
                                .unwrap_or_default();
                            if let Some(model) = models
                                .get((id - ID_MODELO_BASE) as usize)
                                .map(String::as_str)
                            {
                                set_ollama_model(&data.state, model);
                                data.state.spawn_models_refresh();
                            }
                        }
                    }
                    id if (ID_ATAJOS_BASE + 1..=ID_ATAJOS_NINGUNO).contains(&id) => {
                        if let Some(data) = tray_state(hwnd) {
                            handle_atajos_menu(&data.state, id);
                        }
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

    let Some(data) = tray_state(hwnd) else {
        tracing::warn!("Tray sin estado (GWLP_USERDATA vacio).");
        return;
    };
    let state = &data.state;

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
    let paused = state.suspendido.load(Ordering::Relaxed);
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

    // === Submenu "Atajos" (U3, pausa modular per-atajo) ===
    let hotkeys_cfg = state.config.read().ok().map(|c| c.hotkeys.clone());

    match CreatePopupMenu() {
        Err(_) => {
            tracing::warn!("CreatePopupMenu fallo para el submenu de atajos.");
        }
        Ok(hmenu_atajos) => {
            if let Some(hk) = hotkeys_cfg {
                for (i, (cmd, label, desc)) in crate::hotkeys::HOTKEY_ACTIONS.iter().enumerate() {
                    let on = crate::hotkeys::is_hotkey_enabled(*cmd, &hk);
                    let flags = if on {
                        windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS(
                            MF_STRING.0 | MF_CHECKED.0,
                        )
                    } else {
                        MF_STRING
                    };
                    let text = windows::core::HSTRING::from(format!("{desc} ({label})"));
                    let _ = AppendMenuW(
                        hmenu_atajos,
                        flags,
                        (ID_ATAJOS_BASE + 1 + i as u32) as usize,
                        &text,
                    );
                }
                let _ = AppendMenuW(hmenu_atajos, MF_SEPARATOR, 0, windows::core::w!(""));
                let _ = AppendMenuW(
                    hmenu_atajos,
                    MF_STRING,
                    ID_ATAJOS_TODOS as usize,
                    windows::core::w!("Activar todos"),
                );
                let _ = AppendMenuW(
                    hmenu_atajos,
                    MF_STRING,
                    ID_ATAJOS_NINGUNO as usize,
                    windows::core::w!("Desactivar todos"),
                );
            }

            let _ = AppendMenuW(
                hmenu,
                windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS(
                    MF_STRING.0 | windows::Win32::UI::WindowsAndMessaging::MF_POPUP.0,
                ),
                hmenu_atajos.0 as usize, // MF_POPUP: uidNew = HMENU del submenu
                windows::core::w!("Atajos"),
            );
        }
    }

    // === Submenu "Modelo" (P3) ===
    let current_model = state
        .config
        .read()
        .ok()
        .map(|c| c.ollama.model.clone())
        .unwrap_or_default();

    let models = state
        .ollama_models
        .read()
        .ok()
        .map(|m| m.clone())
        .unwrap_or_default();

    match CreatePopupMenu() {
        Err(_) => {
            tracing::warn!("CreatePopupMenu fallo para el submenu de modelo.");
        }
        Ok(hmenu_model) => {
            if models.is_empty() {
                // Cache vacia: refetch en background; el proximo click
                // tendra la lista (no bloquear el thread del tray).
                state.spawn_models_refresh();
                let loading = windows::core::HSTRING::from("(buscando modelos...)");
                let _ = AppendMenuW(
                    hmenu_model,
                    windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS(
                        MF_STRING.0 | windows::Win32::UI::WindowsAndMessaging::MF_DISABLED.0,
                    ),
                    0,
                    &loading,
                );
            } else {
                for (i, model) in models.iter().take(MAX_MODELOS_MENU).enumerate() {
                    let flags = if *model == current_model {
                        windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS(
                            MF_STRING.0 | MF_CHECKED.0,
                        )
                    } else {
                        MF_STRING
                    };
                    let text = windows::core::HSTRING::from(model);
                    let _ = AppendMenuW(
                        hmenu_model,
                        flags,
                        (ID_MODELO_BASE + i as u32) as usize,
                        &text,
                    );
                }
            }

            // Label del root: estado actual
            let root_label = windows::core::HSTRING::from(format!(
                "Modelo: {}",
                if current_model.is_empty() {
                    "?"
                } else {
                    &current_model
                }
            ));
            let _ = AppendMenuW(
                hmenu,
                windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS(
                    MF_STRING.0 | windows::Win32::UI::WindowsAndMessaging::MF_POPUP.0,
                ),
                hmenu_model.0 as usize, // MF_POPUP: uidNew = HMENU del submenu
                &root_label,
            );
        }
    }

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
            let _ = state
                .ui_tx
                .send(UiCommand::ShowHelp(crate::actions::help_text(
                    state.hook_modifier,
                    state.hook_capsmode,
                )));
        }
        ID_TRAY_CONFIG => {
            // U4: abrir la pestana Ajustes de la GUI (no un editor);
            let _ = state.ui_tx.send(UiCommand::ShowAjustes);
            state.repaint();
        }
        ID_TRAY_AUTOSTART => {
            toggle_autostart(state);
        }
        ID_TRAY_PAUSE => {
            toggle_pause(state);
        }
        id if (ID_MODELO_BASE..ID_MODELO_BASE + MAX_MODELOS_MENU as u32).contains(&id) => {
            let models = state
                .ollama_models
                .read()
                .ok()
                .map(|m| m.clone())
                .unwrap_or_default();
            if let Some(model) = models
                .get((id - ID_MODELO_BASE) as usize)
                .map(String::as_str)
            {
                set_ollama_model(state, model);
                state.spawn_models_refresh();
            }
        }
        id if (ID_ATAJOS_BASE + 1..=ID_ATAJOS_NINGUNO).contains(&id) => {
            handle_atajos_menu(state, id);
        }
        ID_TRAY_EXIT => {
            tracing::info!("Saliendo via tray menu...");
            PostQuitMessage(0);
            std::process::exit(0);
        }
        _ => {}
    }
}
