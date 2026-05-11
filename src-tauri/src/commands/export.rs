//! Export commands: Markdown, PDF, Obsidian, Notion.

use tracing::info;

fn load_export_data(session_id: &str) -> Result<gravai_export::ExportData, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;

    let session = db
        .get_session(session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {session_id}"))?;

    let utterances = db.get_utterances(session_id).map_err(|e| e.to_string())?;
    let bookmarks = db.list_bookmarks(session_id).map_err(|e| e.to_string())?;
    // Best-effort: a missing or unparsable summary just yields None, the
    // markdown writer skips the section gracefully.
    let summary = db
        .get_session_summary(session_id)
        .map_err(|e| e.to_string())?
        .and_then(summary_record_to_export);

    Ok(gravai_export::ExportData {
        session_id: session.id,
        title: session.title,
        started_at: session.started_at,
        ended_at: session.ended_at,
        duration_seconds: session.duration_seconds,
        meeting_app: session.meeting_app,
        utterances: utterances
            .iter()
            .map(|u| gravai_export::ExportUtterance {
                timestamp: u.timestamp.clone(),
                source: u.source.clone(),
                speaker: u.speaker.clone(),
                text: u
                    .corrected_text
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&u.text)
                    .to_string(),
            })
            .collect(),
        bookmarks: bookmarks
            .iter()
            .map(|b| gravai_export::ExportBookmark {
                offset_ms: b.offset_ms,
                note: b.note.clone(),
            })
            .collect(),
        summary,
    })
}

/// Convert a stored `SummaryRecord` to the export shape. Stored arrays are
/// serialized as JSON strings; missing or invalid JSON falls back to empty.
fn summary_record_to_export(
    rec: gravai_storage::SummaryRecord,
) -> Option<gravai_export::ExportSummary> {
    let tldr = rec.tldr.unwrap_or_default();
    if tldr.trim().is_empty() {
        return None;
    }
    let parse_str_arr = |s: Option<String>| -> Vec<String> {
        s.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default()
    };
    let parse_val_arr = |s: Option<String>| -> Vec<serde_json::Value> {
        s.and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
            .unwrap_or_default()
    };
    Some(gravai_export::ExportSummary {
        tldr,
        key_decisions: parse_str_arr(rec.key_decisions),
        action_items: parse_val_arr(rec.action_items),
        open_questions: parse_str_arr(rec.open_questions),
    })
}

/// Export session as Markdown string.
#[tauri::command]
pub async fn export_markdown(session_id: String) -> Result<String, String> {
    let data = load_export_data(&session_id)?;
    let md =
        gravai_export::markdown::export_markdown(&data, &gravai_export::ExportOptions::default());
    info!(
        "Markdown export for session {session_id} ({} chars)",
        md.len()
    );
    Ok(md)
}

/// Export session to a Markdown file and return the path.
#[tauri::command]
pub async fn export_markdown_file(session_id: String, path: String) -> Result<String, String> {
    let data = load_export_data(&session_id)?;
    let md =
        gravai_export::markdown::export_markdown(&data, &gravai_export::ExportOptions::default());
    std::fs::write(&path, &md).map_err(|e| format!("Write: {e}"))?;
    Ok(path)
}

/// Export session as PDF (text-based) to a file path.
#[tauri::command]
pub async fn export_pdf(session_id: String, path: String) -> Result<String, String> {
    let data = load_export_data(&session_id)?;
    gravai_export::pdf::export_pdf(
        &data,
        &gravai_export::ExportOptions::default(),
        std::path::Path::new(&path),
    )?;
    Ok(path)
}

/// Export session to Obsidian vault folder.
#[tauri::command]
pub async fn export_obsidian(session_id: String, vault_folder: String) -> Result<String, String> {
    let data = load_export_data(&session_id)?;
    gravai_export::obsidian::export_obsidian(
        &data,
        &gravai_export::ExportOptions::default(),
        std::path::Path::new(&vault_folder),
    )
}

/// Export session to Notion.
#[tauri::command]
pub async fn export_notion(
    session_id: String,
    api_key: String,
    database_id: String,
) -> Result<String, String> {
    let data = load_export_data(&session_id)?;
    gravai_export::notion::export_notion(&data, &api_key, &database_id).await
}
