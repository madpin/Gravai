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

/// LLM-based summarization provider (local GGUF or OpenAI-compatible API).
pub struct LlmSummarizationProvider {
    client: LlmClient,
}

impl LlmSummarizationProvider {
    pub async fn new(config: &gravai_config::LlmConfig) -> Result<Self, String> {
        Ok(Self {
            client: LlmClient::new(config).await?,
        })
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
    // 1. Direct JSON parse — works when the model is well-behaved.
    if let Ok(summary) = serde_json::from_str::<MeetingSummary>(response.trim()) {
        return Ok(summary);
    }

    // 2. Strip ```json … ``` (or any ``` fence) and try again.
    if let Some(unfenced) = strip_code_fence(response) {
        if let Ok(summary) = serde_json::from_str::<MeetingSummary>(unfenced.trim()) {
            return Ok(summary);
        }
    }

    // 3. Find the largest balanced `{ … }` block in the response and try each
    //    candidate, longest first. Handles "Sure! Here's the summary: { … }".
    for candidate in extract_json_objects(response) {
        if let Ok(summary) = serde_json::from_str::<MeetingSummary>(&candidate) {
            return Ok(summary);
        }
    }

    // 4. Last resort: the LLM gave us prose. Use the first ~3 sentences as
    //    the TL;DR rather than dumping the whole response into the field.
    warn!("Could not parse structured summary, using raw response");
    let tldr = first_sentences(response, 3);
    Ok(MeetingSummary {
        tldr,
        key_decisions: Vec::new(),
        action_items: Vec::new(),
        open_questions: Vec::new(),
    })
}

/// Remove a single triple-backtick code fence wrapper (with optional language
/// tag) if the response is wrapped in one. Returns `None` if no fence found.
fn strip_code_fence(s: &str) -> Option<&str> {
    let s = s.trim();
    let after_open = s.strip_prefix("```")?;
    // Drop optional language tag on the same line as opening fence.
    let after_lang = after_open.find('\n').map(|i| &after_open[i + 1..])?;
    let inner = after_lang.trim_end();
    inner.strip_suffix("```").map(|s| s.trim())
}

/// Walk the string and return all balanced `{...}` substrings (longest first).
/// Handles strings/escapes correctly so quotes inside don't throw off the
/// brace counter.
fn extract_json_objects(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let mut results: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = find_matching_brace(bytes, i) {
                if let Ok(slice) = std::str::from_utf8(&bytes[i..=end]) {
                    results.push(slice.to_string());
                }
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    results.sort_by_key(|b| std::cmp::Reverse(b.len()));
    results
}

fn find_matching_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if escape {
                escape = false;
            } else if c == b'\\' {
                escape = true;
            } else if c == b'"' {
                in_string = false;
            }
        } else {
            match c {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn first_sentences(s: &str, n: usize) -> String {
    let trimmed = s.trim();
    let mut count = 0;
    let mut end = trimmed.len();
    for (idx, ch) in trimmed.char_indices() {
        if matches!(ch, '.' | '!' | '?' | '\n') {
            count += 1;
            if count >= n {
                end = idx + ch.len_utf8();
                break;
            }
        }
    }
    trimmed[..end].trim().to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json() {
        let r = r#"{"tldr":"hi","key_decisions":[],"action_items":[],"open_questions":[]}"#;
        let s = parse_summary_response(r).unwrap();
        assert_eq!(s.tldr, "hi");
    }

    #[test]
    fn parses_fenced_json() {
        let r = "```json\n{\"tldr\":\"hi\",\"key_decisions\":[],\"action_items\":[],\"open_questions\":[]}\n```";
        let s = parse_summary_response(r).unwrap();
        assert_eq!(s.tldr, "hi");
    }

    #[test]
    fn parses_json_with_prose_preamble() {
        let r = "Sure! Here's the summary:\n{\"tldr\":\"hi\",\"key_decisions\":[\"x\"],\"action_items\":[],\"open_questions\":[]}\nLet me know if you need anything else.";
        let s = parse_summary_response(r).unwrap();
        assert_eq!(s.tldr, "hi");
        assert_eq!(s.key_decisions, vec!["x"]);
    }

    #[test]
    fn falls_back_to_first_sentences_when_no_json() {
        let r = "First sentence here. Second one. Third! Fourth one is too long.";
        let s = parse_summary_response(r).unwrap();
        assert!(s.tldr.starts_with("First sentence here."));
        assert!(!s.tldr.contains("Fourth"));
    }

    #[test]
    fn handles_braces_inside_strings() {
        let r = r#"{"tldr":"a {fake} brace","key_decisions":[],"action_items":[],"open_questions":[]}"#;
        let s = parse_summary_response(r).unwrap();
        assert_eq!(s.tldr, "a {fake} brace");
    }
}
