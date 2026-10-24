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
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex, RwLock};

    use quick_actions::clipboard::ClipboardManager;
    use quick_actions::config::Config;
    use quick_actions::history::History;
    use quick_actions::hotkeys::{init_sender, start_listener, HotkeyCommand};
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

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .init();

    tracing::info!("========================================");
    tracing::info!("Iniciando Quick Actions v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("Exe dir: {:?}", exe_dir);

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

    // Flag compartido de pausa de atajos (tray <-> hook) — P2.
    let suspended = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // === 4. Inicializar clipboard e historial ===
    let clipboard = match ClipboardManager::new() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            let msg = format!("Error accediendo al portapapeles:\n\n{e}");
            tracing::error!("{msg}");
            show_error_dialog(&msg);
            std::process::exit(1);
        }
    };

    // === 4. Inicializar clipboard e historial ===
    // Cache de modelos para el submenu del tray (llena en background — P3).
    let ollama_models = Arc::new(Mutex::new(Vec::<String>::new()));

    {
        let models_cache = Arc::clone(&ollama_models);
        let config_snapshot = config.read().ok().map(|c| c.ollama.clone());
        if let Some(ollama_config) = config_snapshot {
            std::thread::spawn(move || {
                let runtime = quick_actions::runtime::global();
                match runtime.block_on(quick_actions::ollama::OllamaClient::list_models(
                    &ollama_config,
                )) {
                    Ok(models) => {
                        tracing::info!("Ollama: {} modelos instalados", models.len());
                        *models_cache.lock().unwrap_or_else(|e| e.into_inner()) = models;
                    }
                    Err(e) => {
                        tracing::warn!("No se pudo listar modelos de Ollama: {e}");
                    }
                }
            });
        }
    }

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

    // === 5. Iniciar tray icon (thread dedicado) ===
    let ui_tx_for_tray = ui_channels.tx.clone();
    let config_path_for_tray = exe_dir.join("config.toml");
    init_tray(
        ui_tx_for_tray,
        config_path_for_tray.clone(),
        Arc::clone(&suspended),
        Arc::clone(&config),
        Arc::clone(&ollama_models),
    );
    tracing::info!("Tray icon iniciado.");

    // === 5.5. Check de updates (background, no bloquea) — P4 ===
    if let Some(app_config) = config.read().ok().map(|c| c.app.clone()) {
        quick_actions::update_check::spawn_check(&app_config, ui_channels.tx.clone());
    }

    // === 6. Iniciar hook de teclado (thread dedicado) ===
    init_sender(hotkey_tx);
    quick_actions::hotkeys::init_suspended(Arc::clone(&suspended));
    // P7: modificador + capsmode desde config (leer antes del listener).
    let hk_cfg = config
        .read()
        .ok()
        .map(|c| c.hotkeys.clone())
        .unwrap_or_default();
    quick_actions::hotkeys::init_caps(
        quick_actions::hotkeys::Modifier::from_config(&hk_cfg.modifier),
        quick_actions::hotkeys::CapsMode::from_config(&hk_cfg.capsmode),
    );
    start_listener();
    tracing::info!(
        "Hotkeys iniciados. Modificador '{}' + G/T/R/Enter/V/H listos.",
        hk_cfg.modifier
    );

    // === 7. Iniciar eframe en el thread principal (requerido por Windows) ===
    // Pasamos el hotkey_rx y los datos compartidos al App.
    // El App procesara hotkeys en cada frame (cada 100ms).
    let app = QuickActionsApp::new(
        ui_channels.rx,
        hotkey_rx,
        Arc::clone(&config),
        Arc::clone(&clipboard),
        Arc::clone(&history),
        ui_channels.tx.clone(),
        Arc::clone(&ollama_models),
        config_path_for_tray.clone(),
    );

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1.0, 1.0])
            .with_min_inner_size([400.0, 300.0])
            .with_position([-10000.0, -10000.0])
            .with_taskbar(false)
            .with_visible(false),
        ..Default::default()
    };

    let _ = eframe::run_native("QuickActions", options, Box::new(|_cc| Ok(Box::new(app))));
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
