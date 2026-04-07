//! AI intelligence commands: summarize, diarize.

use gravai_core::AppState;
use gravai_intelligence::SummarizationProvider;
use std::sync::Arc;
use tauri::State;
use tracing::info;

/// Generate a summary for a session's transcript.
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

    // Build transcript text
    let transcript: String = utterances
        .iter()
        .map(|u| {
            format!(
                "[{}] {}: {}",
                u.timestamp,
                u.speaker.as_deref().unwrap_or(&u.source),
                u.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    info!(
        "Summarizing session {session_id} ({} utterances, {} chars)",
        utterances.len(),
        transcript.len()
    );

    // Create LLM provider and summarize
    let provider = gravai_intelligence::summarization::LlmSummarizationProvider::new(&config.llm);
    let summary = provider
        .summarize(&transcript, None)
        .await
        .map_err(|e| e.to_string())?;

    // Store summary in DB
    let summary_json = serde_json::to_value(&summary).map_err(|e| e.to_string())?;

    info!("Summary generated for session {session_id}");
    Ok(summary_json)
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
