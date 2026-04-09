//! Knowledge base CRUD commands and correction settings for transcript correction.

use gravai_storage::{Database, KnowledgeEntry};

/// List knowledge entries. Pass `active_only: true` to exclude inactive entries.
#[tauri::command]
pub async fn list_knowledge(active_only: Option<bool>) -> Result<Vec<KnowledgeEntry>, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = Database::open(&db_path).map_err(|e| e.to_string())?;
    db.list_knowledge_entries(active_only.unwrap_or(false))
        .map_err(|e| e.to_string())
}

/// Create or update a knowledge entry.
/// If `entry.id == 0`, a new entry is inserted and its id is returned.
/// If `entry.id > 0`, the existing entry is updated.
#[tauri::command]
pub async fn upsert_knowledge(entry: KnowledgeEntry) -> Result<i64, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = Database::open(&db_path).map_err(|e| e.to_string())?;
    if entry.id == 0 {
        db.insert_knowledge_entry(&entry).map_err(|e| e.to_string())
    } else {
        db.update_knowledge_entry(&entry)
            .map_err(|e| e.to_string())?;
        Ok(entry.id)
    }
}

/// Delete a knowledge entry by id. Returns true if the entry existed.
#[tauri::command]
pub async fn delete_knowledge(id: i64) -> Result<bool, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = Database::open(&db_path).map_err(|e| e.to_string())?;
    db.delete_knowledge_entry(id).map_err(|e| e.to_string())
}

/// Return the built-in default correction system prompt so the frontend can show it.
#[tauri::command]
pub async fn get_correction_defaults() -> serde_json::Value {
    serde_json::json!({
        "default_system_prompt": gravai_intelligence::prompts::CORRECTION_SYSTEM,
    })
}
