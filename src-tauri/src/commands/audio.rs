//! Audio device and capture commands.

use gravai_audio::capture::AudioCaptureManager;
use gravai_audio::screencapturekit;
use gravai_core::AppState;
use std::sync::Arc;
use tauri::State;

/// List available audio input devices (uses cpal, no Screen Recording needed).
#[tauri::command]
pub async fn list_audio_devices() -> Result<serde_json::Value, String> {
    let devices = AudioCaptureManager::list_devices();
    serde_json::to_value(&devices).map_err(|e| e.to_string())
}

/// List running apps via process list (no Screen Recording permission needed).
/// Used for meeting detection and general app listing.
#[tauri::command]
pub async fn list_running_apps() -> Result<serde_json::Value, String> {
    let apps = list_apps_via_ps();
    Ok(serde_json::json!(apps))
}

/// List running apps with bundle IDs via ScreenCaptureKit.
/// Requires Screen Recording permission. Used for the system audio app picker
/// so the correct bundle ID is passed to the per-app audio filter.
#[tauri::command]
pub async fn list_capturable_apps() -> Result<serde_json::Value, String> {
    let apps = screencapturekit::list_running_apps();
    // apps already have { name, bundle_id } from SCK
    Ok(serde_json::json!(apps))
}

/// Open a different microphone without stopping the recording.
/// `device_name` empty follows the system default input.
#[tauri::command]
pub async fn switch_microphone(
    state: State<'_, Arc<AppState>>,
    device_name: String,
) -> Result<String, String> {
    {
        let mut config = state.config.write().await;
        config.audio.microphone.device_name = device_name.clone();
        config.audio.microphone.device_index = -1;
        config.audio.microphone.enabled = true;
        gravai_config::save_config(&config).map_err(|e| e.to_string())?;
    }
    super::session::request_mic_switch(device_name)
}

/// Get the path to a session's playable audio file (compressed M4A AAC for browser compatibility).
/// Creates a `playback.m4a` file on first access (merge if multi-track, then encode).
/// Falls back to 16-bit PCM WAV if AAC encoding is unavailable.
#[tauri::command]
pub async fn get_session_audio_path(session_id: String) -> Result<String, String> {
    resolve_playback_path(session_id, false).await
}

/// Delete any cached playback/mix files for a session and rebuild from the
/// raw source tracks. For the "Rebuild audio" button shown when playback
/// fails to load — covers a stale cache left by a crash, a recording that
/// was later continued, or a corrupt encode.
#[tauri::command]
pub async fn rebuild_session_audio(session_id: String) -> Result<String, String> {
    resolve_playback_path(session_id, true).await
}

/// Shared implementation for [`get_session_audio_path`] and
/// [`rebuild_session_audio`]. Runs entirely inside one `spawn_blocking` task
/// holding this session's lock for the whole read-cache-or-rebuild decision,
/// so two concurrent requests for the same session (double-click, Archive
/// open racing an export) can't race on the same cache/temp files; different
/// sessions are not blocked by each other.
async fn resolve_playback_path(session_id: String, force_rebuild: bool) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let db = super::session::shared_db()?;
        gravai_audio::retention::resolve_replay(
            &db,
            &session_id,
            &gravai_config::sessions_dir().join(&session_id),
            force_rebuild,
        )
        .map(|p| p.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// List running GUI apps using ps (no SCK permission).
fn list_apps_via_ps() -> Vec<serde_json::Value> {
    match std::process::Command::new("ps")
        .args(["-eo", "comm="])
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut seen = std::collections::HashSet::new();
            stdout
                .lines()
                .filter_map(|line| {
                    let name = line.trim();
                    if name.is_empty() {
                        return None;
                    }
                    let short = name.rsplit('/').next().unwrap_or(name);
                    if seen.insert(short.to_string()) {
                        Some(serde_json::json!({ "name": short }))
                    } else {
                        None
                    }
                })
                .collect()
        }
        Err(_) => Vec::new(),
    }
}
