//! Meeting summarization via LLM.

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::llm_client::LlmClient;
use crate::prompts;

/// Structured meeting summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingSummary {
    pub tldr: String,
    pub key_decisions: Vec<String>,
    pub action_items: Vec<ActionItem>,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionItem {
    pub description: String,
    pub owner: Option<String>,
}

/// Provider trait for meeting summarization.
#[allow(async_fn_in_trait)]
pub trait SummarizationProvider: Send + Sync {
    async fn summarize(
        &self,
        transcript: &str,
        prompt_template: Option<&str>,
    ) -> Result<MeetingSummary, gravai_core::GravaiError>;
    fn name(&self) -> &str;
}

/// LLM-based summarization provider (works with Ollama, OpenAI, Anthropic).
pub struct LlmSummarizationProvider {
    client: LlmClient,
}

impl LlmSummarizationProvider {
    pub fn new(config: &gravai_config::LlmConfig) -> Self {
        Self {
            client: LlmClient::new(config),
        }
    }
}

impl SummarizationProvider for LlmSummarizationProvider {
    async fn summarize(
        &self,
        transcript: &str,
        _prompt_template: Option<&str>,
    ) -> Result<MeetingSummary, gravai_core::GravaiError> {
        let context = serde_json::json!({
            "utterances": parse_transcript_lines(transcript),
        });

        let system_prompt = prompts::DEFAULT_SUMMARY_SYSTEM.to_string();
        let user_prompt = prompts::render_prompt(prompts::DEFAULT_SUMMARY_USER, &context)
            .unwrap_or_else(|_| transcript.to_string());

        let messages = vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user", "content": user_prompt}),
        ];

        info!(
            "Generating meeting summary ({} chars transcript)",
            transcript.len()
        );

        let response = self
            .client
            .chat(&messages, 1000, 0.3)
            .await
            .map_err(|e| gravai_core::GravaiError::Provider(format!("Summary LLM: {e}")))?;

        // Try to parse JSON response
        parse_summary_response(&response)
    }

    fn name(&self) -> &str {
        "llm"
    }
}

fn parse_summary_response(response: &str) -> Result<MeetingSummary, gravai_core::GravaiError> {
    // Try direct JSON parse
    if let Ok(summary) = serde_json::from_str::<MeetingSummary>(response) {
        return Ok(summary);
    }

    // Try extracting JSON from markdown fences
    let json_str = response
        .find('{')
        .and_then(|start| response.rfind('}').map(|end| &response[start..=end]));

    if let Some(json) = json_str {
        if let Ok(summary) = serde_json::from_str::<MeetingSummary>(json) {
            return Ok(summary);
        }
    }

    // Fallback: use the whole response as TL;DR
    warn!("Could not parse structured summary, using raw response");
    Ok(MeetingSummary {
        tldr: response.to_string(),
        key_decisions: Vec::new(),
        action_items: Vec::new(),
        open_questions: Vec::new(),
    })
}

fn parse_transcript_lines(transcript: &str) -> Vec<serde_json::Value> {
    transcript
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            serde_json::json!({
                "text": line.trim(),
                "timestamp": "",
                "source": "",
            })
        })
        .collect()
}
