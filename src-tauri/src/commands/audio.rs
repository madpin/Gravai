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

/// Set volume gain for a source.
#[tauri::command]
pub async fn set_source_gain(
    state: State<'_, Arc<AppState>>,
    source: String,
    gain: f32,
) -> Result<(), String> {
    let gain = gain.clamp(0.0, 2.0);
    let config = state.config.write().await;

    match source.as_str() {
        "microphone" | "mic" => tracing::info!("Mic gain set to {gain}"),
        "system" | "system_audio" => tracing::info!("System audio gain set to {gain}"),
        _ => return Err(format!("Unknown source: {source}")),
    }

    gravai_config::save_config(&config).map_err(|e| e.to_string())?;
    Ok(())
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
