//! Prompts del sistema para las distintas funciones de IA.
//!
//! Cada funcion devuelve un `String` con las instrucciones
//! que se envian a Ollama como `system` prompt.

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

#[cfg(test)]
mod tests {
    use super::*;

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
