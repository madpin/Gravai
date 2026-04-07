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
                text: u.text.clone(),
            })
            .collect(),
        summary: None, // Could load from DB if we stored summaries
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
