use reqwest;
use serde::{Deserialize, Serialize};

use crate::config::OllamaConfig;
use crate::error::{AppError, Result};

/// Cliente para comunicarse con la API de Ollama.
pub struct OllamaClient {
    config: OllamaConfig,
    client: reqwest::Client,
}

/// Cuerpo de la peticion a Ollama.
#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    system: String,
    prompt: String,
    stream: bool,
}

/// Respuesta de Ollama.
#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

impl OllamaClient {
    /// Crea un nuevo cliente configurado.
    pub fn new(config: OllamaConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { config, client }
    }

    /// Genera una respuesta usando Ollama.
    ///
    /// # Arguments
    ///
    /// * `system_prompt` - Instrucciones del sistema.
    /// * `user_text` - Texto de entrada del usuario.
    ///
    /// # Returns
    ///
    /// El texto generado por el modelo, o un error si falla la conexion
    /// o la respuesta.
    pub async fn generate(&self,
        system_prompt: &str,
        user_text: &str,
    ) -> Result<String> {
        let body = OllamaRequest {
            model: self.config.model.clone(),
            system: system_prompt.to_string(),
            prompt: user_text.to_string(),
            stream: false,
        };

        let response = self.client
            .post(&self.config.endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    AppError::Ollama(
                        "No se pudo conectar con Ollama. Asegurate de que 'ollama serve' esta corriendo.".to_string()
                    )
                } else {
                    AppError::Ollama(format!("Error de conexion: {e}"))
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::Ollama(
                format!("Ollama devolvio HTTP {status}: {text}")
            ));
        }

        let ollama_resp: OllamaResponse = response.json().await
            .map_err(|e| AppError::Ollama(format!("Error parseando respuesta JSON: {e}")))?;

        Ok(ollama_resp.response)
    }
}
