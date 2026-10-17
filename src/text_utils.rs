//! Utilidades de procesamiento de texto (testables, sin dependencias de plataforma).

/// Extrae `[contexto]` al inicio del texto y devuelve (contexto, texto_limpio).
///
/// Si no hay corchetes al inicio, devuelve `("", texto_original)`.
///
/// # Examples
///
/// ```
/// use quick_actions::text_utils::extraer_contexto;
///
/// let (ctx, limpio) = extraer_contexto("[frances] hola mundo");
/// assert_eq!(ctx, "frances");
/// assert_eq!(limpio, "hola mundo");
///
/// let (ctx, limpio) = extraer_contexto("sin contexto");
/// assert_eq!(ctx, "");
/// assert_eq!(limpio, "sin contexto");
/// ```
pub fn extraer_contexto(texto: &str) -> (String, String) {
    if let Some(rest) = texto.trim_start().strip_prefix('[') {
        if let Some((ctx, rest2)) = rest.split_once(']') {
            let texto_limpio = rest2.trim_start().to_string();
            return (ctx.trim().to_string(), texto_limpio);
        }
    }
    (String::new(), texto.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraer_contexto_con_contexto() {
        let (ctx, limpio) = extraer_contexto("[frances] hola mundo");
        assert_eq!(ctx, "frances");
        assert_eq!(limpio, "hola mundo");
    }

    #[test]
    fn extraer_contexto_sin_contexto() {
        let (ctx, limpio) = extraer_contexto("hola mundo");
        assert_eq!(ctx, "");
        assert_eq!(limpio, "hola mundo");
    }

    #[test]
    fn extraer_contexto_con_espacios_antes() {
        let (ctx, limpio) = extraer_contexto("   [5 vinetas] texto largo");
        assert_eq!(ctx, "5 vinetas");
        assert_eq!(limpio, "texto largo");
    }

    #[test]
    fn extraer_contexto_vacio() {
        let (ctx, limpio) = extraer_contexto("");
        assert_eq!(ctx, "");
        assert_eq!(limpio, "");
    }

    #[test]
    fn extraer_contexto_corchetes_sin_cerrar() {
        let (ctx, limpio) = extraer_contexto("[frances hola mundo");
        assert_eq!(ctx, "");
        assert_eq!(limpio, "[frances hola mundo");
    }

    #[test]
    fn extraer_contexto_contexto_vacio() {
        let (ctx, limpio) = extraer_contexto("[] hola");
        assert_eq!(ctx, "");
        assert_eq!(limpio, "hola");
    }
}
