//! Meeting summarization via LLM.

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::llm_client::LlmClient;
use crate::prompts;

/// Approximate prompt-input budget for the transcript itself, in characters.
///
/// Most local GGUF/ISQ models we ship target an 8K-token context window;
/// after the system prompt + JSON schema + ~1.5K tokens reserved for the
/// summary output, that leaves around ~5–6K tokens for the transcript. At
/// roughly 4 chars/token of spoken English we cap the transcript at ~22K
/// chars and let `truncate_transcript` keep the head and tail (which is
/// where most "intent" and "outcome" content sits in real meetings).
///
/// Going above this on small local models causes a quadratic blow-up in
/// KV-cache prefill that turns a 30-second summary into a 5+ minute one,
/// which is the timeout the user originally hit. Longer is fine on hosted
/// API providers, but their costs/latency favor truncation too.
const MAX_TRANSCRIPT_CHARS: usize = 22_000;

/// When we truncate, this string is inserted between the kept head and tail
/// so the model knows it is looking at an excerpt and shouldn't invent
/// content for the missing middle.
const TRUNCATION_MARKER: &str = "\n[…truncated for length…]\n";

/// Default cap on summary completion tokens when the user-config value is
/// pathologically low. Picks the larger of `config.max_tokens` and this so
/// the JSON schema can always materialize fully.
const MIN_SUMMARY_OUTPUT_TOKENS: u32 = 1_024;

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
    /// Output token budget for the summary itself. Sourced from
    /// `LlmConfig.max_tokens` and bounded below by [`MIN_SUMMARY_OUTPUT_TOKENS`].
    max_output_tokens: u32,
}

impl LlmSummarizationProvider {
    pub async fn new(config: &gravai_config::LlmConfig) -> Result<Self, String> {
        Ok(Self {
            client: LlmClient::new(config).await?,
            max_output_tokens: config.max_tokens.max(MIN_SUMMARY_OUTPUT_TOKENS),
        })
    }
}

impl SummarizationProvider for LlmSummarizationProvider {
    async fn summarize(
        &self,
        transcript: &str,
        _prompt_template: Option<&str>,
    ) -> Result<MeetingSummary, gravai_core::GravaiError> {
        let original_len = transcript.len();
        let trimmed = truncate_transcript(transcript, MAX_TRANSCRIPT_CHARS);
        if trimmed.len() < original_len {
            warn!(
                "Transcript truncated for summarization: {} -> {} chars (cap: {})",
                original_len,
                trimmed.len(),
                MAX_TRANSCRIPT_CHARS
            );
        }

        let context = serde_json::json!({
            "utterances": parse_transcript_lines(&trimmed),
        });

        let system_prompt = prompts::DEFAULT_SUMMARY_SYSTEM.to_string();
        let user_prompt = prompts::render_prompt(prompts::DEFAULT_SUMMARY_USER, &context)
            .unwrap_or_else(|_| trimmed.to_string());

        let messages = vec![
            serde_json::json!({"role": "system", "content": system_prompt}),
            serde_json::json!({"role": "user", "content": user_prompt}),
        ];

        info!(
            "Generating meeting summary ({} chars transcript, max_tokens={})",
            trimmed.len(),
            self.max_output_tokens
        );

        let response = self
            .client
            .chat(&messages, self.max_output_tokens, 0.3)
            .await
            .map_err(|e| gravai_core::GravaiError::Provider(format!("Summary LLM: {e}")))?;

        parse_summary_response(&response)
    }

    fn name(&self) -> &str {
        "llm"
    }
}

/// Trim a transcript to at most `max_chars`, keeping the beginning and end
/// (which is where introductions and conclusions live in most meetings) and
/// inserting a clear marker in the middle. Splits on UTF-8 char boundaries
/// and on newlines so we don't cut a sentence in half.
pub(crate) fn truncate_transcript(transcript: &str, max_chars: usize) -> String {
    if transcript.len() <= max_chars {
        return transcript.to_string();
    }

    // Reserve some space for the marker; split the rest 60/40 head/tail.
    let usable = max_chars.saturating_sub(TRUNCATION_MARKER.len());
    let head_budget = (usable * 60) / 100;
    let tail_budget = usable - head_budget;

    let head = take_head_chars(transcript, head_budget);
    let tail = take_tail_chars(transcript, tail_budget);

    format!("{head}{TRUNCATION_MARKER}{tail}")
}

/// Take up to `n` characters from the start of `s`, preferring to end on a
/// newline so we don't truncate mid-utterance.
fn take_head_chars(s: &str, n: usize) -> &str {
    if s.len() <= n {
        return s;
    }
    let mut end = floor_char_boundary(s, n);
    if let Some(nl) = s[..end].rfind('\n') {
        // Prefer the last full line within the budget, but only if it leaves
        // us with at least half of the budget (otherwise we'd throw away too
        // much content for a marginal alignment win).
        if nl >= n / 2 {
            end = nl;
        }
    }
    &s[..end]
}

/// Take up to `n` characters from the end of `s`, preferring to start on a
/// newline so we don't begin mid-utterance.
fn take_tail_chars(s: &str, n: usize) -> &str {
    if s.len() <= n {
        return s;
    }
    let target = s.len().saturating_sub(n);
    let mut start = ceil_char_boundary(s, target);
    if let Some(nl) = s[start..].find('\n') {
        let candidate = start + nl + 1;
        if candidate <= s.len() && (s.len() - candidate) >= n / 2 {
            start = candidate;
        }
    }
    &s[start..]
}

/// `str::floor_char_boundary` polyfill (still nightly-only on stable as of
/// today). Returns the largest valid char boundary `<= idx`.
fn floor_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// `str::ceil_char_boundary` polyfill. Returns the smallest valid char
/// boundary `>= idx`.
fn ceil_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
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
        let r =
            r#"{"tldr":"a {fake} brace","key_decisions":[],"action_items":[],"open_questions":[]}"#;
        let s = parse_summary_response(r).unwrap();
        assert_eq!(s.tldr, "a {fake} brace");
    }

    #[test]
    fn truncate_passes_through_short_input() {
        let s = "short transcript";
        assert_eq!(truncate_transcript(s, 1024), s);
    }

    #[test]
    fn truncate_keeps_head_and_tail_with_marker() {
        // Build a transcript that's clearly bigger than the cap.
        let line = "[00:00:01] mic: hello there this is a sample line\n";
        let big: String = line.repeat(2000);
        let cap = 4_000;
        let out = truncate_transcript(&big, cap);
        assert!(out.len() <= cap + TRUNCATION_MARKER.len());
        assert!(out.contains(TRUNCATION_MARKER.trim()));
        // Head and tail both come from the original lines.
        assert!(out.starts_with("[00:00:01] mic:"));
        assert!(out.trim_end().ends_with("sample line"));
    }

    #[test]
    fn truncate_respects_utf8_boundaries() {
        // A multi-byte char near the truncation point shouldn't panic.
        let head: String = "ééé\n".repeat(500);
        let tail: String = "ààà\n".repeat(500);
        let big = format!("{head}{tail}");
        let out = truncate_transcript(&big, 1_500);
        // Output is still valid UTF-8 (implicit by being a String) and
        // contains the marker.
        assert!(out.contains(TRUNCATION_MARKER.trim()));
    }
}
