//! Handlers de comandos de hotkey (corregir, traducir, etc.).

use crate::app_state::AppState;
use crate::error::{AppError, Result};
use crate::hotkeys::{CapsMode, HotkeyCommand, Modifier};
use crate::key_sim::KeySimulator;
use crate::ollama::OllamaClient;
use crate::prompts;
use crate::text_utils;
use crate::ui_protocol::{HistoryData, HistoryEntryView, ResultData, UiCommand};

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
        let _ = state
            .ui_tx
            .send(crate::ui_protocol::UiCommand::SetStatus(msg));
        state.repaint();
        return Ok(());
    }

    let ollama = OllamaClient::new(config.ollama.clone());
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
        let msg = format!("Error: {e}");
        let _ = state
            .ui_tx
            .send(crate::ui_protocol::UiCommand::SetStatus(msg));
        state.repaint();
    }

    result
}

fn handle_correct(state: &AppState, ollama: OllamaClient) -> Result<()> {
    let _ = state
        .ui_tx
        .send(UiCommand::SetStatus("Corrigiendo...".to_string()));
    state.repaint();

    let texto = obtener_texto_para_corregir(state)?;
    if texto.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let (contexto, texto_limpio) = text_utils::extraer_contexto(&texto);
    let sys_prompt = prompts::sys_prompt_corregir(&contexto);

    tracing::info!("Corrigiendo texto ({} chars)...", texto_limpio.len());
    let resultado =
        crate::runtime::global().block_on(ollama.generate(&sys_prompt, &texto_limpio))?;

    if resultado.is_empty() {
        tracing::warn!("Ollama devolvio resultado vacio.");
        let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
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

    let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
    Ok(())
}

fn handle_translate(state: &AppState, ollama: OllamaClient) -> Result<()> {
    let _ = state
        .ui_tx
        .send(UiCommand::SetStatus("Traduciendo...".to_string()));
    state.repaint();

    let texto = obtener_texto_para_corregir(state)?;
    if texto.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let (idioma, texto_limpio) = text_utils::extraer_contexto(&texto);
    let sys_prompt = prompts::sys_prompt_traducir(&idioma);

    tracing::info!("Traduciendo ({} chars)...", texto_limpio.len());
    let resultado =
        crate::runtime::global().block_on(ollama.generate(&sys_prompt, &texto_limpio))?;

    if resultado.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let titulo = if idioma.is_empty() {
        "Traduccion".to_string()
    } else {
        format!("Traduccion al {idioma}")
    };

    let _ = state.ui_tx.send(UiCommand::ShowResult(ResultData {
        titulo,
        original: texto_limpio,
        resultado,
    }));
    state.repaint();

    Ok(())
}

fn handle_summarize(state: &AppState, ollama: OllamaClient) -> Result<()> {
    let _ = state
        .ui_tx
        .send(UiCommand::SetStatus("Resumiendo...".to_string()));
    state.repaint();

    let texto = obtener_texto_para_corregir(state)?;
    if texto.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let (formato, texto_limpio) = text_utils::extraer_contexto(&texto);
    let sys_prompt = prompts::sys_prompt_resumir(&formato);

    tracing::info!("Resumiendo ({} chars)...", texto_limpio.len());
    let resultado =
        crate::runtime::global().block_on(ollama.generate(&sys_prompt, &texto_limpio))?;

    if resultado.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
    let _ = state.ui_tx.send(UiCommand::ShowResult(ResultData {
        titulo: "Resumen".to_string(),
        original: texto_limpio,
        resultado,
    }));
    state.repaint();

    Ok(())
}

fn handle_generate(state: &AppState, ollama: OllamaClient) -> Result<()> {
    let _ = state
        .ui_tx
        .send(UiCommand::SetStatus("Generando respuesta...".to_string()));
    state.repaint();

    let texto = obtener_texto_para_corregir(state)?;
    if texto.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let (contexto, prompt) = text_utils::extraer_contexto(&texto);
    let sys_prompt = prompts::sys_prompt_generar(&contexto);

    tracing::info!("Generando respuesta ({} chars)...", prompt.len());
    let resultado = crate::runtime::global().block_on(ollama.generate(&sys_prompt, &prompt))?;

    if resultado.is_empty() {
        let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let _ = state.ui_tx.send(UiCommand::SetStatus(String::new()));
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

    let entries: Vec<HistoryEntryView> = hist
        .entries()
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let preview = if e.original.len() > 60 {
                format!("{}...", &e.original[..60])
            } else {
                e.original.clone()
            };
            HistoryEntryView {
                index: i + 1,
                preview,
                fecha: e.fecha.clone(),
                original: e.original.clone(),
            }
        })
        .collect();

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

fn obtener_texto_para_corregir(state: &AppState) -> Result<String> {
    state.clipboard.backup()?;
    state.clipboard.clear()?;

    KeySimulator::copy(state.hook_modifier)?;
    std::thread::sleep(std::time::Duration::from_millis(150));
    let texto = state.clipboard.get_text()?;

    if texto.trim().is_empty() {
        state.clipboard.restore()?;
        KeySimulator::select_all(state.hook_modifier)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        state.clipboard.clear()?;
        KeySimulator::copy(state.hook_modifier)?;
        std::thread::sleep(std::time::Duration::from_millis(150));
        let texto = state.clipboard.get_text()?;

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
            std::thread::sleep(std::time::Duration::from_millis(150));
            let texto = state.clipboard.get_text()?;

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
