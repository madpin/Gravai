//! Search, embeddings, and Ask Gravai chat commands.

use gravai_core::AppState;
use std::sync::Arc;
use tauri::State;
use tracing::{error, info};

/// Generate embeddings for a session's utterances.
#[tauri::command]
pub async fn generate_embeddings(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<usize, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let utterances = db.get_utterances(&session_id).map_err(|e| e.to_string())?;

    if utterances.is_empty() {
        return Ok(0);
    }

    let config = state.config.read().await;
    let embedder = gravai_intelligence::embeddings::create_embedder_from_config(&config.embedding);
    let mut count = 0;

    for u in &utterances {
        match embedder.embed(&u.text) {
            Ok(vec) => {
                if let Err(e) = db.store_embedding(u.id, &session_id, &vec) {
                    error!("Store embedding: {e}");
                } else {
                    count += 1;
                }
            }
            Err(e) => error!("Embed: {e}"),
        }
    }

    info!("Generated {count} embeddings for session {session_id}");
    Ok(count)
}

/// Semantic search across all sessions.
#[tauri::command]
pub async fn semantic_search(
    state: State<'_, Arc<AppState>>,
    query: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;

    let config = state.config.read().await;
    let embedder = gravai_intelligence::embeddings::create_embedder_from_config(&config.embedding);
    let query_vec = embedder.embed(&query).map_err(|e| e.to_string())?;

    let results = db
        .semantic_search(&query_vec, limit.unwrap_or(10))
        .map_err(|e| e.to_string())?;

    let items: Vec<serde_json::Value> = results
        .iter()
        .map(|(u, score)| {
            serde_json::json!({
                "utterance": u,
                "score": score,
            })
        })
        .collect();

    Ok(serde_json::json!(items))
}

/// Hybrid search: combine FTS5 and semantic results.
#[tauri::command]
pub async fn hybrid_search(
    state: State<'_, Arc<AppState>>,
    query: String,
) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;

    // FTS5 results
    let fts_results = db.search_utterances(&query).map_err(|e| e.to_string())?;

    // Semantic results
    let config = state.config.read().await;
    let embedder = gravai_intelligence::embeddings::create_embedder_from_config(&config.embedding);
    let query_vec = embedder.embed(&query).map_err(|e| e.to_string())?;
    let sem_results = db
        .semantic_search(&query_vec, 20)
        .map_err(|e| e.to_string())?;

    // Merge: FTS results first, then semantic results not already in FTS
    let mut seen_ids = std::collections::HashSet::new();
    let mut merged = Vec::new();

    for u in &fts_results {
        seen_ids.insert(u.id);
        merged.push(serde_json::json!({ "utterance": u, "source": "keyword", "score": 1.0 }));
    }
    for (u, score) in &sem_results {
        if !seen_ids.contains(&u.id) {
            seen_ids.insert(u.id);
            merged
                .push(serde_json::json!({ "utterance": u, "source": "semantic", "score": score }));
        }
    }

    Ok(serde_json::json!(merged))
}

/// Search sessions with filters.
#[tauri::command]
pub async fn search_sessions_filtered(
    date_from: Option<String>,
    date_to: Option<String>,
    meeting_app: Option<String>,
) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let sessions = db
        .search_sessions_filtered(
            None,
            date_from.as_deref(),
            date_to.as_deref(),
            meeting_app.as_deref(),
        )
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&sessions).map_err(|e| e.to_string())
}

/// Ask Gravai: RAG-based Q&A over transcripts.
#[tauri::command]
pub async fn ask_gravai(
    state: State<'_, Arc<AppState>>,
    question: String,
    session_id: Option<String>,
    conversation_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let config = state.config.read().await;
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;

    // Auto-create conversation if none provided, titling from the question
    let conv_id = match conversation_id {
        Some(cid) => cid,
        None => {
            let title: String = question.chars().take(60).collect();
            db.create_conversation(session_id.as_deref(), Some(&title))
                .map_err(|e| e.to_string())?
        }
    };

    // Always search across ALL sessions so that names and references are
    // found regardless of which session is currently selected.
    // When a session_id is provided it is used only for prioritisation:
    // utterances from that session are surfaced first, then global results fill
    // the remaining context slots.
    //
    // FTS5 is tried first (exact name / phrase matching), then semantic search
    // adds topically similar content that keyword search might miss.
    let context_utterances = {
        // FTS5 keyword results across all sessions
        let fts_results = db.search_utterances(&question).unwrap_or_default();

        // Semantic results across all sessions
        let embedder =
            gravai_intelligence::embeddings::create_embedder_from_config(&config.embedding);
        let query_vec = embedder.embed(&question).map_err(|e| e.to_string())?;
        let sem_results = db.semantic_search(&query_vec, 20).unwrap_or_default();

        let mut seen_ids = std::collections::HashSet::new();
        let mut merged: Vec<serde_json::Value> = Vec::new();

        // Priority 1 — FTS hits from the focused session (if any)
        for u in &fts_results {
            if session_id.as_deref() == Some(u.session_id.as_str()) && seen_ids.insert(u.id) {
                merged.push(serde_json::to_value(u).unwrap_or_default());
            }
        }
        // Priority 2 — semantic hits from the focused session (if any)
        for (u, _) in &sem_results {
            if session_id.as_deref() == Some(u.session_id.as_str()) && seen_ids.insert(u.id) {
                merged.push(serde_json::to_value(u).unwrap_or_default());
            }
        }
        // Priority 3 — FTS hits from all other sessions
        for u in &fts_results {
            if seen_ids.insert(u.id) {
                merged.push(serde_json::to_value(u).unwrap_or_default());
            }
        }
        // Priority 4 — semantic hits from all other sessions
        for (u, _) in &sem_results {
            if seen_ids.insert(u.id) {
                merged.push(serde_json::to_value(u).unwrap_or_default());
            }
        }

        // Cap to keep the LLM context window manageable
        merged.truncate(30);
        merged
    };

    // Load prior conversation turns so the LLM has memory of this session.
    let prior_messages: Vec<serde_json::Value> = db
        .get_chat_history(Some(&conv_id), None, 500)
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "role": m["role"],
                "content": m["content"],
            })
        })
        .collect();

    let max_tokens = config.llm.max_tokens;
    let client = gravai_intelligence::LlmClient::new(&config.llm);
    let response = gravai_intelligence::chat::ask_gravai(
        &client,
        &question,
        &context_utterances,
        &prior_messages,
        max_tokens,
    )
    .await
    .map_err(|e| e.to_string())?;

    // Persist both turns to this conversation
    let citations_json = serde_json::to_string(&response.citations).ok();
    let _ = db.save_chat_message(
        Some(&conv_id),
        session_id.as_deref(),
        "user",
        &question,
        None,
    );
    let _ = db.save_chat_message(
        Some(&conv_id),
        session_id.as_deref(),
        "assistant",
        &response.answer,
        citations_json.as_deref(),
    );

    Ok(serde_json::json!({
        "answer": response.answer,
        "citations": response.citations,
        "conversation_id": conv_id,
    }))
}

/// Get chat history for a conversation or session.
#[tauri::command]
pub async fn get_chat_history(
    conversation_id: Option<String>,
    session_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let history = db
        .get_chat_history(conversation_id.as_deref(), session_id.as_deref(), 500)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!(history))
}

/// Create a new chat conversation.
#[tauri::command]
pub async fn create_chat_conversation(session_id: Option<String>) -> Result<String, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    db.create_conversation(session_id.as_deref(), None)
        .map_err(|e| e.to_string())
}

/// List all chat conversations.
#[tauri::command]
pub async fn list_chat_conversations() -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let convs = db.list_conversations().map_err(|e| e.to_string())?;
    Ok(serde_json::json!(convs))
}

/// Delete a chat conversation (cascades to its messages).
#[tauri::command]
pub async fn delete_chat_conversation(conversation_id: String) -> Result<(), String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    db.delete_conversation(&conversation_id)
        .map_err(|e| e.to_string())
}

/// Rename a chat conversation.
#[tauri::command]
pub async fn rename_chat_conversation(
    conversation_id: String,
    title: String,
) -> Result<(), String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    db.rename_conversation(&conversation_id, &title)
        .map_err(|e| e.to_string())
}

/// Export a conversation as a Markdown string.
#[tauri::command]
pub async fn export_chat_markdown(conversation_id: String) -> Result<String, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;

    let convs = db.list_conversations().map_err(|e| e.to_string())?;
    let conv = convs.iter().find(|c| c["id"] == conversation_id);
    let title = conv.and_then(|c| c["title"].as_str()).unwrap_or("Chat");
    let date = conv.and_then(|c| c["created_at"].as_str()).unwrap_or("");
    let scope = conv
        .and_then(|c| c["session_id"].as_str())
        .map(|s| format!("Session: {s}"))
        .unwrap_or_else(|| "Scope: All meetings".to_string());

    let history = db
        .get_chat_history(Some(&conversation_id), None, 1000)
        .map_err(|e| e.to_string())?;

    let mut md = format!("# Chat — {title}\n{date}\n{scope}\n\n---\n\n");
    for msg in &history {
        let role = msg["role"].as_str().unwrap_or("unknown");
        let content = msg["content"].as_str().unwrap_or("");
        if role == "user" {
            md.push_str(&format!("**You:** {content}\n\n"));
        } else {
            md.push_str(&format!("**Gravai:** {content}\n"));
            if let Some(citations) = msg["citations"].as_array() {
                for c in citations {
                    let snippet = c["text_snippet"].as_str().unwrap_or("");
                    if !snippet.is_empty() {
                        md.push_str(&format!("> 📎 {snippet}\n"));
                    }
                }
            }
            md.push('\n');
        }
    }
    Ok(md)
}

/// Export a conversation as a Markdown file at the given path.
#[tauri::command]
pub async fn export_chat_markdown_file(
    conversation_id: String,
    path: String,
) -> Result<String, String> {
    let md = export_chat_markdown(conversation_id).await?;
    std::fs::write(&path, &md).map_err(|e| format!("Write: {e}"))?;
    Ok(path)
}
