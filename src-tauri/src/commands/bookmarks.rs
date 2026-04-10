//! Bookmark commands — create, list, delete bookmarks anchored to session timeline.

use gravai_core::{AppState, GravaiEvent};
use std::sync::Arc;
use tauri::State;

/// Add a bookmark at the current position in the active recording session.
#[tauri::command]
pub async fn add_bookmark(
    state: State<'_, Arc<AppState>>,
    note: Option<String>,
) -> Result<serde_json::Value, String> {
    let session = state.session.read().await;
    let session = session
        .as_ref()
        .ok_or_else(|| "No active session".to_string())?;

    if !session.is_active() {
        return Err("Session is not active".into());
    }

    let offset_ms = (chrono::Utc::now() - session.started_at).num_milliseconds();

    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let bookmark_id = db
        .insert_bookmark(&session.id, offset_ms, note.as_deref())
        .map_err(|e| e.to_string())?;

    state.event_bus.publish(GravaiEvent::BookmarkCreated {
        session_id: session.id.clone(),
        bookmark_id,
        offset_ms,
        note: note.clone(),
    });

    Ok(serde_json::json!({
        "id": bookmark_id,
        "session_id": session.id,
        "offset_ms": offset_ms,
        "note": note,
    }))
}

/// List all bookmarks for a session, ordered by timeline offset.
#[tauri::command]
pub async fn list_bookmarks(
    session_id: String,
) -> Result<Vec<gravai_storage::BookmarkRecord>, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    db.list_bookmarks(&session_id).map_err(|e| e.to_string())
}

/// Delete a bookmark by its ID.
#[tauri::command]
pub async fn delete_bookmark(bookmark_id: i64) -> Result<bool, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    db.delete_bookmark(bookmark_id).map_err(|e| e.to_string())
}
