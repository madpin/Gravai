//! Ask Gravai — RAG-based conversational interface over meeting transcripts.

use crate::llm_client::LlmClient;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Max characters of transcript context to include in a single request.
/// Prevents overflowing the model's context window.
const MAX_CONTEXT_CHARS: usize = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCitation {
    pub session_id: String,
    pub utterance_id: i64,
    pub timestamp: String,
    pub text_snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub answer: String,
    pub citations: Vec<ChatCitation>,
}

/// Generate an answer grounded in retrieved transcript context.
///
/// `prior_messages` are previous turns from this conversation in
/// `[{"role": "user"|"assistant", "content": "..."}]` format. They are
/// injected between the system prompt and the current user turn so the
/// model has conversational memory.
pub async fn ask_gravai(
    client: &LlmClient,
    question: &str,
    context_utterances: &[serde_json::Value],
    prior_messages: &[serde_json::Value],
    max_tokens: u32,
) -> Result<ChatResponse, String> {
    // Build context, truncating if necessary to stay within the context window.
    let mut context_text = String::new();
    let mut all_citations = Vec::new();

    for (i, u) in context_utterances.iter().enumerate() {
        let text = u["text"].as_str().unwrap_or("");
        let session_id = u["session_id"].as_str().unwrap_or("");
        let timestamp = u["timestamp"].as_str().unwrap_or("");
        let source = u["source"].as_str().unwrap_or("");
        let speaker = u["speaker"].as_str().unwrap_or(source);

        let line = format!("[{i}] [{timestamp}] {speaker}: {text}\n");
        if context_text.len() + line.len() > MAX_CONTEXT_CHARS {
            break;
        }
        context_text.push_str(&line);

        all_citations.push((
            i,
            ChatCitation {
                session_id: session_id.to_string(),
                utterance_id: u["id"].as_i64().unwrap_or(0),
                timestamp: timestamp.to_string(),
                text_snippet: text.chars().take(120).collect(),
            },
        ));
    }

    let system_prompt = "You are Gravai, an AI meeting assistant. \
Answer the user's question based ONLY on the transcript excerpts provided. \
If the answer is not in the transcripts, say so clearly. \
When you reference a transcript entry, include its [number] inline. \
Be concise and factual.";

    // Build the message list: system → prior turns → current user turn (with context)
    let mut messages: Vec<serde_json::Value> =
        vec![serde_json::json!({"role": "system", "content": system_prompt})];

    // Include the last N prior turns to keep the prompt bounded.
    // Each turn = 2 messages (user + assistant); we include up to 6 turns = 12 messages.
    let prior_start = prior_messages.len().saturating_sub(12);
    messages.extend_from_slice(&prior_messages[prior_start..]);

    let user_content = if context_text.is_empty() {
        question.to_string()
    } else {
        format!("Transcript context:\n{context_text}\nQuestion: {question}")
    };
    messages.push(serde_json::json!({"role": "user", "content": user_content}));

    info!(
        "Ask Gravai: {} context items, {} prior messages",
        context_utterances.len(),
        prior_messages.len(),
    );

    let answer = client.chat(&messages, max_tokens, 0.3).await?;

    // Only include citations whose [number] was actually referenced in the answer.
    let citations = all_citations
        .into_iter()
        .filter(|(i, _)| answer.contains(&format!("[{i}]")))
        .map(|(_, c)| c)
        .collect();

    Ok(ChatResponse { answer, citations })
}
