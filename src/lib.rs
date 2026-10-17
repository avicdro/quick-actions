#![allow(missing_docs)]

// Modulos cross-platform (testables en Linux).
pub mod config;
pub mod error;
pub mod history;
pub mod ollama;
pub mod prompts;
pub mod runtime;
pub mod text_utils;
pub mod ui_protocol;
pub mod update_check;

// Modulos que dependen de crates solo disponibles en Windows.
// En Linux (para tests), no se compilan.
#[cfg(windows)]
pub mod actions;
#[cfg(windows)]
pub mod autostart;
#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
pub mod hotkeys;
#[cfg(windows)]
pub mod key_sim;
#[cfg(windows)]
pub mod tray;
#[cfg(windows)]
pub mod ui;
