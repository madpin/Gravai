//! Storage management commands: disk usage, cleanup, real-time export.

use tracing::info;

/// Info about a session's storage footprint.
#[derive(serde::Serialize)]
struct SessionStorageInfo {
    session_id: String,
    title: Option<String>,
    started_at: String,
    audio_files: Vec<AudioFileInfo>,
    audio_total_bytes: u64,
    transcript_utterances: usize,
}

#[derive(serde::Serialize)]
struct AudioFileInfo {
    name: String,
    size_bytes: u64,
    path: String,
}

/// Get storage usage for all sessions.
#[tauri::command]
pub async fn get_storage_info() -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let sessions = db.list_sessions().map_err(|e| e.to_string())?;

    let sessions_dir = gravai_config::sessions_dir();
    let mut infos = Vec::new();
    let mut total_audio_bytes: u64 = 0;

    for s in &sessions {
        let session_dir = sessions_dir.join(&s.id);
        let mut audio_files = Vec::new();
        let mut session_audio_bytes: u64 = 0;

        for path in session_audio_files(&session_dir)? {
            let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
            audio_files.push(AudioFileInfo {
                name: path
                    .strip_prefix(&session_dir)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
                size_bytes: size,
                path: path.display().to_string(),
            });
            session_audio_bytes += size;
        }

        let utterance_count = db.get_utterances(&s.id).map(|u| u.len()).unwrap_or(0);
        total_audio_bytes += session_audio_bytes;

        infos.push(SessionStorageInfo {
            session_id: s.id.clone(),
            title: s.title.clone(),
            started_at: s.started_at.clone(),
            audio_files,
            audio_total_bytes: session_audio_bytes,
            transcript_utterances: utterance_count,
        });
    }

    let db_size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    Ok(serde_json::json!({
        "sessions": infos,
        "total_sessions": sessions.len(),
        "total_audio_bytes": total_audio_bytes,
        "database_bytes": db_size,
        "total_bytes": total_audio_bytes + db_size,
    }))
}

/// Delete only audio files for a session (keep transcript in DB).
#[tauri::command]
pub async fn delete_session_audio(session_id: String) -> Result<String, String> {
    let session_dir = gravai_config::sessions_dir().join(&session_id);
    if !session_dir.exists() {
        return Ok("No audio files found".into());
    }

    let mut deleted = 0u64;
    for path in session_audio_files(&session_dir)? {
        let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        deleted += size;
    }
    // Remove empty directory
    let _ = std::fs::remove_dir(&session_dir);

    info!(
        "Deleted audio for session {session_id}: {:.1} MB freed",
        deleted as f64 / 1_048_576.0
    );
    Ok(format!("{:.1} MB freed", deleted as f64 / 1_048_576.0))
}

/// Delete entire session (audio + transcript + DB records).
#[tauri::command]
pub async fn delete_full_session(session_id: String) -> Result<String, String> {
    // Delete audio files
    let session_dir = gravai_config::sessions_dir().join(&session_id);
    if session_dir.exists() {
        let _ = std::fs::remove_dir_all(&session_dir);
    }

    // Delete DB records
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    db.delete_session(&session_id).map_err(|e| e.to_string())?;

    info!("Deleted full session {session_id}");
    Ok(format!("Session {session_id} deleted"))
}

/// Save transcript to disk in real-time (crash-safe auto-save).
#[tauri::command]
pub async fn save_realtime_transcript(session_id: String) -> Result<String, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let utterances = db.get_utterances(&session_id).map_err(|e| e.to_string())?;

    if utterances.is_empty() {
        return Ok("No utterances to save".into());
    }

    let config: gravai_config::AppConfig = gravai_config::load_config();
    let export_dir = config
        .export
        .transcript_folder
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| gravai_config::data_dir().join("exports"));

    let _ = std::fs::create_dir_all(&export_dir);

    let bookmarks = db.list_bookmarks(&session_id).unwrap_or_default();
    let data = gravai_export::ExportData {
        session_id: session_id.clone(),
        title: None,
        started_at: utterances
            .first()
            .map(|u| u.timestamp.clone())
            .unwrap_or_default(),
        ended_at: None,
        duration_seconds: None,
        meeting_app: None,
        utterances: utterances
            .iter()
            .map(|u| gravai_export::ExportUtterance {
                timestamp: u.timestamp.clone(),
                source: u.source.clone(),
                speaker: u.speaker.clone(),
                text: u.text.clone(),
            })
            .collect(),
        bookmarks: bookmarks
            .iter()
            .map(|b| gravai_export::ExportBookmark {
                offset_ms: b.offset_ms,
                note: b.note.clone(),
            })
            .collect(),
        summary: None,
    };

    let md =
        gravai_export::markdown::export_markdown(&data, &gravai_export::ExportOptions::default());
    let path = export_dir.join(format!("{session_id}.md"));
    std::fs::write(&path, &md).map_err(|e| format!("Write: {e}"))?;

    Ok(path.display().to_string())
}

// Audio may be in the original directory or in later capture segments.
fn session_audio_files(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_dir() {
            files.extend(session_audio_files(&path)?);
        } else if kind.is_file()
            && path.extension().is_some_and(|ext| {
                ["wav", "aiff", "caf", "m4a", "mp3", "flac"]
                    .iter()
                    .any(|audio| ext.eq_ignore_ascii_case(audio))
            })
        {
            files.push(path);
        }
    }
    Ok(files)
}
