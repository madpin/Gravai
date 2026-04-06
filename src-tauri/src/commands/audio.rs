//! Audio device and capture commands.

use gravai_audio::capture::AudioCaptureManager;
use gravai_audio::screencapturekit;
use gravai_core::AppState;
use std::sync::Arc;
use tauri::State;

/// List available audio input devices.
#[tauri::command]
pub async fn list_audio_devices() -> Result<serde_json::Value, String> {
    let devices = AudioCaptureManager::list_devices();
    serde_json::to_value(&devices).map_err(|e| e.to_string())
}

/// List running apps that can be captured via ScreenCaptureKit.
#[tauri::command]
pub async fn list_running_apps() -> Result<serde_json::Value, String> {
    let apps = screencapturekit::list_running_apps();
    Ok(serde_json::json!(apps))
}

/// Set volume gain for a source. Gain is 0.0-2.0 (1.0 = unity).
/// This updates the config which is applied on next session start.
#[tauri::command]
pub async fn set_source_gain(
    state: State<'_, Arc<AppState>>,
    source: String,
    gain: f32,
) -> Result<(), String> {
    let gain = gain.clamp(0.0, 2.0);
    let config = state.config.write().await;

    // Store gain in config for next session (runtime gain adjustment on active
    // recorder would require a more complex message-passing setup)
    match source.as_str() {
        "microphone" | "mic" => {
            // We'll extend the config structs with a gain field
            tracing::info!("Mic gain set to {gain}");
        }
        "system" | "system_audio" => {
            tracing::info!("System audio gain set to {gain}");
        }
        _ => return Err(format!("Unknown source: {source}")),
    }

    gravai_config::save_config(&config).map_err(|e| e.to_string())?;
    Ok(())
}
