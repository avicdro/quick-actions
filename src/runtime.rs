//! Runtime global de tokio compartido por toda la aplicacion.
//!
//! Evita crear y destruir un runtime en cada hotkey.

use std::sync::OnceLock;
use tokio::runtime::Runtime;

/// Runtime global. Se inicializa una sola vez.
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Devuelve una referencia al runtime global.
///
/// Inicializa el runtime en la primera llamada.
pub fn global() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|e| {
                panic!("No se pudo crear el runtime de tokio: {e}");
            })
    })
}
