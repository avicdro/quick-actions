//! quick-actions — Suite de atajos globales para IA y productividad.
//!
//! Reemplaza scripts de AutoHotkey con un unico ejecutable Rust.
//! El bin solo es compilable para Windows (`--target x86_64-pc-windows-gnu`);
//! los modulos cross-platform de la lib (config, history, ...) si corren sus
//! tests en Linux.

// En Windows: no mostrar consola. Es una app de fondo con tray icon.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    windows_main();
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "quick-actions es una app para Windows, mas alla de la lib que compila aca. \
         Build: cargo build --release --target x86_64-pc-windows-gnu"
    );
}

#[cfg(windows)]
fn windows_main() {
    use eframe::egui;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex, RwLock};

    use quick_actions::app_state::AppState;
    use quick_actions::clipboard::ClipboardManager;
    use quick_actions::config::Config;
    use quick_actions::history::History;
    use quick_actions::hotkeys::{CapsMode, HotkeyCommand, Modifier};
    use quick_actions::tray::init_tray;
    use quick_actions::ui::{QuickActionsApp, UiChannels};

    // === 0. Panic hook — registrar crashes de cualquier thread ===
    //
    // El release no usa `panic = "abort"` a proposito: un panic en un thread
    // (tray, hotkey, worker) no mata el proceso, pero antes nadie lo veia.
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    install_panic_hook(&exe_dir);

    // === 1. Logging a archivo (para debug si crashea) ===
    let log_dir = exe_dir.join("logs");
    std::fs::create_dir_all(&log_dir).unwrap_or_default();

    let file_appender = tracing_appender::rolling::daily(&log_dir, "quick-actions.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // N1: `wgpu_core::instance` loguea el ADAPTER elegido al arrancar (INFO)
    // — ahi se ve de verdad en que GPU pinta la app (iGPU vs dGPU; el
    // nvidia-smi de Windows NO lista procesos Vulkan/graficos y mintio en L9).
    // El spam `Device::maintain: waiting for submission` (wgpu_core por
    // frame, ~140k lineas/dia) se silencia con un target especifico a warn,
    // de modo que la INFO de adapter sobrevive. RUST_LOG manda si existe
    // (p.ej. `RUST_LOG=wgpu_core=debug` para un diagnostico profundo).
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new("info,wgpu_core::device::resource=warn")
    });
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(filter)
        .init();

    tracing::info!("========================================");
    tracing::info!(
        "Iniciando Quick Actions {}",
        quick_actions::version_string()
    );
    tracing::info!("Exe dir: {:?}", exe_dir);

    // === 1.5. S1: instancia unica — ANTES de tocar tray/hook/GUI/config.
    // Si ya vive otra: enfoca su GUI (si esta abierta) y `return` — salir
    // limpio para que el WorkerGuard del logger flusha TODO al log.
    if !quick_actions::single_instance::acquire(&exe_dir) {
        return;
    }

    // === 2. Cargar o crear configuracion ===
    let config = match Config::load_or_create_default() {
        Ok(c) => {
            tracing::info!("Config cargada. Modelo: {}", c.ollama.model);
            // RwLock compartido: tray puede cambiar el modelo on-the-fly (P3)
            Arc::new(RwLock::new(c))
        }
        Err(e) => {
            let msg =
                format!("Error cargando configuracion:\n\n{e}\n\nCrea config.toml junto al exe.");
            tracing::error!("{msg}");
            show_error_dialog(&msg);
            std::process::exit(1);
        }
    };

    if !config.read().map(|c| c.hotkeys.enabled).unwrap_or(true) {
        tracing::warn!("Hotkeys desactivados en config.toml. Saliendo.");
        return;
    }

    // === 2.5. Auto-arranque: alinear registro con config ([app] autostart) ===
    quick_actions::autostart::sync_with_config(
        config.read().map(|c| c.app.autostart).unwrap_or(false),
    );

    // === 3. Crear canales ===
    let (hotkey_tx, hotkey_rx) = channel::<HotkeyCommand>();
    let ui_channels = UiChannels::new();

    // === U7: aftermath del auto-update — limpiar el binario previo y
    // avisar "Actualizado" (el marker lo escribe el updater antes de
    // reiniciar). Si el .prev sigue lockeado, se reintentara en el proximo
    // arranque (mejor esfuerzo, silencioso).
    {
        let prev = exe_dir.join("quick-actions.prev.exe");
        if prev.exists() {
            match std::fs::remove_file(&prev) {
                Ok(()) => tracing::info!("Auto-update: .prev.exe eliminado."),
                Err(e) => tracing::debug!("Auto-update: .prev.exe sigue lockeado: {e}"),
            }
        }
        let marker = exe_dir.join("update-applied.tmp");
        if let Ok(tag) = std::fs::read_to_string(&marker) {
            let _ = std::fs::remove_file(&marker);
            let tag = tag.trim();
            tracing::info!("Auto-update completado en el arranque previo: {tag}.");
            let _ = ui_channels
                .tx
                .send(quick_actions::ui_protocol::UiCommand::SetStatus(format!(
                    "Actualizado a {tag} ✓"
                )));
        }
    }

    // Flag compartido de pausa de atajos (tray <-> hook) — P2.
    let suspended = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // === 4. Inicializar clipboard, historial y AppState (fase 1.3) ===
    // El AppState empaqueta todo lo compartido: config hot-reloadable, ruta
    // del config.toml, pausa, clipboard, historial, cache de modelos, canal
    // UI y el repaint del UI (cablea el egui::Context en el primer frame).
    let clipboard = match ClipboardManager::new() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            let msg = format!("Error accediendo al portapapeles:\n\n{e}");
            tracing::error!("{msg}");
            show_error_dialog(&msg);
            std::process::exit(1);
        }
    };

    let history_max_entries = config
        .read()
        .map(|c| c.ui.history_max_entries)
        .unwrap_or(50);
    let history_path = exe_dir.join("history.json");
    let history = match History::load_or_create(&history_path, history_max_entries) {
        Ok(h) => Arc::new(Mutex::new(h)),
        Err(e) => {
            tracing::error!("Error cargando historial: {e}");
            // No es critico, seguir sin historial
            Arc::new(Mutex::new(History::empty(history_max_entries)))
        }
    };

    let config_path = exe_dir.join("config.toml");

    // Fase 1.3: modificador/capsmode APLICADOS — snapshot de la config de
    // arranque (el hook NO la relee en caliente; cambiarlos exige reinicio).
    let hk_cfg = config
        .read()
        .ok()
        .map(|c| c.hotkeys.clone())
        .unwrap_or_default();

    let state = Arc::new(AppState::new(
        Arc::clone(&config),
        config_path.clone(),
        Arc::clone(&suspended),
        Arc::clone(&clipboard),
        Arc::clone(&history),
        // Cache de modelos para el submenu del tray y Ajustes (P3); se
        // llena en background justo despues (spawn_models_refresh).
        Arc::new(std::sync::RwLock::new(Vec::<String>::new())),
        ui_channels.tx.clone(),
        Modifier::from_config(&hk_cfg.modifier),
        CapsMode::from_config(&hk_cfg.capsmode),
    ));

    // Cache de modelos: refetch en background (P3).
    state.spawn_models_refresh();

    // === 5. Iniciar tray icon (thread dedicado) ===
    init_tray(Arc::clone(&state));
    tracing::info!("Tray icon iniciado.");

    // === 5.5. Check de updates (background, no bloquea) — P4 ===
    if let Some(app_config) = state.config_snapshot().map(|c| c.app) {
        quick_actions::update_check::spawn_check(&app_config, ui_channels.tx.clone());
    }

    // === 6. Iniciar hook de teclado (thread dedicado) — fase 1.3: todo
    // inyectado, sin init_* ni statics (SENDER/SUSPENDED/MODIFIER/CAPS_MODE
    // eliminados; el modificador aplicado ya vive en AppState y en el
    // estado del thread del hook).
    quick_actions::hotkeys::start_listener(
        hotkey_tx,
        Arc::clone(&suspended),
        Modifier::from_config(&hk_cfg.modifier),
        CapsMode::from_config(&hk_cfg.capsmode),
        Arc::clone(&state.repaint),
    );
    tracing::info!(
        "Hotkeys iniciados. Modificador '{}' + G/T/R/Enter/V/H listos.",
        hk_cfg.modifier
    );

    // === 7. Iniciar eframe en el thread principal (requerido por Windows) ===
    // El App solo necesita los canales y el AppState compartido.
    //
    // `--show`: QA/uso manual — abre la ventana principal al arrancar
    // (simulate clic en el tray Ayuda) sin esperar a que nadie la pida.
    let show_on_start = std::env::args().any(|a| a == "--show");
    let app = QuickActionsApp::new(ui_channels.rx, hotkey_rx, Arc::clone(&state), show_on_start);

    // I2: icono de ventana/taskbar/Alt-Tab — extraido del recurso '1 ICON'
    // del propio exe (misma fuente unica que el tray). Sin esto, eframe
    // pinta su icono DEFAULT (la 'e' navy de eframe) en el boton de la
    // taskbar y en Alt-Tab.
    let window_icon = window_icon_from_resource();

    let viewport = {
        let mut vp = eframe::egui::ViewportBuilder::default()
            .with_inner_size([1.0, 1.0])
            .with_min_inner_size([480.0, 420.0])
            .with_position([-10000.0, -10000.0])
            // V2: SIN with_taskbar(false) — la ventana, cuando esta visible,
            // es una ventana estandar de Windows (taskbar + Alt-Tab).
            .with_visible(false);
        match window_icon {
            Some(icon) => vp = vp.with_icon(icon),
            None => {
                tracing::warn!("I2: icono de recurso no extraible; queda el default de eframe.")
            }
        }
        vp
    };

    let options = eframe::NativeOptions {
        // L9: renderizador wgpu — GPU INTEGRADA (LowPower) y sin OpenGL
        // (WGL): una UI trivial no tiene nada que hacer en la dGPU, y el
        // contexto GL era lo que despertaba al driver NVIDIA (flash negro
        // + toast del overlay "videojuego" al abrir la GUI). Con
        // Backends::PRIMARY (Vulkan/Metal/Dx12): nada de ANGLE/WGL.
        // Si solo hay una dGPU, wgpu usa esa y no empeora nada.
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            supported_backends: eframe::wgpu::Backends::PRIMARY,
            power_preference: eframe::wgpu::PowerPreference::LowPower,
            ..Default::default()
        },
        viewport,
        ..Default::default()
    };

    let state_for_cc = Arc::clone(&state);
    let _ = eframe::run_native(
        "QuickActions",
        options,
        // V6-fix: con with_visible(false) eframe NO lanza ni un frame hasta
        // que algo pide repaint (p.ej. el primer clic del tray) — todo el
        // cap de boot (tema + toolwindow) quedaba sin ejecutar y la ventana
        // nacia con APPWINDOW => boton en taskbar. Aqui cableamos el
        // contexto YA (hook/tray pueden despertar el UI desde t=0), pedimos
        // el primer frame y fijamos el titulo definitivo.
        Box::new(move |cc| {
            state_for_cc.set_ui_ctx(cc.egui_ctx.clone());
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Title("Quick Actions".to_owned()));
            cc.egui_ctx.request_repaint();
            Ok(Box::new(app))
        }),
    );
}

/// Registra panics de cualquier thread en `logs/panic.log`.
///
/// El hook es global y persiste hasta el cierre. Escribe directo al archivo
/// (no via tracing) para que funcione incluso si el logging fallo antes.
#[cfg(windows)]
fn install_panic_hook(exe_dir: &std::path::Path) {
    let panic_log_dir = exe_dir.join("logs");
    let _ = std::fs::create_dir_all(&panic_log_dir);
    let panic_log = panic_log_dir.join("panic.log");

    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        let backtrace = std::backtrace::Backtrace::force_capture();
        let line = format!(
            "[{}] PANIC en thread '{thread_name}': {info}\nbacktrace:\n{backtrace}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        );

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&panic_log)
        {
            use std::io::Write as _;
            let _ = writeln!(file, "{line}");
        }

        // Ademas el pipeline normal (logs/quick-actions.log)
        tracing::error!("PANIC en thread '{thread_name}': {info}");
        eprintln!("{line}");
    }));
}

/// Muestra un dialogo de error nativo de Windows (MessageBoxW).
#[cfg(windows)]
fn show_error_dialog(msg: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    unsafe {
        let _ = MessageBoxW(
            None,
            &HSTRING::from(msg),
            &HSTRING::from("Quick Actions — Error"),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// I2: icono RGBA de la ventana desde el recurso '1 ICON' del exe.
///
/// La cadena canonica del icono (S2) es: un SOLO recurso embebido ->
/// (a) tray via LoadImageW-resource (P6), (b) ventana/taskbar/Alt-Tab
/// via este getter + `with_icon`. Sin (b), eframe muestra su icono
/// default (la 'e' navy) en taskbar.
///
/// Mecanica: `LoadImageW(id=1, 128x128, sin LR_DEFAULTSIZE)` (el
/// LR_DEFAULTSIZE ignoraria cx/cy y usaria 32px del metrico) ->
/// `GetIconInfo` (copia las HBITMAPs) -> `GetDIBits` 32bpp con height
/// negativa (top-down): los iconos 32bpp llevan canal alfa recto ->
/// swap BGRA -> RGBA (egui espera alfa NO premultiplicado, que es como
/// viven los datos de icono).
///
/// Limpieza: las HBITMAPs de GetIconInfo son NUESTRAS (DeleteObject);
/// el HICON nace con LR_SHARED (no DestroyIcon) y el DC se libera.
#[cfg(windows)]
fn window_icon_from_resource() -> Option<eframe::egui::IconData> {
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetIconInfo, LoadImageW, ICONINFO, IMAGE_ICON, LR_SHARED,
    };

    const SIZE: i32 = 128; // tamano nativo del frame mayor del .ico

    unsafe {
        let hinstance = GetModuleHandleW(None).ok()?;
        let hinstance = HINSTANCE(hinstance.0);
        // MAKEINTRESOURCE(1): el id del recurso del build.rs.
        let hicon_raw = LoadImageW(
            hinstance,
            windows::core::PCWSTR(1usize as *const u16),
            IMAGE_ICON,
            SIZE,
            SIZE,
            LR_SHARED,
        )
        .ok()?;
        let hicon = windows::Win32::UI::WindowsAndMessaging::HICON(hicon_raw.0);

        let mut info = ICONINFO::default();
        GetIconInfo(hicon, &mut info).ok()?;
        let (hbm_color, hbm_mask) = (info.hbmColor, info.hbmMask);
        if hbm_color.0.is_null() {
            return None;
        }

        let hdc = GetDC(None);
        let outcome = (|| {
            // 1a pasada: dims reales del bitmap (con lpvbits = None).
            let mut bmi = BITMAPINFO::default();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            if GetDIBits(hdc, info.hbmColor, 0, 0, None, &mut bmi, DIB_RGB_COLORS) == 0 {
                return None;
            }
            let w = bmi.bmiHeader.biWidth.max(0);
            let h_abs = bmi.bmiHeader.biHeight.abs();
            if w == 0 || h_abs == 0 {
                return None;
            }

            // 2a pasada: top-down (height negativa), 32bpp BGRA.
            bmi.bmiHeader.biHeight = -h_abs;
            bmi.bmiHeader.biBitCount = 32u16;
            bmi.bmiHeader.biCompression = BI_RGB.0;
            let mut buf = vec![0u8; (w * h_abs * 4) as usize];
            let got = GetDIBits(
                hdc,
                info.hbmColor,
                0,
                h_abs as u32,
                Some(buf.as_mut_ptr().cast()),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            if got != h_abs {
                return None;
            }

            // BGRA (alfa RECTO en iconos 32bpp, no premul) -> RGBA.
            for px in buf.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            Some(eframe::egui::IconData {
                width: w as u32,
                height: h_abs as u32,
                rgba: buf,
            })
        })();

        let _ = ReleaseDC(None, hdc);
        let _ = DeleteObject(HGDIOBJ(hbm_color.0));
        let _ = DeleteObject(HGDIOBJ(hbm_mask.0));
        outcome
    }
}
