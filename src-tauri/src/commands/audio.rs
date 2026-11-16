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
        let session_dir = gravai_config::sessions_dir().join(&session_id);
        if !session_dir.exists() {
            return Err(format!("Session directory not found: {session_id}"));
        }
        let lock = gravai_audio::encoder::session_lock(&session_dir);
        let _guard = lock.lock().map_err(|_| "Audio lock poisoned".to_string())?;

        let playback_m4a = session_dir.join("playback.m4a");
        let playback_wav = session_dir.join("playback.wav");
        let master = session_dir.join("master.wav");

        if force_rebuild {
            for path in [
                &playback_m4a,
                &playback_wav,
                &master,
                &session_dir.join("_playback_timeline.wav"),
                &session_dir.join("_timeline.m4a"),
                &session_dir.join("_playback_temp.wav"),
            ] {
                let _ = std::fs::remove_file(path);
            }
        }

        if session_dir.join("continuations").exists() {
            let wav = session_dir.join("_playback_timeline.wav");
            gravai_audio::encoder::export_session_timeline(
                &session_dir,
                &wav,
                gravai_audio::encoder::ExportFormat::Wav,
                128,
            )?;
            let output = session_dir.join("_timeline.m4a");
            if let Err(error) = gravai_audio::encoder::export_audio(
                &wav,
                &output,
                gravai_audio::encoder::ExportFormat::M4aAac,
                128,
            ) {
                tracing::warn!("Timeline AAC encoding failed; using WAV: {error}");
                return Ok(wav.display().to_string());
            }
            let _ = std::fs::remove_file(wav);
            return Ok(output.display().to_string());
        }

        // A cached playback file is only reused if it's still fresh: decodable,
        // non-empty, and not older than the session's current raw source
        // tracks (a crash/recovery/continued session can add or replace audio
        // after a cache was built — that stale cache must not be served
        // forever just because it exists).
        if gravai_audio::encoder::is_fresh_cache(&playback_m4a, &session_dir) {
            return Ok(playback_m4a.display().to_string());
        }
        if gravai_audio::encoder::is_fresh_cache(&playback_wav, &session_dir) {
            return Ok(playback_wav.display().to_string());
        }

        match gravai_audio::encoder::compact_session(&session_dir, 128, false) {
            Ok(_) if gravai_audio::encoder::is_fresh_cache(&playback_m4a, &session_dir) => {
                return Ok(playback_m4a.display().to_string());
            }
            Ok(_) => {}
            Err(error) => tracing::warn!("Compact playback generation failed: {error}"),
        }

        // Clean up any broken playback files before falling back to a manual merge.
        let _ = std::fs::remove_file(&playback_m4a);
        let _ = std::fs::remove_file(&playback_wav);

        // Collect source WAV files (exclude derived files)
        let wav_files: Vec<std::path::PathBuf> = std::fs::read_dir(&session_dir)
            .map_err(|e| format!("Read dir: {e}"))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                let ext_ok = p
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"));
                if !ext_ok {
                    return false;
                }
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                !name.contains("trimmed")
                    && !name.starts_with("_mixed")
                    && !name.starts_with("export")
                    && !name.starts_with("playback")
                    && name != "master.wav"
            })
            .collect();

        if wav_files.is_empty() {
            return Err("No audio files found for this session".into());
        }

        // Step 1: Get a single source WAV (merge multiple tracks if needed).
        // The merged master is rebuilt whenever stale — never served forever
        // just because a file with that name exists.
        let source_wav = if wav_files.len() == 1 {
            wav_files[0].clone()
        } else {
            if !gravai_audio::encoder::is_fresh_cache(&master, &session_dir) {
                tracing::info!(
                    "Merging {} tracks for playback in {}",
                    wav_files.len(),
                    session_id
                );
                let _ = std::fs::remove_file(&master);
                gravai_audio::encoder::merge_and_export(
                    &session_dir,
                    &master,
                    gravai_audio::encoder::ExportFormat::Wav,
                    0,
                )?;
            }
            master.clone()
        };

        // Step 2: Encode to M4A AAC (compressed, browser-compatible)
        // Falls back to 16-bit PCM WAV if afconvert is unavailable
        tracing::info!("Creating playback file for {session_id}");

        // First ensure we have a 16-bit PCM WAV (afconvert needs valid input, and
        // browsers need PCM if we fall back to WAV)
        let pcm_temp = session_dir.join("_playback_temp.wav");
        gravai_audio::encoder::ensure_pcm16_wav(&source_wav, &pcm_temp)?;

        // Try M4A AAC via afconvert (macOS)
        match gravai_audio::encoder::export_audio(
            &pcm_temp,
            &playback_m4a,
            gravai_audio::encoder::ExportFormat::M4aAac,
            128,
        ) {
            Ok(()) => {
                let _ = std::fs::remove_file(&pcm_temp);
                Ok(playback_m4a.display().to_string())
            }
            Err(e) => {
                tracing::warn!("M4A encoding failed ({e}), falling back to PCM WAV");
                // Use the PCM temp as the playback file
                std::fs::rename(&pcm_temp, &playback_wav).map_err(|e| format!("Rename: {e}"))?;
                Ok(playback_wav.display().to_string())
            }
        }
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
