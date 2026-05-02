//! AI intelligence commands: summarize, diarize.

use gravai_core::AppState;
use gravai_intelligence::SummarizationProvider;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tracing::info;

/// Generate a summary for a session's transcript.
///
/// Persists the result in `session_summaries` so subsequent requests
/// (Archive, reopen, refresh) can retrieve it cheaply via
/// `get_session_summary` instead of re-running the LLM.
#[tauri::command]
pub async fn summarize_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<serde_json::Value, String> {
    let config = state.config.read().await;

    // Load transcript from DB
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let utterances = db.get_utterances(&session_id).map_err(|e| e.to_string())?;

    if utterances.is_empty() {
        return Err("No transcript to summarize".into());
    }

    // Build transcript text — prefer corrected_text when available so the
    // summary works on the cleaned-up version of what was said.
    let transcript: String = utterances
        .iter()
        .map(|u| {
            let text = u.corrected_text.as_deref().unwrap_or(&u.text);
            format!(
                "[{}] {}: {}",
                u.timestamp,
                u.speaker.as_deref().unwrap_or(&u.source),
                text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    info!(
        "Summarizing session {session_id} ({} utterances, {} chars)",
        utterances.len(),
        transcript.len()
    );

    // Create LLM provider and summarize (with timeout to avoid hanging the UI)
    let llm_config = config.llm.clone();
    let provider_label = match llm_config.provider.as_str() {
        "local" => format!("local/{}", llm_config.local_model),
        _ => format!("api/{}", llm_config.model),
    };
    drop(config); // release read lock before async work
    let summary = tokio::time::timeout(std::time::Duration::from_secs(300), async {
        let provider =
            gravai_intelligence::summarization::LlmSummarizationProvider::new(&llm_config)
                .await
                .map_err(|e| e.to_string())?;
        provider
            .summarize(&transcript, None)
            .await
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Summary generation timed out after 5 minutes".to_string())??;

    // Persist to DB so it survives navigation/refresh
    {
        let key_decisions_json = serde_json::to_string(&summary.key_decisions).ok();
        let action_items_json = serde_json::to_string(&summary.action_items).ok();
        let open_questions_json = serde_json::to_string(&summary.open_questions).ok();
        if let Err(e) = db.upsert_session_summary(
            &session_id,
            Some(&summary.tldr),
            key_decisions_json.as_deref(),
            action_items_json.as_deref(),
            open_questions_json.as_deref(),
            Some(&provider_label),
        ) {
            tracing::warn!("Failed to persist summary for {session_id}: {e}");
        }
    }

    let summary_json = serde_json::to_value(&summary).map_err(|e| e.to_string())?;
    info!("Summary generated for session {session_id}");
    Ok(summary_json)
}

/// Retrieve the persisted summary for a session, if one has been generated.
/// Returns `null` if no summary exists yet (frontend should fall back to
/// running `summarize_session`).
#[tauri::command]
pub async fn get_session_summary(session_id: String) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let record = db
        .get_session_summary(&session_id)
        .map_err(|e| e.to_string())?;
    let Some(rec) = record else {
        return Ok(serde_json::Value::Null);
    };
    let parse_arr = |s: Option<&str>| -> serde_json::Value {
        s.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .unwrap_or_else(|| serde_json::json!([]))
    };
    Ok(serde_json::json!({
        "tldr": rec.tldr.unwrap_or_default(),
        "key_decisions": parse_arr(rec.key_decisions.as_deref()),
        "action_items": parse_arr(rec.action_items.as_deref()),
        "open_questions": parse_arr(rec.open_questions.as_deref()),
        "created_at": rec.created_at,
        "provider": rec.provider,
    }))
}

/// List available export formats for the current platform.
#[tauri::command]
pub async fn get_export_formats() -> Result<serde_json::Value, String> {
    let formats: Vec<serde_json::Value> = gravai_audio::encoder::available_formats()
        .iter()
        .map(|(id, label)| serde_json::json!({"id": id, "label": label}))
        .collect();
    Ok(serde_json::json!(formats))
}

/// Export a session's audio to a different format.
/// Merges all tracks (mic + system) into one file automatically.
#[tauri::command]
pub async fn export_session_audio(
    session_id: String,
    format: String,
    source_track: Option<String>,
) -> Result<String, String> {
    let session_dir = gravai_config::sessions_dir().join(&session_id);
    let fmt = gravai_audio::encoder::ExportFormat::parse(&format);

    if let Some(track) = source_track {
        // Export a specific track only
        let source = session_dir.join(format!("{track}.wav"));
        if !source.exists() {
            return Err(format!("Track not found: {track}.wav"));
        }
        let output = session_dir.join(format!("{track}.{}", fmt.extension()));
        gravai_audio::encoder::export_audio(&source, &output, fmt, 192)?;
        Ok(output.display().to_string())
    } else {
        // Default: merge all tracks into one export
        let output = session_dir.join(format!("export.{}", fmt.extension()));
        gravai_audio::encoder::merge_and_export(&session_dir, &output, fmt, 192)?;
        Ok(output.display().to_string())
    }
}

/// Retrieve per-speaker sentiment summary for a session.
/// Returns a list of speakers with their dominant emotion and top emotion counts.
#[tauri::command]
pub async fn get_session_sentiment(session_id: String) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let utterances = db
        .get_session_sentiment(&session_id)
        .map_err(|e| e.to_string())?;

    // Group by speaker, accumulate emotion counts
    let mut speakers: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for u in &utterances {
        let speaker = u.speaker.clone().unwrap_or_else(|| "Remote".into());
        let label = u
            .sentiment_label
            .clone()
            .unwrap_or_else(|| "neutral".into());
        let score = u.sentiment_score.unwrap_or(0.0);
        speakers
            .entry(speaker)
            .or_default()
            .push(serde_json::json!({ "label": label, "score": score }));
    }

    let summary: Vec<serde_json::Value> = speakers
        .into_iter()
        .map(|(speaker, emotions)| {
            // Count each label
            let mut counts: HashMap<String, u32> = HashMap::new();
            for e in &emotions {
                let label = e["label"].as_str().unwrap_or("neutral").to_string();
                *counts.entry(label).or_default() += 1;
            }
            let dominant = counts
                .iter()
                .max_by_key(|(_, &v)| v)
                .map(|(k, _)| k.clone())
                .unwrap_or_else(|| "neutral".into());
            serde_json::json!({
                "speaker": speaker,
                "dominant_emotion": dominant,
                "utterance_count": emotions.len(),
                "emotion_counts": counts,
            })
        })
        .collect();

    Ok(serde_json::json!({ "speakers": summary }))
}
