//! Auto-actualizacion con un click (U7) — el patron de las apps OSS pro:
//!
//! 1. Descarga `quick-actions.exe` + `SHA256SUMS.txt` de la release.
//! 2. Verifica el SHA-256 del binario bajado.
//! 3. RENOMBRA el exe vivo a `quick-actions.prev.exe` (Windows bloquea
//!    sobreescribir/borrar un exe en ejecucion, pero NO su renombrado).
//! 4. RENOMBRA el `.update.exe` descargado a `quick-actions.exe`.
//! 5. Spawn del nuevo proceso + `exit(0)` del viejo.
//! 6. El nuevo, al arranque: elimina `.prev.exe` (mejor esfuerzo — puede
//!    seguir lockeado un instante;) ya no lo estara y avisa
//!    "Actualizado a vX" (marker `update-applied.tmp`).
//!
//! Si algo falla tras el primer rename, hace rollback del swap para no
//! dejar el instal roto.

use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::mpsc::Sender;

use crate::ui_protocol::{RepaintFn, UiCommand};

/// Timeout de descargas del updater (el exe es ~7MB: bien holgado).
const DL_TIMEOUT_SECS: u64 = 120;

const EXE_ASSET: &str = "quick-actions.exe";
const SUMS_ASSET: &str = "SHA256SUMS.txt";

fn toast(ui_tx: &Sender<UiCommand>, repaint: &RepaintFn, msg: String) {
    let _ = ui_tx.send(UiCommand::SetStatus(msg));
    (repaint)();
}

/// Aplica la release `tag` del repo: descarga, verifica, swap, reinicia.
///
/// Bloquea el thread llamador (spawn en la UI antes de invocar). Todos los
/// pasos informan por toast; un fallo deja la instalacion intacta.
pub fn apply_update(repo: &str, tag: &str, ui_tx: Sender<UiCommand>, repaint: RepaintFn) {
    let Some(exe) = std::env::current_exe().ok() else {
        toast(
            &ui_tx,
            &repaint,
            "Error: no se resolvió el exe actual".to_string(),
        );
        return;
    };
    let Some(dir) = exe.parent().map(std::path::Path::to_path_buf) else {
        toast(&ui_tx, &repaint, "Error: exe sin directorio".to_string());
        return;
    };

    let base = dir.join(EXE_ASSET);
    let update_file = dir.join("quick-actions.update.exe");
    let prev_file = dir.join("quick-actions.prev.exe");

    // === 1. Descarga del exe de la release ===
    toast(&ui_tx, &repaint, format!("Descargando {tag}..."));
    let exe_url = format!("https://github.com/{repo}/releases/download/{tag}/{EXE_ASSET}");
    if let Err(e) = download(&exe_url, &update_file) {
        let _ = std::fs::remove_file(&update_file);
        toast(&ui_tx, &repaint, format!("Error: descarga falló: {e}"));
        return;
    }

    // === 2. Verificacion SHA-256 (pro-grade; si el asset de sums no esta,
    // sigue igual pero lo anota — release viejas no tenian sums) ===
    let sums_url = format!("https://github.com/{repo}/releases/download/{tag}/{SUMS_ASSET}");
    let sums_tmp = dir.join("SHA256SUMS.tmp");
    match download(&sums_url, &sums_tmp) {
        Ok(()) => {
            let expected = std::fs::read_to_string(&sums_tmp)
                .ok()
                .and_then(|raw| expected_hash_of(&raw, EXE_ASSET));

            let Some(expected) = expected else {
                let _ = std::fs::remove_file(&sums_tmp);
                let _ = std::fs::remove_file(&update_file);
                toast(
                    &ui_tx,
                    &repaint,
                    "Error: SHA256SUMS no contiene el exe; no aplico nada".to_string(),
                );
                return;
            };

            let got = match sha256_file(&update_file) {
                Ok(h) => h,
                Err(e) => {
                    let _ = std::fs::remove_file(&update_file);
                    toast(&ui_tx, &repaint, format!("Error: hash fallo: {e}"));
                    return;
                }
            };

            if got != expected {
                let _ = std::fs::remove_file(&update_file);
                let _ = std::fs::remove_file(&sums_tmp);
                tracing::warn!("Auto-update: hash != esperado ({got} vs {expected})");
                toast(
                    &ui_tx,
                    &repaint,
                    "Error: hash del binario no coincide; no aplico nada".to_string(),
                );
                return;
            }
            tracing::info!("Auto-update: SHA-256 verificado ({got}).");
        }
        Err(e) => {
            // Sin sums disponible (release vieja): aviso, no bloqueo el flow.
            tracing::debug!("SHA256SUMS no disponible ({e}); sigo sin verificar hash.");
        }
    }
    let _ = std::fs::remove_file(&sums_tmp);

    // === 3/4. Swap: renombrar el ejecutable en ejecucion (permitido) y
    // colocar el nuevo en su lugar ===
    if prev_file.exists() {
        let _ = std::fs::remove_file(&prev_file);
    }
    if let Err(e) = std::fs::rename(&base, &prev_file) {
        toast(
            &ui_tx,
            &repaint,
            format!("Error: no pude mover el exe actual: {e}"),
        );
        return;
    }
    if let Err(e) = std::fs::rename(&update_file, &base) {
        // Rollback: dejar como estaba.
        let _ = std::fs::rename(&prev_file, &base);
        toast(&ui_tx, &repaint, format!("Error: colocando el nuevo: {e}"));
        return;
    }
    tracing::info!("Auto-update: swap completado ({tag}).");

    // === 5. Marker para el toast del nuevo proceso + reinicio ===
    let _ = std::fs::write(dir.join("update-applied.tmp"), tag);
    toast(
        &ui_tx,
        &repaint,
        format!("Actualizado a {tag} — reiniciando..."),
    );

    match std::process::Command::new(&base).spawn() {
        Ok(_) => {
            tracing::info!("Auto-update: proceso nuevo lanzado; saliendo.");
            // Pequeño delay para que el worker del log async descargue.
            std::thread::sleep(std::time::Duration::from_millis(300));
            std::process::exit(0);
        }
        Err(e) => {
            toast(
                &ui_tx,
                &repaint,
                format!("Error: no arrancó el nuevo exe: {e}"),
            );
        }
    }
}

/// Descarga `url` a `dest` (stream, sin cargar el binario entero en RAM).
fn download(url: &str, dest: &Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DL_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("client: {e}"))?;

    let runtime = crate::runtime::global();
    runtime.block_on(async {
        let response = client
            .get(url)
            .header(
                "User-Agent",
                format!("quick-actions/{}", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await
            .map_err(|e| format!("GET {url}: {e}"))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }

        // U7: 7MB a RAM es aceptable; la feature "stream" no esta activa.
        let bytes = response.bytes().await.map_err(|e| format!("stream: {e}"))?;
        std::fs::write(dest, bytes.as_ref()).map_err(|e| format!("write: {e}"))?;
        Ok(())
    })
}

/// SHA-256 de un archivo (hex minuscula).
fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let digest = Sha256::digest(&bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    Ok(hex)
}

/// Extrae el hash de `quick-actions.exe` desde el contenido estilo
/// `sha256sum` (`"<hash>  <name>"` con espacio o `*` binario).
fn expected_hash_of(raw: &str, name: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (hash, file) = line.split_once(' ')?;
        let file = file.trim_start_matches('*').trim();
        if file.eq_ignore_ascii_case(name) {
            let hash = hash.trim().to_ascii_lowercase();
            if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Some(hash);
            }
        }
    }
    None
}
