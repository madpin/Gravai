//! Search, embeddings, and Ask Gravai chat commands.

use gravai_core::AppState;
use std::sync::Arc;
use tauri::State;
use tracing::{error, info};

/// Generate embeddings for a session's utterances.
#[tauri::command]
pub async fn generate_embeddings(session_id: String) -> Result<usize, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let utterances = db.get_utterances(&session_id).map_err(|e| e.to_string())?;

    if utterances.is_empty() {
        return Ok(0);
    }

    let embedder = gravai_intelligence::embeddings::create_embedder();
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
    query: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;

    let embedder = gravai_intelligence::embeddings::create_embedder();
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
pub async fn hybrid_search(query: String) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;

    // FTS5 results
    let fts_results = db.search_utterances(&query).map_err(|e| e.to_string())?;

    // Semantic results
    let embedder = gravai_intelligence::embeddings::create_embedder();
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
) -> Result<serde_json::Value, String> {
    let config = state.config.read().await;
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;

    // Retrieve relevant context
    let context_utterances = if let Some(ref sid) = session_id {
        // Per-session: get all utterances from this session
        let utts = db.get_utterances(sid).map_err(|e| e.to_string())?;
        utts.iter()
            .map(|u| serde_json::to_value(u).unwrap_or_default())
            .collect::<Vec<_>>()
    } else {
        // Cross-archive: semantic search for relevant utterances
        let embedder = gravai_intelligence::embeddings::create_embedder();
        let query_vec = embedder.embed(&question).map_err(|e| e.to_string())?;
        let results = db
            .semantic_search(&query_vec, 15)
            .map_err(|e| e.to_string())?;
        results
            .iter()
            .map(|(u, _)| serde_json::to_value(u).unwrap_or_default())
            .collect::<Vec<_>>()
    };

    let client = gravai_intelligence::LlmClient::new(&config.llm);
    let response = gravai_intelligence::chat::ask_gravai(&client, &question, &context_utterances)
        .await
        .map_err(|e| e.to_string())?;

    // Save to chat history
    let citations_json = serde_json::to_string(&response.citations).ok();
    let _ = db.save_chat_message(session_id.as_deref(), "user", &question, None);
    let _ = db.save_chat_message(
        session_id.as_deref(),
        "assistant",
        &response.answer,
        citations_json.as_deref(),
    );

    serde_json::to_value(&response).map_err(|e| e.to_string())
}

/// Get chat history.
#[tauri::command]
pub async fn get_chat_history(session_id: Option<String>) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let history = db
        .get_chat_history(session_id.as_deref(), 50)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!(history))
}
