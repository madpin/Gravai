//! Tauri command handlers — bridge between frontend and core crates.

mod audio;
mod config_extras;
mod export;
mod intelligence;
mod models;
mod search;
mod session;
mod storage;
mod tools;

pub use audio::*;
pub use config_extras::*;
pub use export::*;
pub use intelligence::*;
pub use models::*;
pub use search::*;
pub use session::*;
pub use storage::*;
pub use tools::*;

use gravai_core::AppState;
use std::sync::Arc;
use tauri::State;

/// Get current application status.
#[tauri::command]
pub async fn get_app_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let session = state.session.read().await;
    let session_info = match session.as_ref() {
        Some(s) => serde_json::json!({
            "id": s.id,
            "state": s.state().as_str(),
            "started_at": s.started_at.to_rfc3339(),
            "duration_seconds": s.duration_seconds(),
        }),
        None => serde_json::json!(null),
    };

    Ok(serde_json::json!({
        "session": session_info,
        "active_profile": *state.active_profile.read().await,
        "active_preset": *state.active_preset.read().await,
    }))
}

/// Get current configuration.
#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let config = state.config.read().await;
    serde_json::to_value(&*config).map_err(|e| e.to_string())
}

/// Update configuration with a partial JSON patch (deep merge).
#[tauri::command]
pub async fn update_config(
    state: State<'_, Arc<AppState>>,
    patch: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut config = state.config.write().await;

    // Deep merge patch into current config
    let current = serde_json::to_value(&*config).map_err(|e| e.to_string())?;
    let merged = gravai_config::deep_merge(&current, &patch);
    *config = serde_json::from_value(merged).map_err(|e| e.to_string())?;

    // Persist to disk
    gravai_config::save_config(&config).map_err(|e| e.to_string())?;

    serde_json::to_value(&*config).map_err(|e| e.to_string())
}

/// Export full configuration as JSON string (for backup/sharing).
#[tauri::command]
pub async fn export_config(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let config = state.config.read().await;
    serde_json::to_string_pretty(&*config).map_err(|e| e.to_string())
}

/// Import configuration from a JSON string (full replace).
#[tauri::command]
pub async fn import_config(
    state: State<'_, Arc<AppState>>,
    json: String,
) -> Result<serde_json::Value, String> {
    let new_config: gravai_config::AppConfig =
        serde_json::from_str(&json).map_err(|e| format!("Invalid config JSON: {e}"))?;

    let mut config = state.config.write().await;
    *config = new_config;
    gravai_config::save_config(&config).map_err(|e| e.to_string())?;

    serde_json::to_value(&*config).map_err(|e| e.to_string())
}

/// Get recent log lines from the ring buffer.
#[tauri::command]
pub async fn get_recent_logs() -> Result<Vec<String>, String> {
    Ok(gravai_core::logging::recent_logs())
}

/// Get preflight health report.
#[tauri::command]
pub async fn get_health_report(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let config = state.config.read().await;
    let report = gravai_core::preflight::run_preflight_checks(&config);
    serde_json::to_value(&report).map_err(|e| e.to_string())
}
