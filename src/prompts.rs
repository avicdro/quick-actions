//! Prompts del sistema para las distintas funciones de IA.
//!
//! Cada funcion devuelve un `String` con las instrucciones
//! que se envian a Ollama como `system` prompt.
//!
//! # Bloque P (v0.6) — prompts personalizados
//!
//! Cualquier accion puede pisar su prompt por defecto con un `.md` en la
//! carpeta configurada (`[prompts] dir` de config.toml: corregir.md,
//! traducir.md, resumir.md, generar.md). Reglas:
//! - Archivo ausente o vacio → el default compilado de este modulo.
//! - Placeholders opcionales en el .md (`{contexto}`, `{idioma}`,
//!   `{formato}`) se sustituyen por los valores del runtime.
//! - El archivo se lee en CADA invocacion: editarlo toma efecto al
//!    siguiente atajo, sin reiniciar (I/O local despreciable).
//! - La carpeta puede ser relativa a config.toml, absoluta, o apuntar
//!   a un repo externo (`dir = "~/quick-actions-prompts"`); cadena
//!   vacia = override desactivado (solo defaults).

use std::path::{Path, PathBuf};

/// Prompt para corregir ortografia y gramatica.
pub fn sys_prompt_corregir(contexto: &str) -> String {
    let mut prompt = String::from(
        "Eres un corrector ortografico y gramatical experto. \
         Tu UNICA tarea es corregir la ortografia, la gramatica y la puntuacion del texto que te proporciono. \
         Mantienes SIEMPRE el estilo, el tono, el formato y el idioma original del autor. \
         Auto-detecta el idioma (principalmente espanol, pero puede ser cualquier otro). \
         NO anadas explicaciones, comentarios, introducciones, despedidas ni markdown. \
         NO respondas con frases como 'Aqui tienes el texto corregido' ni nada similar. \
         Devuelve UNICAMENTE el texto corregido, sin ningun texto adicional antes o despues. \
         Si el texto ya esta correcto, devuelvelo exactamente igual. \
         Si el texto esta vacio o no es texto, devuelve una cadena vacia.",
    );
    if !contexto.is_empty() {
        prompt.push_str("\n\nCONTEXTO OBLIGATORIO que debes seguir: ");
        prompt.push_str(contexto);
    }
    prompt
}

/// Prompt para traducir texto.
pub fn sys_prompt_traducir(idioma_destino: &str) -> String {
    let instruccion = if idioma_destino.is_empty() {
        String::from(
            "Detecta el idioma del texto. Si esta en espanol, traducelo al ingles. \
             Si esta en ingles (o cualquier otro idioma), traducelo al espanol.",
        )
    } else {
        format!("Traduce el siguiente texto al {idioma_destino}.")
    };

    format!(
        "Eres un traductor profesional. {instruccion} \
         Manten el tono, el formato y el significado original. \
         NO anadas explicaciones, notas ni comentarios. \
         Devuelve UNICAMENTE la traduccion, sin texto adicional."
    )
}

/// Prompt para resumir texto.
pub fn sys_prompt_resumir(formato: &str) -> String {
    let instruccion = if formato.is_empty() {
        String::from(
            "Resume el siguiente texto en un maximo de 3 vinetas concisas. \
             Usa el formato: \u{2022} vineta 1\n\u{2022} vineta 2\n\u{2022} vineta 3",
        )
    } else {
        format!("Resume el siguiente texto en {formato}.")
    };

    format!(
        "Eres un experto en sintesis de informacion. {instruccion} \
         Captura las ideas principales y descarta los detalles secundarios. \
         Manten el idioma original del texto. \
         NO anadas introducciones ni conclusiones. \
         Devuelve UNICAMENTE el resumen."
    )
}

/// Prompt para generar una respuesta generica.
pub fn sys_prompt_generar(contexto: &str) -> String {
    let mut prompt = String::from(
        "Eres un asistente util y conciso. \
         Responde al prompt del usuario de forma directa y clara. \
         Manten el idioma del prompt. \
         NO anadas introducciones innecesarias.",
    );
    if !contexto.is_empty() {
        prompt.push_str("\n\nCONTEXTO: ");
        prompt.push_str(contexto);
    }
    prompt
}

// ---------------------------------------------------------------------------
// Bloque P (v0.6): override de prompts por .md
// ---------------------------------------------------------------------------

/// `(etiqueta legible, archivo .md)` de cada accion con prompt pisable.
pub const PROMPT_FILES: [(&str, &str); 4] = [
    ("corregir", "corregir.md"),
    ("traducir", "traducir.md"),
    ("resumir", "resumir.md"),
    ("generar", "generar.md"),
];

/// README.md que se escribe en el bootstrap de la carpeta por defecto.
/// (Unica pieza de la mecanica visible fuera del codigo: explica las
/// reglas y deja un copiado de ejemplo de override con estilo chat.)
pub const FOLDER_README: &str = r#"# Prompts de IA de quick-actions

Aqui puedes pisar el prompt por defecto de cada atajo con un `.md`
(si la carpeta configurada en `[prompts] dir` de config.toml es otra,
esto aplica a esa carpeta):

| Archivo        | Atajo            | Placeholder opcional |
|----------------|------------------|----------------------|
| corregir.md    | Corrector        | `{contexto}`         |
| traducir.md    | Traducir         | `{idioma}`           |
| resumir.md     | Resumir          | `{formato}`          |
| generar.md     | Generar          | `{contexto}`         |

Reglas:
- Archivo ausente o vacio = prompt por defecto de la app (seguro).
- Los placeholders se sustituyen en cada atajo por los valores del
  runtime (p. ej. el texto detectado en `{contexto}`). Pon los que
  quieras: un placeholder fuera del .md simplemente no llega.
- NO hace falta reiniciar: el .md se lee en cada invocacion.

Ejemplo de corregir.md con estilo propio (separar ideas con saltos
de linea en vez de puntos, ~ como pausa en vez de guion):

    Corrige ortografia y gramatica del texto: tildes, erratas,
    repeticiones y frases enredadas. Suena como escribe el autor
    (mensajes de chat), no como una IA:
    - ideas separadas por comas y saltos de linea; punto/punto y coma
      solo si sin ellos el mensaje queda ambiguo
    - nada de guiones (-, --, —) como pausa: usa "~" (respetar el "-"
      de rangos "2-3" y palabras compuestas)
    - sin markdown, sin explicaciones: solo el texto corregido
"#;

/// Expand `~` (home del usuario) en la ruta configurada.
///
/// Usa `home` explicito para ser testeable; en runtime pasa el env
/// (`USERPROFILE` en Windows, `HOME` en unix).
pub fn resolve_dir_with_home(config_dir: &Path, raw: &str, home: Option<&str>) -> Option<PathBuf> {
    let raw = raw.trim();
    // Vacio = override desactivado; solo defaults compilados.
    if raw.is_empty() {
        return None;
    }

    // ~/... o ~\... (o solo "~"): substituir por el home.
    if raw == "~" {
        return home.map(|h| PathBuf::from(h));
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return home.map(|h| PathBuf::from(h).join(rest));
    }

    let path = Path::new(raw);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    // Relativa: al lado del config.toml (su sitio logico).
    Some(config_dir.join(path))
}

/// Directorio de prompts desde la config (con home del env en runtime).
#[must_use]
pub fn resolve_dir(config_dir: &Path, raw: &str) -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok();
    resolve_dir_with_home(config_dir, raw, home.as_deref())
}

/// Sustituye los placeholders `{nombre}` del prompt por sus valores.
fn aplicar_placeholders(prompt: &str, vars: &[(&str, &str)]) -> String {
    let mut out = prompt.to_string();
    for (nombre, valor) in vars {
        let marcador = format!("{{{nombre}}}");
        out = out.replace(&marcador, valor);
    }
    out
}

/// Prompt final de una accion: `.md` de la carpeta si existe y no esta
/// vacio, si no el default compilado. `vars` = placeholders disponibles.
#[must_use]
pub fn prompt_con_override(
    dir: Option<&Path>,
    archivo: &str,
    default: String,
    vars: &[(&str, &str)],
) -> String {
    let Some(dir) = dir else {
        return default;
    };
    let path = dir.join(archivo);
    match std::fs::read_to_string(&path) {
        // Un .md con contenido manda (aunque ignore los placeholders).
        Ok(contenido) if !contenido.trim().is_empty() => {
            tracing::debug!("prompt personalizado en uso: {}", path.display());
            aplicar_placeholders(&contenido, vars)
        }
        // Archivo vacio (o solo espacios): default; no es un error.
        Ok(_) => {
            tracing::debug!(
                "prompt personalizado vacio, uso default: {}",
                path.display()
            );
            default
        }
        // Ausente: el caso normal sin personalizacion.
        Err(_) => default,
    }
}

/// Prompt del corrector con override opcional (`corregir.md`).
///
/// El placeholder `{contexto}` llega vacio si no hay contexto detectado.
#[must_use]
pub fn corregir(dir: Option<&Path>, contexto: &str) -> String {
    let default = sys_prompt_corregir(contexto);
    prompt_con_override(dir, "corregir.md", default, &[("contexto", contexto)])
}

/// Prompt del traductor con override opcional (`traducir.md`).
///
/// `{idioma}` llega tal cual: cadena vacia = modo auto-deteccion del
/// default; en un .md personalizado el autor define como tratarlo.
#[must_use]
pub fn traducir(dir: Option<&Path>, idioma: &str) -> String {
    let default = sys_prompt_traducir(idioma);
    prompt_con_override(dir, "traducir.md", default, &[("idioma", idioma)])
}

/// Prompt del resumen con override opcional (`resumir.md`).
#[must_use]
pub fn resumir(dir: Option<&Path>, formato: &str) -> String {
    let default = sys_prompt_resumir(formato);
    prompt_con_override(dir, "resumir.md", default, &[("formato", formato)])
}

/// Prompt de generacion con override opcional (`generar.md`).
#[must_use]
pub fn generar(dir: Option<&Path>, contexto: &str) -> String {
    let default = sys_prompt_generar(contexto);
    prompt_con_override(dir, "generar.md", default, &[("contexto", contexto)])
}

/// Bootstrap del primer arranque: si la carpeta configurada NO existe,
/// la crea con un `README.md` documentando la mecanica (la carpeta
/// queda limpia: los defaults viven en codigo).
///
/// Errores silenciados a `warn!`: un bootstrap fallido nunca bloquea el
/// arranque ni el uso de defaults.
pub fn ensure_default_folder(config_dir: &Path, raw: &str) {
    let Some(dir) = resolve_dir(config_dir, raw) else {
        return; // dir vacio: override off, nada que crear
    };
    if dir.is_dir() {
        return; // ya existe: no se toca nada
    }
    match std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(dir.join("README.md"), FOLDER_README))
    {
        Ok(()) => tracing::info!("Carpeta de prompts creada con README: {}", dir.display()),
        Err(e) => tracing::warn!(
            "No se pudo crear la carpeta de prompts ({e}): los prompts \
             personales no funcionaran en esta ruta"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // --- Overrides por .md (Bloque P) -----------------------------------

    fn tmp_dir(nombre: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(nombre);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_dir_relativa_absoluta_vacia_y_tilde() {
        let cfg = PathBuf::from("/cfg");

        // Vacio (o solo espacios) = override desactivado.
        assert_eq!(resolve_dir_with_home(&cfg, "", None), None);
        assert_eq!(resolve_dir_with_home(&cfg, "   ", None), None);

        // Relativa: al lado del config.toml.
        assert_eq!(
            resolve_dir_with_home(&cfg, "prompts", None),
            Some(cfg.join("prompts"))
        );

        // Absoluta: tal cual.
        let abs = PathBuf::from("/home/yo/mis-prompts");
        assert_eq!(
            resolve_dir_with_home(&cfg, "/home/yo/mis-prompts", None),
            Some(abs)
        );

        // ~ (con y sin prefijo) se expande al home dado.
        assert_eq!(
            resolve_dir_with_home(&cfg, "~/qa-prompts", Some("/home/yo")),
            Some(PathBuf::from("/home/yo/qa-prompts"))
        );
        assert_eq!(
            resolve_dir_with_home(&cfg, "~", Some("/home/yo")),
            Some(PathBuf::from("/home/yo"))
        );

        // ~ sin home disponible: override inaccesible -> None.
        assert_eq!(resolve_dir_with_home(&cfg, "~/qa-prompts", None), None);
    }

    #[test]
    fn placeholders_se_sustituyen_todos() {
        let vars = &[("contexto", "modo formal"), ("formato", "3 vinetas")];
        let out =
            aplicar_placeholders("Estilo: {contexto}. Formato: {formato}. Sin {falta}.", vars);
        assert_eq!(out, "Estilo: modo formal. Formato: 3 vinetas. Sin {falta}.");
    }

    #[test]
    fn override_por_md_pisa_al_default() {
        let dir = tmp_dir("qa-prompts-override");
        fs::write(
            dir.join("corregir.md"),
            "Mi prompt propio con contexto {contexto}.",
        )
        .unwrap();

        let p = corregir(Some(&dir), "modo suave");
        assert_eq!(p, "Mi prompt propio con contexto modo suave.");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn override_archivo_vacio_o_ausente_da_el_default() {
        let dir = tmp_dir("qa-prompts-vacio");
        fs::write(dir.join("resumir.md"), "   \n").unwrap();

        // Vacio -> default compilado.
        let p = resumir(Some(&dir), "");
        assert!(p.contains("3 vinetas"), "default al archivo vacio");
        // La .md de otro accion no existe -> default tambien.
        let p2 = generar(Some(&dir), "");
        assert!(!p2.contains("CONTEXTO"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sin_dir_el_override_esta_off() {
        // promt_con_override(None, ...) = default puro.
        let p = traducir(None, "frances");
        assert!(p.contains("frances"));
        assert!(p.contains("traductor"));
    }

    #[test]
    fn bootstrap_crea_carpeta_limpia_con_readme_y_no_revisa_hijos() {
        let base = tmp_dir("qa-prompts-bootstrap");
        let cfg = base.join("config.toml"); // no hace falta que exista

        ensure_default_folder(&base, "prompts");
        let readme = base.join("prompts").join("README.md");
        assert!(readme.exists());
        assert!(fs::read_to_string(&readme).unwrap().contains("corregir.md"));

        // Segunda llamada: no-op (no reescribe README ya editado).
        fs::write(&readme, "MI README personal").unwrap();
        ensure_default_folder(&base, "prompts");
        assert_eq!(fs::read_to_string(&readme).unwrap(), "MI README personal");

        // dir vacio: no crea nada.
        ensure_default_folder(&base, "");
        assert!(fs::read_dir(&base).unwrap().count() <= 1);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn prompt_files_cubren_las_4_acciones() {
        // Garantia: un nombre por accion, archivos .md distintos.
        let mut files: Vec<_> = PROMPT_FILES.iter().map(|(_, f)| *f).collect();
        let mut etqs: Vec<_> = PROMPT_FILES.iter().map(|(e, _)| *e).collect();
        files.sort();
        etqs.sort();
        assert_eq!(
            files,
            ["corregir.md", "generar.md", "resumir.md", "traducir.md"]
        );
    }

    #[test]
    fn sys_prompt_corregir_sin_contexto() {
        let prompt = sys_prompt_corregir("");
        assert!(prompt.contains("corrector"));
        assert!(!prompt.contains("CONTEXTO"));
    }

    #[test]
    fn sys_prompt_corregir_con_contexto() {
        let prompt = sys_prompt_corregir("tono formal");
        assert!(prompt.contains("CONTEXTO"));
        assert!(prompt.contains("tono formal"));
    }

    #[test]
    fn sys_prompt_traducir_auto() {
        let prompt = sys_prompt_traducir("");
        assert!(prompt.contains("Detecta el idioma"));
    }

    #[test]
    fn sys_prompt_traducir_idioma_especifico() {
        let prompt = sys_prompt_traducir("frances");
        assert!(prompt.contains("frances"));
    }

    #[test]
    fn sys_prompt_resumir_default() {
        let prompt = sys_prompt_resumir("");
        assert!(prompt.contains("3 vinetas"));
    }

    #[test]
    fn sys_prompt_resumir_formato_personalizado() {
        let prompt = sys_prompt_resumir("5 puntos");
        assert!(prompt.contains("5 puntos"));
    }

    #[test]
    fn sys_prompt_generar_sin_contexto() {
        let prompt = sys_prompt_generar("");
        assert!(!prompt.contains("CONTEXTO"));
    }

    #[test]
    fn sys_prompt_generar_con_contexto() {
        let prompt = sys_prompt_generar("responde en JSON");
        assert!(prompt.contains("CONTEXTO"));
        assert!(prompt.contains("responde en JSON"));
    }
}
