//! Ask Gravai — RAG-based conversational interface over meeting transcripts.

use crate::llm_client::LlmClient;
use serde::{Deserialize, Serialize};
use tracing::info;

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
pub async fn ask_gravai(
    client: &LlmClient,
    question: &str,
    context_utterances: &[serde_json::Value],
) -> Result<ChatResponse, String> {
    // Build context from retrieved utterances
    let mut context_text = String::new();
    let mut citations = Vec::new();

    for (i, u) in context_utterances.iter().enumerate() {
        let text = u["text"].as_str().unwrap_or("");
        let session_id = u["session_id"].as_str().unwrap_or("");
        let timestamp = u["timestamp"].as_str().unwrap_or("");
        let source = u["source"].as_str().unwrap_or("");
        let speaker = u["speaker"].as_str().unwrap_or(source);

        context_text.push_str(&format!("[{i}] [{timestamp}] {speaker}: {text}\n"));

        citations.push(ChatCitation {
            session_id: session_id.to_string(),
            utterance_id: u["id"].as_i64().unwrap_or(0),
            timestamp: timestamp.to_string(),
            text_snippet: text.chars().take(100).collect(),
        });
    }

    let system_prompt = r#"You are Gravai, an AI meeting assistant. Answer the user's question based ONLY on the transcript excerpts provided below.
If the answer cannot be found in the transcripts, say so.
Reference specific transcript entries by their [number] when quoting.
Be concise and factual."#;

    let user_prompt = format!("Transcript context:\n{context_text}\n\nQuestion: {question}");

    let messages = vec![
        serde_json::json!({"role": "system", "content": system_prompt}),
        serde_json::json!({"role": "user", "content": user_prompt}),
    ];

    info!(
        "Ask Gravai: {} ({} context items)",
        question,
        context_utterances.len()
    );

    let answer = client.chat(&messages, 500, 0.3).await?;

    Ok(ChatResponse { answer, citations })
}
