//! LLM client — dispatches between local (mistral.rs GGUF) and API (OpenAI-compatible HTTP).

use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tracing::{debug, warn};

use crate::local_engine;

/// Unified LLM client with two backends.
pub enum LlmClient {
    /// In-process GGUF inference via mistral.rs.
    /// Stores only the model_id; the engine is resolved per-call from the global
    /// singleton so a poisoned/dead engine can be replaced transparently.
    Local { model_id: String },
    /// Any OpenAI-compatible HTTP endpoint.
    Api {
        base_url: String,
        model: String,
        api_key: Option<String>,
        client: Client,
    },
}

impl LlmClient {
    /// Construct from config. For "local", validates the model exists but does NOT
    /// eagerly load the engine — that happens on first `chat()` call.
    pub async fn new(config: &gravai_config::LlmConfig) -> Result<Self, String> {
        match config.provider.as_str() {
            "local" => {
                // Validate the model can be resolved (file exists / known catalog id).
                // Don't load the engine yet — lazy load on first chat().
                local_engine::validate_model(&config.local_model)?;
                Ok(LlmClient::Local {
                    model_id: config.local_model.clone(),
                })
            }
            "api" => {
                let client = Client::builder()
                    .timeout(Duration::from_secs(120))
                    .build()
                    .unwrap_or_default();
                let api_key = config.api_key.clone().filter(|k| !k.is_empty());
                Ok(LlmClient::Api {
                    base_url: config.base_url.clone(),
                    model: config.model.clone(),
                    api_key,
                    client,
                })
            }
            other => Err(format!(
                "Unknown LLM provider: '{other}'. Use 'local' or 'api'."
            )),
        }
    }

    /// Send a chat completion request. Returns the assistant's response text.
    pub async fn chat(
        &self,
        messages: &[serde_json::Value],
        max_tokens: u32,
        temperature: f64,
    ) -> Result<String, String> {
        match self {
            LlmClient::Local { model_id } => {
                let engine = local_engine::get_or_load_engine(model_id).await?;
                match engine.chat(messages, max_tokens, temperature).await {
                    Ok(content) => Ok(content),
                    Err(e) => {
                        // If inference fails (channel closed, poison error, etc.),
                        // the engine is likely dead. Force-unload so the next call
                        // creates a fresh instance.
                        if e.contains("channel") || e.contains("poison") || e.contains("closed") {
                            warn!("LLM engine appears dead, unloading: {e}");
                            local_engine::unload_engine().await;
                        }
                        Err(e)
                    }
                }
            }
            LlmClient::Api {
                base_url,
                model,
                api_key,
                client,
            } => {
                let url = format!("{base_url}/chat/completions");

                let payload = json!({
                    "model": model,
                    "messages": messages,
                    "max_tokens": max_tokens,
                    "temperature": temperature,
                });

                debug!("LLM request to {} ({} messages)", url, messages.len());

                let mut req = client.post(&url).json(&payload);
                if let Some(key) = api_key {
                    req = req.header("Authorization", format!("Bearer {key}"));
                }

                let response = req.send().await.map_err(|e| {
                    let msg = format!("LLM request failed: {e}");
                    warn!("{msg}");
                    msg
                })?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    let msg = format!("LLM error {status}: {}", &body[..body.len().min(200)]);
                    warn!("{msg}");
                    return Err(msg);
                }

                let data: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| format!("LLM response parse error: {e}"))?;

                let content = data["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .to_string();

                if content.is_empty() {
                    return Err("LLM returned empty response".into());
                }

                debug!("LLM response: {}...", &content[..content.len().min(100)]);
                Ok(content)
            }
        }
    }
}
