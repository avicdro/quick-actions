#![allow(missing_docs)]

// Modulos cross-platform (testables en Linux).
pub mod config;
pub mod error;
pub mod history;
pub mod hotkeys; // comandos/parse; win_hook.rs es el windows-only
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
pub mod app_state;
#[cfg(windows)]
pub mod autostart;
#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
pub mod key_sim;
#[cfg(windows)]
pub mod self_update;
#[cfg(windows)]
pub mod single_instance;
#[cfg(windows)]
pub mod tray;
#[cfg(windows)]
pub mod ui;

/// B1 — Version legible para log y Ajustes: `vX.Y.Z` o `vX.Y.Z (abcdef1)`
/// cuando hay build id (commit corto de git, capturado en build.rs).
pub fn version_string() -> String {
    let build = env!("QA_BUILD_ID");
    if build.is_empty() {
        format!("v{}", env!("CARGO_PKG_VERSION"))
    } else {
        format!("v{} ({build})", env!("CARGO_PKG_VERSION"))
    }
}

#[cfg(test)]
mod tests {
    use super::version_string;

    #[test]
    fn version_string_lleva_version_y_hash() {
        let v = version_string();
        assert!(v.starts_with('v'), "empieza con 'v': {v}");
        assert!(
            v.contains(env!("CARGO_PKG_VERSION")),
            "lleva la version: {v}"
        );
    }
}
