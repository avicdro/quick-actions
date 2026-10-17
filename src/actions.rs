//! Handlers de comandos de hotkey (corregir, traducir, etc.).

use std::sync::{Arc, Mutex};

use crate::clipboard::ClipboardManager;
use crate::config::Config;
use crate::error::Result;
use crate::history::History;
use crate::hotkeys::HotkeyCommand;
use crate::key_sim::KeySimulator;
use crate::ollama::OllamaClient;
use crate::prompts;
use crate::text_utils;
use crate::ui::{self, HistoryData, HistoryEntryView, ResultData, UiCommand};

/// Texto de ayuda mostrado con Alt+H o click en tray.
pub fn help_text() -> String {
    "AI SUITE — Asistente de IA\n\
     =======================================\n\n\
     Selecciona texto en cualquier app y pulsa:\n\n\
     Alt + G      -> Corregir ortografia (pega el resultado)\n\
     Alt + T      -> Traducir (auto o [idioma])\n\
     Alt + R      -> Resumir (o [formato])\n\
     Alt + Enter  -> Generar respuesta\n\
     Alt + V      -> Ver historial de correcciones\n\
     Alt + H      -> Mostrar esta ayuda\n\n\
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
        .to_string()
}

/// Procesa un comando de hotkey.
pub fn handle_command(
    cmd: HotkeyCommand,
    config: &Config,
    clipboard: &Arc<ClipboardManager>,
    history: &Arc<Mutex<History>>,
    ui_tx: &std::sync::mpsc::Sender<UiCommand>,
) -> Result<()> {
    let ollama = OllamaClient::new(config.ollama.clone());
    let result = match cmd {
        HotkeyCommand::Correct => handle_correct(clipboard, history, ui_tx, ollama),
        HotkeyCommand::Translate => handle_translate(clipboard, ui_tx, ollama),
        HotkeyCommand::Summarize => handle_summarize(clipboard, ui_tx, ollama),
        HotkeyCommand::Generate => handle_generate(clipboard, ui_tx, ollama),
        HotkeyCommand::ShowHistory => handle_show_history(history, ui_tx),
        HotkeyCommand::ShowHelp => {
            let _ = ui_tx.send(UiCommand::ShowHelp(help_text()));
            ui::request_repaint();
            Ok(())
        }
    };

    // Si hubo error, mostrarlo sutilmente en la barra de estado
    if let Err(ref e) = result {
        let msg = format!("Error: {e}");
        let _ = ui_tx.send(UiCommand::SetStatus(msg));
        ui::request_repaint();
    }

    result
}

fn handle_correct(
    clipboard: &Arc<ClipboardManager>,
    history: &Arc<Mutex<History>>,
    ui_tx: &std::sync::mpsc::Sender<UiCommand>,
    ollama: OllamaClient,
) -> Result<()> {
    let _ = ui_tx.send(UiCommand::SetStatus("Corrigiendo...".to_string()));
    ui::request_repaint();

    let texto = obtener_texto_para_corregir(clipboard)?;
    if texto.is_empty() {
        let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let (contexto, texto_limpio) = text_utils::extraer_contexto(&texto);
    let sys_prompt = prompts::sys_prompt_corregir(&contexto);

    tracing::info!("Corrigiendo texto ({} chars)...", texto_limpio.len());
    let resultado =
        crate::runtime::global().block_on(ollama.generate(&sys_prompt, &texto_limpio))?;

    if resultado.is_empty() {
        tracing::warn!("Ollama devolvio resultado vacio.");
        let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    tracing::info!(
        "Correccion completada ({} chars). Pegando...",
        resultado.len()
    );

    {
        let mut hist = history
            .lock()
            .map_err(|_| crate::error::AppError::msg("Error de lock en historial"))?;
        hist.push(texto.clone(), resultado.clone());
        let _ = hist.save();
    }

    clipboard.set_text(&resultado)?;
    KeySimulator::paste()?;

    let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
    Ok(())
}

fn handle_translate(
    clipboard: &Arc<ClipboardManager>,
    ui_tx: &std::sync::mpsc::Sender<UiCommand>,
    ollama: OllamaClient,
) -> Result<()> {
    let _ = ui_tx.send(UiCommand::SetStatus("Traduciendo...".to_string()));
    ui::request_repaint();

    let texto = obtener_texto_para_corregir(clipboard)?;
    if texto.is_empty() {
        let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let (idioma, texto_limpio) = text_utils::extraer_contexto(&texto);
    let sys_prompt = prompts::sys_prompt_traducir(&idioma);

    tracing::info!("Traduciendo ({} chars)...", texto_limpio.len());
    let resultado =
        crate::runtime::global().block_on(ollama.generate(&sys_prompt, &texto_limpio))?;

    if resultado.is_empty() {
        let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let titulo = if idioma.is_empty() {
        "Traduccion".to_string()
    } else {
        format!("Traduccion al {idioma}")
    };

    let _ = ui_tx.send(UiCommand::ShowResult(ResultData {
        titulo,
        original: texto_limpio,
        resultado,
    }));
    ui::request_repaint();

    Ok(())
}

fn handle_summarize(
    clipboard: &Arc<ClipboardManager>,
    ui_tx: &std::sync::mpsc::Sender<UiCommand>,
    ollama: OllamaClient,
) -> Result<()> {
    let _ = ui_tx.send(UiCommand::SetStatus("Resumiendo...".to_string()));
    ui::request_repaint();

    let texto = obtener_texto_para_corregir(clipboard)?;
    if texto.is_empty() {
        let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let (formato, texto_limpio) = text_utils::extraer_contexto(&texto);
    let sys_prompt = prompts::sys_prompt_resumir(&formato);

    tracing::info!("Resumiendo ({} chars)...", texto_limpio.len());
    let resultado =
        crate::runtime::global().block_on(ollama.generate(&sys_prompt, &texto_limpio))?;

    if resultado.is_empty() {
        let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
    let _ = ui_tx.send(UiCommand::ShowResult(ResultData {
        titulo: "Resumen".to_string(),
        original: texto_limpio,
        resultado,
    }));
    ui::request_repaint();

    Ok(())
}

fn handle_generate(
    clipboard: &Arc<ClipboardManager>,
    ui_tx: &std::sync::mpsc::Sender<UiCommand>,
    ollama: OllamaClient,
) -> Result<()> {
    let _ = ui_tx.send(UiCommand::SetStatus("Generando respuesta...".to_string()));
    ui::request_repaint();

    let texto = obtener_texto_para_corregir(clipboard)?;
    if texto.is_empty() {
        let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let (contexto, prompt) = text_utils::extraer_contexto(&texto);
    let sys_prompt = prompts::sys_prompt_generar(&contexto);

    tracing::info!("Generando respuesta ({} chars)...", prompt.len());
    let resultado = crate::runtime::global().block_on(ollama.generate(&sys_prompt, &prompt))?;

    if resultado.is_empty() {
        let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
        return Ok(());
    }

    let _ = ui_tx.send(UiCommand::SetStatus(String::new()));
    let _ = ui_tx.send(UiCommand::ShowResult(ResultData {
        titulo: "Respuesta".to_string(),
        original: prompt,
        resultado,
    }));
    ui::request_repaint();

    Ok(())
}

fn handle_show_history(
    history: &Arc<Mutex<History>>,
    ui_tx: &std::sync::mpsc::Sender<UiCommand>,
) -> Result<()> {
    let hist = history
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

    let _ = ui_tx.send(UiCommand::ShowHistory(HistoryData {
        entries,
        selected: 0,
    }));
    ui::request_repaint();

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers de obtencion de texto
// ---------------------------------------------------------------------------

fn obtener_texto_para_corregir(clipboard: &Arc<ClipboardManager>) -> Result<String> {
    clipboard.backup()?;
    clipboard.clear()?;

    KeySimulator::copy()?;
    std::thread::sleep(std::time::Duration::from_millis(150));
    let texto = clipboard.get_text()?;

    if texto.trim().is_empty() {
        clipboard.restore()?;
        KeySimulator::select_all()?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        clipboard.clear()?;
        KeySimulator::copy()?;
        std::thread::sleep(std::time::Duration::from_millis(150));
        let texto = clipboard.get_text()?;

        if texto.trim().is_empty() {
            tracing::warn!("No hay texto para corregir.");
            clipboard.restore()?;
            return Ok(String::new());
        }

        if texto.len() > 3000 {
            clipboard.restore()?;
            KeySimulator::end()?;
            std::thread::sleep(std::time::Duration::from_millis(50));
            KeySimulator::shift_home()?;
            std::thread::sleep(std::time::Duration::from_millis(100));
            clipboard.clear()?;
            KeySimulator::copy()?;
            std::thread::sleep(std::time::Duration::from_millis(150));
            let texto = clipboard.get_text()?;

            if texto.trim().is_empty() || texto.len() > 1000 {
                tracing::warn!("Texto muy largo. Selecciona manualmente.");
                clipboard.restore()?;
                return Ok(String::new());
            }

            clipboard.restore()?;
            return Ok(texto);
        }

        clipboard.restore()?;
        return Ok(texto);
    }

    clipboard.restore()?;
    Ok(texto)
}
