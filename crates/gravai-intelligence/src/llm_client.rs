//! OpenAI-compatible LLM client for Ollama, OpenAI, Anthropic.
//!
//! Ported from ears-rust-api analysis/llm_client.rs.

use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tracing::{debug, warn};

pub struct LlmClient {
    base_url: String,
    model: String,
    api_key: Option<String>,
    client: Client,
}

impl LlmClient {
    pub fn new(config: &gravai_config::LlmConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        let api_key = if config.provider == "ollama" {
            None
        } else {
            // For BYOK providers, the API key would come from config or keychain
            None
        };

        Self {
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            api_key,
            client,
        }
    }

    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }

    /// Send a chat completion request. Returns the assistant's response text.
    pub async fn chat(
        &self,
        messages: &[serde_json::Value],
        max_tokens: u32,
        temperature: f64,
    ) -> Result<String, String> {
        let url = format!("{}/chat/completions", self.base_url);

        let payload = json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
        });

        debug!("LLM request to {} ({} messages)", url, messages.len());

        let mut req = self.client.post(&url).json(&payload);
        if let Some(ref key) = self.api_key {
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
