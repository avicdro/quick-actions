//! Verificación de actualizaciones (P4 en docs/plans/improvements-v0.3.md).
//!
//! Al arrancar consulta `GET https://api.github.com/repos/{repo}/releases/latest`
//! en un thread de fondo; si la tag publicada es > `CARGO_PKG_VERSION`, envía
//! `UiCommand::ShowUpdate` para que la UI muestre aviso + enlace de descarga.
//! No instala nada: solo avisa.

use std::sync::mpsc::Sender;

use serde::Deserialize;

use crate::config::AppConfig;
use crate::ui_protocol::UiCommand;

/// Timeout de la consulta (no colgamos el arranque ni el thread).
const HTTP_TIMEOUT_SECS: u64 = 5;

/// Respuesta minima de `/releases/latest`.
#[derive(Debug, Clone, Deserialize)]
pub struct LatestRelease {
    /// Tag publicada (ej. `v0.3.0`).
    #[serde(rename = "tag_name")]
    pub tag: String,
    /// URL de la release en GitHub (para descargar).
    #[serde(rename = "html_url")]
    pub url: String,
    /// Notas de la release (opcional en la API).
    #[serde(default)]
    pub body: String,
}

/// Lanza el check en un thread de fondo (fire & forget).
///
/// No-op si esta desactivado (`check_updates = false`) o sin repo configurado.
pub fn spawn_check(app_config: &AppConfig, ui_tx: Sender<UiCommand>) {
    if !app_config.check_updates || app_config.repository.is_empty() {
        return;
    }

    let repo = app_config.repository.clone();

    std::thread::spawn(move || {
        let runtime = crate::runtime::global();
        match runtime.block_on(fetch_latest(&repo)) {
            Ok(release) => {
                if is_newer_tag(&release.tag, env!("CARGO_PKG_VERSION")) {
                    tracing::info!(
                        "Nueva version disponible: {} (corriendo {})",
                        release.tag,
                        env!("CARGO_PKG_VERSION")
                    );
                    let _ = ui_tx.send(UiCommand::ShowUpdate {
                        version: release.tag,
                        url: release.url,
                        body: release.body,
                    });
                } else {
                    tracing::debug!(
                        "Sin updates: {} <= {}",
                        release.tag,
                        env!("CARGO_PKG_VERSION")
                    );
                }
            }
            Err(e) => {
                // No es critico: falla silenciosa (offline, rate limit, etc).
                tracing::debug!("Check de updates ignoro: {e}");
            }
        }
    });
}

/// Consulta la ultima release del repo en GitHub.
///
/// # Errors
///
/// Error de red/HTTP o de parseo; el caller lo trata como no-critico.
pub async fn fetch_latest(repo: &str) -> Result<LatestRelease, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Client reqwest: {e}"))?;

    // GitHub API exige User-Agent; reqwest no lo envia por defecto.
    let response = client
        .get(&url)
        .header(
            "User-Agent",
            format!("quick-actions/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("GitHub devolvio HTTP {}", response.status()));
    }

    response
        .json::<LatestRelease>()
        .await
        .map_err(|e| format!("Error parseando release: {e}"))
}

/// Compara la tag de la release con la version del exe actual.
///
/// Carece el sufijo `v`, ignora sufijos pre-release (`-rc1`), y devuelve
/// true si la del repo es estrictamente mayor.
pub fn is_newer_tag(tag: &str, current: &str) -> bool {
    match (parse_version(tag), parse_version(current)) {
        (Some(next), Some(cur)) => next > cur,
        _ => false,
    }
}

/// Parsea `v0.2.1` / `0.2` / `0.2.1-rc1` -> `(0, 2, 1)`.
///
/// Acepta hasta 3 componentes; `None` si el core no es numerico (tags
/// raras: nunca aviso por falso positivo de version).
fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let t = tag.trim().trim_start_matches(['v', 'V']);
    if t.is_empty() {
        return None;
    }
    let core = t.split(['-', '+']).next().unwrap_or(t);
    let mut parts = core.split('.');

    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next()?.parse().ok()?;
    let patch: u64 = parts.next().unwrap_or("0").parse().ok().unwrap_or(0);

    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_basic() {
        assert_eq!(parse_version("v0.2.1"), Some((0, 2, 1)));
        assert_eq!(parse_version("0.3"), Some((0, 3, 0)));
        assert_eq!(parse_version("1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_version("v1.2.3+build.7"), Some((1, 2, 3)));
    }

    #[test]
    fn parse_version_rejects_rare_tags() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_version("x.y.z"), None);
    }

    #[test]
    fn is_newer_tag_compares() {
        assert!(is_newer_tag("v0.3.0", "0.2.0"));
        assert!(is_newer_tag("v0.3.1", "v0.3.0"));
        assert!(is_newer_tag("1.0.0", "0.99.99"));
        assert!(!is_newer_tag("v0.2.0", "0.2.0"));
        assert!(!is_newer_tag("v0.2.0", "0.3.0"));
        assert!(!is_newer_tag("garbage", "0.1.0"));
        // Pre-release misma version: no avisa (core igual)
        assert!(!is_newer_tag("v0.3.0-rc1", "v0.3.0"));
    }
}
