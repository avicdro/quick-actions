//! Auto-arranque con Windows via registro (HKCU).
//!
//! Escribe/elimina la entrada `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! apuntando al exe actual. No requiere permisos de admin (HKCU).

use std::path::Path;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RRF_RT_REG_SZ,
};

/// Nombre del valor dentro de la key Run.
const RUN_VALUE_NAME: PCWSTR = w!("QuickActions");
/// Path de la key Run de usuario (HKCU, sin admin).
const RUN_KEY_PATH: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

/// Exe actual con path absoluto estilo Windows (sin prefijo `\\?\`):
/// las entradas de Run no resuelven rutas de dispositivo.
fn current_exe_absolute() -> Result<std::path::PathBuf, &'static str> {
    std::env::current_exe()
        .ok()
        .map(|p| {
            let s = p.to_string_lossy().to_string();
            let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
            std::path::PathBuf::from(s)
        })
        .ok_or_else(|| "No se pudo resolver current_exe")
}

/// Indica si la entrada de auto-arranque existe y apunta al exe actual.
///
/// Si existe pero apunta a otro path (exe movido), devuelve `false`: para el
/// usuario el toggle esta apagado (el heal de `sync_with_config` lo recrea).
pub fn is_enabled() -> bool {
    match current_exe_absolute() {
        Ok(exe) => is_enabled_with_exe(&exe),
        Err(_) => false,
    }
}

/// Variante inyectable con el exe por parametro (base para mocks de tests).
fn is_enabled_with_exe(exe: &Path) -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        // SAFETY: api del registro con params validos; key Run siempre existe.
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY_PATH,
            Default::default(),
            KEY_QUERY_VALUE,
            &mut hkey,
        );
        if status != ERROR_SUCCESS {
            return false;
        }

        let mut buf = [0u16; 1024];
        let mut buf_len = 1024u32;
        // SAFETY: buf_len declara el tamano exacto para que la API no escriba
        // fuera del buffer; devuele el numero de bytes copiados.
        let status = RegGetValueW(
            hkey,
            PCWSTR::null(),
            RUN_VALUE_NAME,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut buf_len),
        );
        let _ = RegCloseKey(hkey);

        if status != ERROR_SUCCESS {
            return false;
        }

        // buf_len viene en BYTES (+1 char para la NUL); recorta a chars u16.
        let chars = (buf_len as usize / 2).saturating_sub(1);
        let stored = String::from_utf16_lossy(&buf[..chars]);
        let stored = stored.trim_matches('"').to_string();

        stored.eq_ignore_ascii_case(&exe.to_string_lossy())
    }
}

/// Activa o desactiva el auto-arranque.
///
/// # Errors
///
/// Devuelve String si falla la api del registro; el caller lo loguea/muestra.
pub fn set_enabled(on: bool) -> Result<(), String> {
    match current_exe_absolute() {
        Ok(exe) => set_enabled_with_exe(on, &exe),
        Err(e) => Err(e.to_string()),
    }
}

fn set_enabled_with_exe(on: bool, exe: &Path) -> Result<(), String> {
    unsafe {
        let mut hkey = HKEY::default();
        // SAFETY: RegCreateKeyExW para no depender de que la key Run exista;
        // crear una key existe es idempotente. phkResult sale del out-param.
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY_PATH,
            0, // reserved
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None, // security attrs
            &mut hkey,
            None,
        );
        if status != ERROR_SUCCESS {
            return Err(format!("RegCreateKeyExW fallo: {status:?}"));
        }

        let status = if on {
            // Entre comillas para paths con espacios:
            // "C:\quick-actions\quick-actions.exe" + NUL necesario para REG_SZ.
            let quoted = format!("\"{}\"", exe.to_string_lossy());
            let bytes: Vec<u8> = quoted
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            // SAFETY: &[u8] -> reg api con REG_SZ (encode utf-16 LE con NUL).
            RegSetValueExW(hkey, RUN_VALUE_NAME, 0, REG_SZ, Some(&bytes))
        } else {
            RegDeleteValueW(hkey, RUN_VALUE_NAME)
        };

        let _ = RegCloseKey(hkey);

        if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
            // FILE_NOT_FOUND en delete = la entrada ya estaba off: no es error.
            return Err(format!("Error escribiendo registro: {status:?}"));
        }

        Ok(())
    }
}

/// Alinea la entrada del registro con el valor deseado de config al arrancar.
///
/// Reglas (P1 en docs/plans/improvements-v0.3.md):
/// - config `true`: si la entrada no existe (o apunta a otro path), la
///   crea/actualiza. Auto-repara si el exe se movio.
/// - config `false`: NO toca nada — no deshacemos toggles hechos por el
///   usuario desde el tray (el registro pasa a ser la fuente desde ese
///   momento).
pub fn sync_with_config(desired: bool) {
    if !desired {
        return;
    }

    let exe = match current_exe_absolute() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Auto-arranque: {e}");
            return;
        }
    };

    if is_enabled_with_exe(&exe) {
        tracing::debug!("Auto-arranque ya configurado con el exe correcto.");
        return;
    }

    match set_enabled_with_exe(desired, &exe) {
        Ok(()) => tracing::info!("Auto-arranque activado (HKCU Run)."),
        Err(e) => tracing::warn!("No se pudo activar auto-arranque: {e}"),
    }
}
