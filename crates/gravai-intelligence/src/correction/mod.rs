//! Async transcript correction via LLM, guided by a user-managed knowledge base.

use std::collections::HashMap;

use tracing::{info, warn};

use crate::llm_client::LlmClient;
use crate::prompts;

/// Corrects ASR transcript errors using an LLM and knowledge base entries.
pub struct TranscriptCorrectionProvider {
    client: LlmClient,
    pub provider_name: String,
    /// Effective system prompt (custom if set, otherwise built-in default).
    system_prompt: String,
}

impl TranscriptCorrectionProvider {
    pub async fn new(
        llm_config: &gravai_config::LlmConfig,
        model_override: Option<&str>,
        custom_prompt: Option<&str>,
    ) -> Result<Self, String> {
        let mut cfg = llm_config.clone();
        // Model override only applies to API provider (local always uses the loaded engine).
        if cfg.provider == "api" {
            if let Some(m) = model_override {
                cfg.model = m.to_string();
            }
        }
        let provider_name = match cfg.provider.as_str() {
            "local" => format!("local/{}", cfg.local_model),
            _ => format!("api/{}", cfg.model),
        };
        let system_prompt = custom_prompt
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(prompts::CORRECTION_SYSTEM)
            .to_string();
        Ok(Self {
            client: LlmClient::new(&cfg).await?,
            provider_name,
            system_prompt,
        })
    }

    /// Correct a batch of utterances using knowledge entries.
    /// Returns a map of utterance_id -> corrected_text for utterances where a correction differs.
    pub async fn correct(
        &self,
        utterances: &[gravai_storage::UtteranceRecord],
        knowledge: &[gravai_storage::KnowledgeEntry],
    ) -> Result<HashMap<i64, String>, String> {
        if utterances.is_empty() {
            return Ok(HashMap::new());
        }

        // Build context for Jinja template — each entry is a titled text block
        let knowledge_ctx: Vec<serde_json::Value> = knowledge
            .iter()
            .filter_map(|e| {
                let text = e.context.as_deref().unwrap_or("").trim().to_string();
                if text.is_empty() {
                    return None;
                }
                Some(serde_json::json!({
                    "title": e.name,
                    "text": text,
                }))
            })
            .collect();

        let utterances_ctx: Vec<serde_json::Value> = utterances
            .iter()
            .map(|u| {
                serde_json::json!({
                    "id": u.id,
                    "speaker": u.speaker,
                    "text": u.text,
                })
            })
            .collect();

        let context = serde_json::json!({
            "knowledge": knowledge_ctx,
            "utterances": utterances_ctx,
        });

        let user_prompt = prompts::render_prompt(prompts::CORRECTION_USER, &context)
            .map_err(|e| format!("Correction prompt render: {e}"))?;

        let messages = vec![
            serde_json::json!({"role": "system", "content": &self.system_prompt}),
            serde_json::json!({"role": "user", "content": user_prompt}),
        ];

        info!(
            "Correcting {} utterances (knowledge entries: {})",
            utterances.len(),
            knowledge.len()
        );

        let response = self
            .client
            .chat(&messages, 2048, 0.1)
            .await
            .map_err(|e| format!("Correction LLM: {e}"))?;

        Ok(parse_correction_response(&response))
    }
}

/// Parse LLM response lines of the form `[123] corrected text here`.
fn parse_correction_response(response: &str) -> HashMap<i64, String> {
    let mut corrections = HashMap::new();
    for line in response.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Expect format: [<id>] <text>  (speaker prefix may or may not be present)
        if let Some(rest) = line.strip_prefix('[') {
            if let Some(bracket_end) = rest.find(']') {
                let id_str = &rest[..bracket_end];
                if let Ok(id) = id_str.parse::<i64>() {
                    let corrected = rest[bracket_end + 1..].trim();
                    // Strip optional "Speaker: " prefix if present (we want just the text)
                    let text = if let Some(colon_pos) = corrected.find(": ") {
                        // Only strip if it looks like a short speaker prefix (< 40 chars)
                        if colon_pos < 40 {
                            &corrected[colon_pos + 2..]
                        } else {
                            corrected
                        }
                    } else {
                        corrected
                    };
                    if !text.is_empty() {
                        corrections.insert(id, text.to_string());
                    }
                } else {
                    warn!("Correction response: could not parse id from '{}'", id_str);
                }
            }
        }
    }
    corrections
}
