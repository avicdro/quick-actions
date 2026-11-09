//! Handlers de comandos de hotkey (corregir, traducir, etc.).

use crate::app_state::AppState;
use crate::error::{AppError, Result};
use crate::hotkeys::{CapsMode, HotkeyCommand, Modifier};
use crate::key_sim::KeySimulator;
use crate::ollama::OllamaClient;
use crate::prompts;
use crate::text_utils;
use crate::ui_protocol::{HistoryData, ResultData, StatusMsg, UiCommand};

/// Texto de ayuda mostrado con el hotkey de ayuda o click en tray.
///
/// El prefix del modificador es dinamico (P7): "CapsLock" o "Alt" segun
/// la config (fase 1.3: ya no hay static `MODIFIER`; se pasa como param).
pub fn help_text(modifier: Modifier, caps_mode: CapsMode) -> String {
    let m = modifier.name();
    let caps_note = if modifier == Modifier::CapsLock && caps_mode == CapsMode::Toggle {
        "  (modo toggle: tap de CapsLock latchea; la proxima tecla dispara)\n"
    } else {
        ""
    };

    format!(
        "AI SUITE — Asistente de IA\n\
         =======================================\n\n\
         Selecciona texto en cualquier app y pulsa:\n\n\
         {m} + G      -> Corregir ortografia (pega el resultado)\n\
         {m} + T      -> Traducir (auto o [idioma])\n\
         {m} + R      -> Resumir (o [formato])\n\
         {m} + Enter  -> Generar respuesta\n\
         {m} + V      -> Ver historial de correcciones\n\
         {m} + H      -> Mostrar esta ayuda\n\n{caps_note}\
         =======================================\n\n\
         DOS MODOS:\n\
           - Corregir (G): reemplaza el texto pegando el resultado.\n\
             Si no hay seleccion, intenta Ctrl+A (todo el input).\n\
           - Traducir/Resumir/Generar (T/R/Enter): muestra el\n\
             resultado en una ventana sin modificar el texto.\n\n\
         =======================================\n\n\
         CONTEXTO OPCIONAL:\n\
           Escribe [instrucciones] al inicio del texto.\n\
           Ej: [frances] hola mundo\n\
           Ej: [5 vinetas] texto largo..."
    )
}

/// Procesa un comando de hotkey.
///
/// La config se lee del `RwLock` compartido al inicio del handler: el modelo
/// vigente es el que el usuario haya seleccionado en el tray (hot-reload P3).
/// Fase 1.3: el estado llega empaquetado en `AppState` (antes: 5 params
/// sueltos + `ui::request_repaint` global).
pub fn handle_command(cmd: HotkeyCommand, state: &AppState) -> Result<()> {
    // U3, pausa modular: atajo individualmente desactivado en la config.
    // La pausa total ([hotkeys] enabled / tray) se resuelve antes en el hook.
    let Some(config) = state.config_snapshot() else {
        return Err(AppError::msg("Error de lock en config (poisoned)"));
    };
    if !crate::hotkeys::is_hotkey_enabled(cmd, &config.hotkeys) {
        let msg = format!(
            "Atajo '{}+{}' desactivado (Ajustes)",
            state.hook_modifier.name(),
            cmd.key_label()
        );
        let _ = state.ui_tx.send(crate::ui_protocol::UiCommand::SetStatus(
            crate::ui_protocol::StatusMsg::Notice(msg),
        ));
        state.repaint();
        return Ok(());
    }

    let ollama = OllamaClient::new(config.ollama.clone(), (*state.http).clone());
    let result = match cmd {
        HotkeyCommand::Correct => handle_correct(state, ollama),
        HotkeyCommand::Translate => handle_translate(state, ollama),
        HotkeyCommand::Summarize => handle_summarize(state, ollama),
        HotkeyCommand::Generate => handle_generate(state, ollama),
        HotkeyCommand::ShowHistory => handle_show_history(state),
        HotkeyCommand::ShowHelp => {
            let _ = state
                .ui_tx
                .send(crate::ui_protocol::UiCommand::ShowHelp(help_text(
                    state.hook_modifier,
                    state.hook_capsmode,
                )));
            state.repaint();
            Ok(())
        }
    };

    // Si hubo error, mostrarlo sutilmente en la barra de estado
    if let Err(ref e) = result {
        // v0.3.0-hotfix: el error SOLO iba al toast; sin esta linea el log
        // no decia NADA cuando la captura/pegado fallaba (diagnostico a
        // ciegas). Error persistente en log + toast para el usuario.
        tracing::error!("accion {cmd:?} fallo: {e}");
        let msg = format!("Error: {e}");
        let _ = state.ui_tx.send(crate::ui_protocol::UiCommand::SetStatus(
            crate::ui_protocol::StatusMsg::Error(msg),
        ));
        state.repaint();
    }

    result
}

/// Bloque P (v0.6): carpeta de prompts personalizados resuelta desde la
/// config (`[prompts] dir`). `None` = override desactivado (dir vacio)
/// o config ilegible: los wrappers de `prompts` caen al default.
fn prompts_dir(state: &AppState) -> Option<std::path::PathBuf> {
    let raw = state.config.read().ok()?.prompts.dir.clone();
    let cfg_dir = state
        .config_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    crate::prompts::resolve_dir(&cfg_dir, &raw)
}

fn handle_correct(state: &AppState, ollama: OllamaClient) -> Result<()> {
    // v0.3.0-hotfix: capturar el texto ANTES de mostrar el toast. Si el
    // overlay nace antes del Ctrl+C, puede llevarse el foco justo en la
    // captura (el WS_EX_NOACTIVATE del toast es el fix de raiz; este orden
    // es el cinturon extra: durante la captura ni siquiera hay overlay).
    let texto = obtener_texto_para_corregir(state)?;
    if texto.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
        return Ok(());
    }

    let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Loading(
        "Corrigiendo...".to_string(),
    )));
    state.repaint();

    let (contexto, texto_limpio) = text_utils::extraer_contexto(&texto);
    // Bloque P: corregir.md personalizado si existe; default si no.
    let sys_prompt = prompts::corregir(prompts_dir(state).as_deref(), &contexto);

    tracing::info!("Corrigiendo texto ({} chars)...", texto_limpio.len());
    let resultado =
        crate::runtime::global().block_on(ollama.generate(&sys_prompt, &texto_limpio))?;

    if resultado.is_empty() {
        tracing::warn!("Ollama devolvio resultado vacio.");
        let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
        return Ok(());
    }

    tracing::info!(
        "Correccion completada ({} chars). Pegando...",
        resultado.len()
    );

    {
        let mut hist = state
            .history
            .lock()
            .map_err(|_| crate::error::AppError::msg("Error de lock en historial"))?;
        hist.push(texto.clone(), resultado.clone());
        let _ = hist.save();
    }

    state.clipboard.set_text(&resultado)?;
    KeySimulator::paste(state.hook_modifier)?;

    // v0.3.2: feedback visible tras pegar. Antes se hacia Clear: si el
    // destino es de solo-lectura (p. ej. la respuesta de Gemini en una
    // web) el Ctrl+V no escribe nada y el usuario no veia NADA — con el
    // corregido YA en el portapapeles pero sin saberlo. Toast Notice:
    // pegado + red de seguridad por Ctrl+V (el original queda en el
    // historial de la app).
    let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Notice(
        "Corregido — pegado (si no se pego, esta en tu portapapeles: Ctrl+V)".to_string(),
    )));
    state.repaint();
    Ok(())
}

fn handle_translate(state: &AppState, ollama: OllamaClient) -> Result<()> {
    // v0.3.0-hotfix: captura antes del toast (mismo motivo que correct).
    let texto = obtener_texto_para_corregir(state)?;
    if texto.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
        return Ok(());
    }

    let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Loading(
        "Traduciendo...".to_string(),
    )));
    state.repaint();

    let (idioma, texto_limpio) = text_utils::extraer_contexto(&texto);
    // Bloque P: traducir.md personalizado si existe; default si no.
    let sys_prompt = prompts::traducir(prompts_dir(state).as_deref(), &idioma);

    tracing::info!("Traduciendo ({} chars)...", texto_limpio.len());
    let resultado =
        crate::runtime::global().block_on(ollama.generate(&sys_prompt, &texto_limpio))?;

    if resultado.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
        return Ok(());
    }

    let titulo = if idioma.is_empty() {
        "Traduccion".to_string()
    } else {
        format!("Traduccion al {idioma}")
    };

    // C6 colateral (bug): translate era el UNICO sin Clear antes de abrir
    // la ventana — el spinner "Traduciendo..." quedaba visible para siempre
    // junto al resultado (Loading no expira).
    let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
    let _ = state.ui_tx.send(UiCommand::ShowResult(ResultData {
        titulo,
        original: texto_limpio,
        resultado,
    }));
    state.repaint();

    Ok(())
}

fn handle_summarize(state: &AppState, ollama: OllamaClient) -> Result<()> {
    // v0.3.0-hotfix: captura antes del toast (mismo motivo que correct).
    let texto = obtener_texto_para_corregir(state)?;
    if texto.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
        return Ok(());
    }

    let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Loading(
        "Resumiendo...".to_string(),
    )));
    state.repaint();

    let (formato, texto_limpio) = text_utils::extraer_contexto(&texto);
    // Bloque P: resumir.md personalizado si existe; default si no.
    let sys_prompt = prompts::resumir(prompts_dir(state).as_deref(), &formato);

    tracing::info!("Resumiendo ({} chars)...", texto_limpio.len());
    let resultado =
        crate::runtime::global().block_on(ollama.generate(&sys_prompt, &texto_limpio))?;

    if resultado.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
        return Ok(());
    }

    let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
    let _ = state.ui_tx.send(UiCommand::ShowResult(ResultData {
        titulo: "Resumen".to_string(),
        original: texto_limpio,
        resultado,
    }));
    state.repaint();

    Ok(())
}

fn handle_generate(state: &AppState, ollama: OllamaClient) -> Result<()> {
    // v0.3.0-hotfix: captura antes del toast (mismo motivo que correct).
    let texto = obtener_texto_para_corregir(state)?;
    if texto.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
        return Ok(());
    }

    let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Loading(
        "Generando respuesta...".to_string(),
    )));
    state.repaint();

    let (contexto, prompt) = text_utils::extraer_contexto(&texto);
    // Bloque P: generar.md personalizado si existe; default si no.
    let sys_prompt = prompts::generar(prompts_dir(state).as_deref(), &contexto);

    tracing::info!("Generando respuesta ({} chars)...", prompt.len());
    let resultado = crate::runtime::global().block_on(ollama.generate(&sys_prompt, &prompt))?;

    if resultado.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
        return Ok(());
    }

    let _ = state.ui_tx.send(UiCommand::SetStatus(StatusMsg::Clear));
    let _ = state.ui_tx.send(UiCommand::ShowResult(ResultData {
        titulo: "Respuesta".to_string(),
        original: prompt,
        resultado,
    }));
    state.repaint();

    Ok(())
}

fn handle_show_history(state: &AppState) -> Result<()> {
    let hist = state
        .history
        .lock()
        .map_err(|_| crate::error::AppError::msg("Error de lock en historial"))?;

    let entries = crate::ui_protocol::history_views(hist.entries());

    let _ = state.ui_tx.send(UiCommand::ShowHistory(HistoryData {
        entries,
        selected: 0,
    }));
    state.repaint();

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers de obtencion de texto
// ---------------------------------------------------------------------------

/// v0.3.0-hotfix: diagnóstico de foco en la captura — si el Ctrl+C se
/// manda con quick-actions en primer plano (en vez del editor del
/// usuario), el portapapeles queda vacio y no hay nada que corregir.
#[cfg(windows)]
fn log_foreground_window(tag: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        let mut cls = [0u16; 128];
        let nc = GetClassNameW(hwnd, &mut cls);
        let class = String::from_utf16_lossy(&cls[..nc.max(0) as usize]);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let own = if pid == std::process::id() {
            "NUESTRA"
        } else {
            "otro-proceso"
        };
        tracing::debug!(
            "captura({tag}): foreground hwnd=0x{:X} pid={pid}({own}) class='{class}' title='{title}'",
            hwnd.0 as usize
        );
    }
}

/// v0.3.1-hotfix: espera ACTIVA del texto copiado. Chrome y otras apps
/// escriben el portapapeles ASYNC: un sleep fijo de 150ms quedaba corto
/// a rachas (0 chars → fallback select_all que cambiaba la seleccion
/// del usuario por "toda la pagina"). Poll cada 50ms hasta `max_ms`;
/// devuelve el texto (posiblemente vacio si la app no copio nada).
fn clipboard_wait_text(state: &AppState, max_ms: u64) -> Result<String> {
    let mut waited = 0u64;
    loop {
        let texto = state.clipboard.get_text()?;
        if !texto.trim().is_empty() || waited >= max_ms {
            return Ok(texto);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        waited += 50;
    }
}

fn obtener_texto_para_corregir(state: &AppState) -> Result<String> {
    state.clipboard.backup()?;
    state.clipboard.clear()?;
    log_foreground_window("antes-de-copy");
    tracing::debug!("captura: backup+clear ok; enviando Ctrl+C");

    KeySimulator::copy(state.hook_modifier)?;
    let texto = clipboard_wait_text(state, 800)?;
    log_foreground_window("despues-de-copy");
    tracing::debug!("captura: clipboard tras copy = {} chars", texto.len());

    if texto.trim().is_empty() {
        tracing::debug!("captura: copy vacio -> fallback select_all + copy");
        state.clipboard.restore()?;
        KeySimulator::select_all(state.hook_modifier)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        state.clipboard.clear()?;
        KeySimulator::copy(state.hook_modifier)?;
        let texto = clipboard_wait_text(state, 800)?;
        tracing::debug!("captura: clipboard tras fallback = {} chars", texto.len());

        if texto.trim().is_empty() {
            tracing::warn!("No hay texto para corregir.");
            state.clipboard.restore()?;
            return Ok(String::new());
        }

        if texto.len() > 3000 {
            state.clipboard.restore()?;
            KeySimulator::end()?;
            std::thread::sleep(std::time::Duration::from_millis(50));
            KeySimulator::shift_home()?;
            std::thread::sleep(std::time::Duration::from_millis(100));
            state.clipboard.clear()?;
            KeySimulator::copy(state.hook_modifier)?;
            let texto = clipboard_wait_text(state, 800)?;

            if texto.trim().is_empty() || texto.len() > 1000 {
                tracing::warn!("Texto muy largo. Selecciona manualmente.");
                state.clipboard.restore()?;
                return Ok(String::new());
            }

            state.clipboard.restore()?;
            return Ok(texto);
        }

        state.clipboard.restore()?;
        return Ok(texto);
    }

    state.clipboard.restore()?;
    Ok(texto)
}
