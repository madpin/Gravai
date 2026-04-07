//! Model management commands: list, download status, trigger downloads.

use tauri::Emitter;
use tracing::info;

/// All available Whisper models with sizes.
const WHISPER_MODELS: &[(&str, &str, u64)] = &[
    ("tiny", "Tiny — 75 MB, fastest", 75_000_000),
    ("base", "Base — 142 MB", 142_000_000),
    ("small", "Small — 466 MB", 466_000_000),
    ("medium", "Medium — 1.5 GB, balanced", 1_500_000_000),
    ("large-v3", "Large v3 — 3 GB, best accuracy", 3_000_000_000),
];

/// Get list of all models with download status.
#[tauri::command]
pub async fn get_models_status() -> Result<serde_json::Value, String> {
    let models_dir = gravai_config::models_dir();
    let models: Vec<serde_json::Value> = WHISPER_MODELS
        .iter()
        .map(|(id, desc, approx_size)| {
            let path = models_dir.join(format!("ggml-{id}.bin"));
            let exists = path.exists();
            let actual_size = if exists {
                std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };
            // Flag as corrupted if file exists but is way too small
            let corrupted = exists && actual_size < 1_000_000;
            if corrupted {
                tracing::warn!(
                    "Model ggml-{id}.bin appears corrupted ({actual_size} bytes, expected ~{approx_size})"
                );
            }
            serde_json::json!({
                "id": id,
                "description": desc,
                "approx_size": approx_size,
                "downloaded": exists && !corrupted,
                "corrupted": corrupted,
                "actual_size": actual_size,
                "path": path.display().to_string(),
            })
        })
        .collect();

    // Also check Silero VAD
    let silero_path = models_dir.join("silero_vad.onnx");
    let silero = serde_json::json!({
        "id": "silero_vad",
        "description": "Silero VAD — voice activity detection",
        "downloaded": silero_path.exists(),
        "actual_size": if silero_path.exists() { std::fs::metadata(&silero_path).map(|m| m.len()).unwrap_or(0) } else { 0 },
    });

    Ok(serde_json::json!({
        "whisper_models": models,
        "silero_vad": silero,
        "models_dir": models_dir.display().to_string(),
    }))
}

/// Download a specific Whisper model. Emits progress events.
#[tauri::command]
pub async fn download_model(app: tauri::AppHandle, model_id: String) -> Result<String, String> {
    let models_dir = gravai_config::models_dir();
    let _ = std::fs::create_dir_all(&models_dir);

    let filename = format!("ggml-{model_id}.bin");
    let path = models_dir.join(&filename);

    if path.exists() {
        return Ok(format!("{} already downloaded", filename));
    }

    let url =
        format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model_id}.bin");

    info!("Downloading Whisper model {model_id} from {url}");

    // Emit start event
    let _ = app.emit(
        "gravai:model-download",
        serde_json::json!({
            "model_id": model_id,
            "status": "downloading",
            "progress": 0,
        }),
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("HTTP: {e}"))?;

    if !response.status().is_success() {
        let _ = app.emit(
            "gravai:model-download",
            serde_json::json!({
                "model_id": model_id,
                "status": "error",
                "message": format!("HTTP {}", response.status()),
            }),
        );
        return Err(format!("Download failed: HTTP {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let temp_path = path.with_extension("tmp");

    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| format!("Create file: {e}"))?;

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_pct: u64 = 0;

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream: {e}"))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| format!("Write: {e}"))?;
        downloaded += chunk.len() as u64;

        // Emit progress every 1%
        let pct = if total > 0 {
            (downloaded * 100) / total
        } else {
            0
        };
        if pct != last_pct {
            last_pct = pct;
            let _ = app.emit(
                "gravai:model-download",
                serde_json::json!({
                    "model_id": model_id,
                    "status": "downloading",
                    "progress": pct,
                    "downloaded": downloaded,
                    "total": total,
                }),
            );
        }
    }

    // Validate download — model files should be at least 1MB
    let min_size = 1_000_000u64;
    if downloaded < min_size {
        let _ = tokio::fs::remove_file(&temp_path).await;
        let msg = format!(
            "Download appears corrupted ({} bytes, expected > 1MB). Deleted temp file.",
            downloaded
        );
        let _ = app.emit(
            "gravai:model-download",
            serde_json::json!({ "model_id": model_id, "status": "error", "message": &msg }),
        );
        return Err(msg);
    }

    // Rename temp to final
    tokio::fs::rename(&temp_path, &path)
        .await
        .map_err(|e| format!("Rename: {e}"))?;

    let _ = app.emit(
        "gravai:model-download",
        serde_json::json!({
            "model_id": model_id,
            "status": "complete",
            "progress": 100,
        }),
    );

    info!(
        "Downloaded Whisper model {model_id} ({} MB)",
        downloaded / 1_048_576
    );
    Ok(format!(
        "Downloaded {} ({:.0} MB)",
        filename,
        downloaded as f64 / 1_048_576.0
    ))
}

/// Delete a downloaded model to free disk space.
#[tauri::command]
pub async fn delete_model(model_id: String) -> Result<String, String> {
    let path = gravai_config::models_dir().join(format!("ggml-{model_id}.bin"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Delete: {e}"))?;
        info!("Deleted model ggml-{model_id}.bin");
        Ok(format!("Deleted ggml-{model_id}.bin"))
    } else {
        Ok("Model not found".into())
    }
}
