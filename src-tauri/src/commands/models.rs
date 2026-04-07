//! Model management commands: list, download status, trigger downloads.

use tauri::Emitter;
use tracing::info;

/// All available Whisper models with sizes.
const WHISPER_MODELS: &[(&str, &str, u64)] = &[
    ("tiny", "Tiny — 75 MB, fastest", 75_000_000),
    ("base", "Base — 142 MB", 142_000_000),
    ("small", "Small — 466 MB", 466_000_000),
    ("medium", "Medium — 1.5 GB, balanced", 1_500_000_000),
    ("large-v3-turbo", "Large v3 Turbo — 1.5 GB, 3–5× faster than large-v3, recommended for Apple Silicon", 1_600_000_000),
    ("large-v3", "Large v3 — 3 GB, highest accuracy", 3_000_000_000),
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

    // AI models: sentiment and diarization
    let ai_models = ai_models_status(&models_dir);

    Ok(serde_json::json!({
        "whisper_models": models,
        "silero_vad": silero,
        "ai_models": ai_models,
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

    // Handle AI models (sentiment/diarization)
    if let Some(ai_info) = ai_model_info(&model_id) {
        let dir = models_dir.join(ai_info.subdir);
        let _ = std::fs::create_dir_all(&dir);

        use futures_util::StreamExt;
        for (fname, primary_url, fallback_url) in ai_info.files {
            let file_path = dir.join(fname);
            if file_path.exists() {
                continue;
            }

            // Try primary URL; on failure try fallback if available.
            let client = reqwest::Client::new();
            let mut urls_to_try: Vec<&str> = vec![primary_url];
            if let Some(fb) = fallback_url {
                urls_to_try.push(fb);
            }

            let mut last_err = String::new();
            let mut success = false;

            'url_loop: for url in &urls_to_try {
                info!("Downloading {model_id}/{fname} from {url}");
                let _ = app.emit("gravai:model-download", serde_json::json!({
                    "model_id": model_id, "status": "downloading", "progress": 0, "file": fname,
                }));

                let response = match client.get(*url).send().await {
                    Ok(r) => r,
                    Err(e) => { last_err = format!("HTTP: {e}"); continue 'url_loop; }
                };
                if !response.status().is_success() {
                    last_err = format!("HTTP {}", response.status());
                    tracing::warn!("Primary URL for {fname} returned {}, trying fallback", response.status());
                    continue 'url_loop;
                }

                let total = response.content_length().unwrap_or(0);
                let temp_path = file_path.with_extension("tmp");
                let mut file = tokio::fs::File::create(&temp_path).await.map_err(|e| format!("Create: {e}"))?;
                let mut stream = response.bytes_stream();
                let mut downloaded: u64 = 0;
                let mut last_pct: u64 = 0;
                let mut stream_ok = true;
                while let Some(chunk) = stream.next().await {
                    let chunk = match chunk {
                        Ok(c) => c,
                        Err(e) => { last_err = format!("Stream: {e}"); stream_ok = false; break; }
                    };
                    if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await {
                        last_err = format!("Write: {e}"); stream_ok = false; break;
                    }
                    downloaded += chunk.len() as u64;
                    let pct = if total > 0 { (downloaded * 100) / total } else { 0 };
                    if pct != last_pct {
                        last_pct = pct;
                        let _ = app.emit("gravai:model-download", serde_json::json!({
                            "model_id": model_id, "status": "downloading",
                            "progress": pct, "downloaded": downloaded, "total": total, "file": fname,
                        }));
                    }
                }
                if !stream_ok {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    continue 'url_loop;
                }
                tokio::fs::rename(&temp_path, &file_path).await.map_err(|e| format!("Rename: {e}"))?;
                success = true;
                break;
            }

            if !success {
                return Err(format!("Download failed for {fname}: {last_err}"));
            }
        }

        let _ = app.emit("gravai:model-download", serde_json::json!({
            "model_id": model_id, "status": "complete", "progress": 100,
        }));
        return Ok(format!("Downloaded {model_id} model files"));
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
    let models_dir = gravai_config::models_dir();

    // Handle AI model deletion (sentiment/diarization subdirs)
    if let Some(info) = ai_model_info(&model_id) {
        let dir = models_dir.join(info.subdir);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("Delete: {e}"))?;
            info!("Deleted AI model directory: {}", dir.display());
            return Ok(format!("Deleted {model_id} model"));
        }
        return Ok("Model not found".into());
    }

    // Whisper model
    let path = models_dir.join(format!("ggml-{model_id}.bin"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Delete: {e}"))?;
        info!("Deleted model ggml-{model_id}.bin");
        Ok(format!("Deleted ggml-{model_id}.bin"))
    } else {
        Ok("Model not found".into())
    }
}

struct AiModelInfo {
    subdir: &'static str,
    /// (filename, primary_url, fallback_url)
    files: &'static [(&'static str, &'static str, Option<&'static str>)],
    description: &'static str,
    approx_size: u64,
    note: Option<&'static str>,
}

fn ai_model_info(model_id: &str) -> Option<AiModelInfo> {
    match model_id {
        "go-emotions" => Some(AiModelInfo {
            subdir: "sentiment/go-emotions",
            files: &[
                (
                    "model.onnx",
                    // Dedicated ONNX export repo — publicly accessible
                    "https://huggingface.co/SamLowe/roberta-base-go_emotions-onnx/resolve/main/onnx/model.onnx",
                    None,
                ),
                (
                    "tokenizer.json",
                    "https://huggingface.co/SamLowe/roberta-base-go_emotions-onnx/resolve/main/onnx/tokenizer.json",
                    None,
                ),
            ],
            description: "RoBERTa go-emotions — 28-class emotion detection for participants",
            approx_size: 500_000_000,
            note: None,
        }),
        "pyannote-segmentation" => Some(AiModelInfo {
            subdir: "diarization",
            files: &[
                (
                    "segmentation.onnx",
                    "https://huggingface.co/onnx-community/pyannote-segmentation-3.0/resolve/main/onnx/model.onnx",
                    None,
                ),
            ],
            description: "Pyannote segmentation — speaker turn detection (requires HuggingFace login)",
            approx_size: 90_000_000,
            note: Some("Requires accepting the pyannote license at huggingface.co/pyannote/segmentation-3.0"),
        }),
        _ => None,
    }
}

fn ai_models_status(models_dir: &std::path::Path) -> Vec<serde_json::Value> {
    ["go-emotions", "pyannote-segmentation"]
        .iter()
        .filter_map(|&id| {
            let info = ai_model_info(id)?;
            let dir = models_dir.join(info.subdir);
            // Consider downloaded only if all required files exist
            let downloaded = info.files.iter().all(|(fname, _, _)| dir.join(fname).exists());
            let actual_size: u64 = info
                .files
                .iter()
                .filter_map(|(fname, _, _)| {
                    let p = dir.join(fname);
                    if p.exists() { std::fs::metadata(&p).map(|m| m.len()).ok() } else { None }
                })
                .sum();
            Some(serde_json::json!({
                "id": id,
                "description": info.description,
                "approx_size": info.approx_size,
                "downloaded": downloaded,
                "actual_size": actual_size,
                "note": info.note,
            }))
        })
        .collect()
}
