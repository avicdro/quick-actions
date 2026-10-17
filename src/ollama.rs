use reqwest;
use serde::{Deserialize, Serialize};

use crate::config::OllamaConfig;
use crate::error::{AppError, Result};

/// Cliente para comunicarse con la API de Ollama.
pub struct OllamaClient {
    config: OllamaConfig,
    client: reqwest::Client,
}

/// Deriva la base URL del endpoint de generacion.
///
/// `http://localhost:11434/api/generate` -> `http://localhost:11434`
/// Si no contiene `/api/`, devuelve el endpoint tal cual.
pub fn endpoint_base(endpoint: &str) -> String {
    match endpoint.find("/api/") {
        Some(i) => endpoint[..i].to_string(),
        None => endpoint.trim_end_matches('/').to_string(),
    }
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
    pub async fn generate(&self, system_prompt: &str, user_text: &str) -> Result<String> {
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
            return Err(AppError::Ollama(format!(
                "Ollama devolvio HTTP {status}: {text}"
            )));
        }

        let ollama_resp: OllamaResponse = response
            .json()
            .await
            .map_err(|e| AppError::Ollama(format!("Error parseando respuesta JSON: {e}")))?;

        Ok(ollama_resp.response)
    }

    /// Lista los modelos instalados en Ollama (`GET /api/tags`).
    ///
    /// Devuelve los nombres tal cual los reporta Ollama (con tag, ej.
    /// `gemma4:cloud`). Vacio si la API responde sin modelos.
    ///
    /// # Errors
    ///
    /// Si no se puede conectar (Ollama caido) o el HTTP falla.
    pub async fn list_models(config: &OllamaConfig) -> Result<Vec<String>> {
        let url = format!("{}/api/tags", endpoint_base(&config.endpoint));

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let response = client.get(&url).send().await.map_err(|e| {
            if e.is_connect() {
                AppError::Ollama(
                    "Ollama no responde (ollama serve caido?). No se puede listar modelos."
                        .to_string(),
                )
            } else {
                AppError::Ollama(format!("Error listando modelos: {e}"))
            }
        })?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(AppError::Ollama(format!(
                "Ollama devolvio HTTP {status} al listar modelos"
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AppError::Ollama(format!("Error parseando /api/tags: {e}")))?;

        let names = json
            .get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                    .map(std::string::ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();

        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_base_strips_api_path() {
        assert_eq!(
            endpoint_base("http://localhost:11434/api/generate"),
            "http://localhost:11434"
        );
    }

    #[test]
    fn endpoint_base_without_api_returns_as_is() {
        assert_eq!(
            endpoint_base("http://localhost:11434"),
            "http://localhost:11434"
        );
        assert_eq!(endpoint_base("http://host:8080/"), "http://host:8080");
    }
}
