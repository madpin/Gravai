//! Utility commands: silence trimming, performance monitoring.

use gravai_core::AppState;
use std::sync::Arc;
use tauri::State;

/// Detect silent regions in a session's master recording.
#[tauri::command]
pub async fn detect_silence(
    session_id: String,
    threshold_db: Option<f64>,
    min_duration_ms: Option<u64>,
) -> Result<serde_json::Value, String> {
    let session_dir = gravai_config::sessions_dir().join(&session_id);
    let wav_path = find_wav(&session_dir).ok_or("No recording found")?;

    let regions = gravai_audio::silence::detect_silence(
        &wav_path,
        threshold_db.unwrap_or(-40.0),
        min_duration_ms.unwrap_or(3000),
    )?;

    serde_json::to_value(&regions).map_err(|e| e.to_string())
}

/// Trim silent regions from a session's recording. Non-destructive (creates new file).
#[tauri::command]
pub async fn trim_silence(
    session_id: String,
    threshold_db: Option<f64>,
    min_duration_ms: Option<u64>,
) -> Result<String, String> {
    let session_dir = gravai_config::sessions_dir().join(&session_id);
    let wav_path = find_wav(&session_dir).ok_or("No recording found")?;

    let regions = gravai_audio::silence::detect_silence(
        &wav_path,
        threshold_db.unwrap_or(-40.0),
        min_duration_ms.unwrap_or(3000),
    )?;

    if regions.is_empty() {
        return Ok("No silence detected — nothing to trim".into());
    }

    let stem = wav_path.file_stem().unwrap_or_default().to_string_lossy();
    let output_path = session_dir.join(format!("{stem}_trimmed.wav"));
    gravai_audio::silence::trim_silence(&wav_path, &output_path, &regions)?;

    let trimmed_ms: u64 = regions.iter().map(|r| r.duration_ms).sum();
    Ok(format!(
        "Trimmed {}s of silence → {}",
        trimmed_ms / 1000,
        output_path.display()
    ))
}

/// Get performance snapshot (memory, uptime).
#[tauri::command]
pub async fn get_perf_snapshot(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let session_count = {
        let s = state.session.read().await;
        if s.is_some() {
            1u32
        } else {
            0u32
        }
    };
    let snapshot = gravai_core::perf::snapshot(session_count);
    serde_json::to_value(&snapshot).map_err(|e| e.to_string())
}

fn find_wav(session_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    for name in &["mic.wav", "system.wav"] {
        let p = session_dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    std::fs::read_dir(session_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .find(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
        })
        .map(|e| e.path())
}
