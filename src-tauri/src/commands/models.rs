//! Model management commands: list, download status, trigger downloads, LLM engine lifecycle.

use gravai_core::AppState;
use std::sync::Arc;
use tauri::{Emitter, State};
use tracing::info;

/// All available Whisper models with sizes.
const WHISPER_MODELS: &[(&str, &str, u64)] = &[
    ("tiny", "Tiny — 75 MB, fastest", 75_000_000),
    ("base", "Base — 142 MB", 142_000_000),
    ("small", "Small — 466 MB", 466_000_000),
    ("medium", "Medium — 1.5 GB, balanced", 1_500_000_000),
    (
        "large-v3-turbo",
        "Large v3 Turbo — 1.5 GB, 3–5× faster than large-v3, recommended for Apple Silicon",
        1_600_000_000,
    ),
    (
        "large-v3",
        "Large v3 — 3 GB, highest accuracy",
        3_000_000_000,
    ),
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

    // Embedding models for semantic search
    let embedding_models = embedding_models_status(&models_dir);

    // LLM models for local inference
    let llm_models = llm_models_status(&models_dir);

    Ok(serde_json::json!({
        "whisper_models": models,
        "silero_vad": silero,
        "ai_models": ai_models,
        "embedding_models": embedding_models,
        "llm_models": llm_models,
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
                let _ = app.emit(
                    "gravai:model-download",
                    serde_json::json!({
                        "model_id": model_id, "status": "downloading", "progress": 0, "file": fname,
                    }),
                );

                let response = match client.get(*url).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = format!("HTTP: {e}");
                        continue 'url_loop;
                    }
                };
                if !response.status().is_success() {
                    last_err = format!("HTTP {}", response.status());
                    tracing::warn!(
                        "Primary URL for {fname} returned {}, trying fallback",
                        response.status()
                    );
                    continue 'url_loop;
                }

                let total = response.content_length().unwrap_or(0);
                let temp_path = file_path.with_extension("tmp");
                let mut file = tokio::fs::File::create(&temp_path)
                    .await
                    .map_err(|e| format!("Create: {e}"))?;
                let mut stream = response.bytes_stream();
                let mut downloaded: u64 = 0;
                let mut last_pct: u64 = 0;
                let mut stream_ok = true;
                while let Some(chunk) = stream.next().await {
                    let chunk = match chunk {
                        Ok(c) => c,
                        Err(e) => {
                            last_err = format!("Stream: {e}");
                            stream_ok = false;
                            break;
                        }
                    };
                    if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await {
                        last_err = format!("Write: {e}");
                        stream_ok = false;
                        break;
                    }
                    downloaded += chunk.len() as u64;
                    let pct = if total > 0 {
                        (downloaded * 100) / total
                    } else {
                        0
                    };
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
                tokio::fs::rename(&temp_path, &file_path)
                    .await
                    .map_err(|e| format!("Rename: {e}"))?;
                success = true;
                break;
            }

            if !success {
                return Err(format!("Download failed for {fname}: {last_err}"));
            }
        }

        let _ = app.emit(
            "gravai:model-download",
            serde_json::json!({
                "model_id": model_id, "status": "complete", "progress": 100,
            }),
        );
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

// ── LLM engine lifecycle commands ────────────────────────────────────────────

/// Get the current LLM engine status.
#[tauri::command]
pub async fn get_llm_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let config = state.config.read().await;
    let loaded_model = gravai_intelligence::local_engine::engine_status().await;
    Ok(serde_json::json!({
        "provider": config.llm.provider,
        "loaded": loaded_model.is_some(),
        "model_id": loaded_model,
        "configured_model": config.llm.local_model,
    }))
}

/// Eagerly load the local LLM engine for a given model ID.
#[tauri::command]
pub async fn preload_llm_engine(model_id: String) -> Result<String, String> {
    info!("Preloading LLM engine: {model_id}");
    gravai_intelligence::local_engine::get_or_load_engine(&model_id).await?;
    Ok(format!("LLM engine loaded: {model_id}"))
}

/// Unload the local LLM engine to free RAM/VRAM.
#[tauri::command]
pub async fn unload_llm_engine() -> Result<String, String> {
    gravai_intelligence::local_engine::unload_engine().await;
    Ok("LLM engine unloaded".into())
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
        // ── Semantic search / embedding models ────────────────────────────────
        "all-minilm" => Some(AiModelInfo {
            subdir: "embeddings/all-minilm",
            files: &[
                (
                    "model.onnx",
                    "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx",
                    None,
                ),
                (
                    "tokenizer.json",
                    "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main/tokenizer.json",
                    None,
                ),
            ],
            description: "all-MiniLM-L6-v2 — fast English semantic search, 384-dim (22 MB)",
            approx_size: 23_000_000,
            note: None,
        }),
        "gemma-embed" => Some(AiModelInfo {
            subdir: "embeddings/gemma-embed",
            files: &[
                (
                    "model.onnx",
                    "https://huggingface.co/Xenova/nomic-embed-text-v1.5/resolve/main/onnx/model.onnx",
                    // Fallback to v1 if v1.5 not available
                    Some("https://huggingface.co/Xenova/nomic-embed-text-v1/resolve/main/onnx/model.onnx"),
                ),
                (
                    "tokenizer.json",
                    "https://huggingface.co/Xenova/nomic-embed-text-v1.5/resolve/main/tokenizer.json",
                    Some("https://huggingface.co/Xenova/nomic-embed-text-v1/resolve/main/tokenizer.json"),
                ),
            ],
            description: "EmbeddingGemma (nomic-embed-text-v1.5) — balanced multilingual, 768-dim (274 MB)",
            approx_size: 274_000_000,
            note: None,
        }),
        "bge-m3" => Some(AiModelInfo {
            subdir: "embeddings/bge-m3",
            files: &[
                (
                    "model.onnx",
                    "https://huggingface.co/onnx-community/bge-m3/resolve/main/onnx/model.onnx",
                    None,
                ),
                (
                    "tokenizer.json",
                    "https://huggingface.co/onnx-community/bge-m3/resolve/main/tokenizer.json",
                    None,
                ),
            ],
            description: "BGE-M3 — best multilingual quality, 1024-dim (572 MB)",
            approx_size: 572_000_000,
            note: None,
        }),
        // ── Sentiment and diarization ──────────────────────────────────────────
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
        // ── LLM models (local GGUF inference via mistral.rs) ─────────────────
        // Note: Gemma 4 GGUF architecture is NOT supported by mistral.rs 0.8.x.
        // Gemma 4 GGUF files will appear as "Custom" with a warning if downloaded.
        "qwen3-0.6b" => Some(AiModelInfo {
            subdir: "llm",
            files: &[(
                "Qwen3-0.6B-Q4_K_M.gguf",
                "https://huggingface.co/unsloth/Qwen3-0.6B-GGUF/resolve/main/Qwen3-0.6B-Q4_K_M.gguf",
                None,
            )],
            description: "Qwen3 0.6B Q4 — tiny, instant, ~0.5 GB RAM",
            approx_size: 490_000_000,
            note: None,
        }),
        "qwen3-1.7b" => Some(AiModelInfo {
            subdir: "llm",
            files: &[(
                "Qwen3-1.7B-Q4_K_M.gguf",
                "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf",
                None,
            )],
            description: "Qwen3 1.7B Q4 — fast, lightweight, ~1.2 GB RAM",
            approx_size: 1_200_000_000,
            note: None,
        }),
        "qwen3-4b" => Some(AiModelInfo {
            subdir: "llm",
            files: &[(
                "Qwen3-4B-Q4_K_M.gguf",
                "https://huggingface.co/unsloth/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf",
                None,
            )],
            description: "Qwen3 4B Q4 — good balance of speed and quality, ~2.6 GB RAM",
            approx_size: 2_600_000_000,
            note: None,
        }),
        "qwen3-8b" => Some(AiModelInfo {
            subdir: "llm",
            files: &[(
                "Qwen3-8B-Q4_K_M.gguf",
                "https://huggingface.co/unsloth/Qwen3-8B-GGUF/resolve/main/Qwen3-8B-Q4_K_M.gguf",
                None,
            )],
            description: "Qwen3 8B Q4 — strong multilingual, ~5 GB RAM",
            approx_size: 5_000_000_000,
            note: None,
        }),
        "llama-3.2-3b" => Some(AiModelInfo {
            subdir: "llm",
            files: &[(
                "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
                "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
                None,
            )],
            description: "Llama 3.2 3B Q4 — compact, fast, ~2 GB RAM",
            approx_size: 2_020_000_000,
            note: None,
        }),
        "phi3-mini-q4" => Some(AiModelInfo {
            subdir: "llm",
            files: &[(
                "Phi-3-mini-4k-instruct-Q4_K_M.gguf",
                "https://huggingface.co/bartowski/Phi-3-mini-4k-instruct-GGUF/resolve/main/Phi-3-mini-4k-instruct-Q4_K_M.gguf",
                None,
            )],
            description: "Phi-3 Mini 4K Q4 — 3.8B params, ~2.4 GB RAM",
            approx_size: 2_300_000_000,
            note: None,
        }),
        "mistral-7b-q4" => Some(AiModelInfo {
            subdir: "llm",
            files: &[(
                "Mistral-7B-Instruct-v0.3-Q4_K_M.gguf",
                "https://huggingface.co/bartowski/Mistral-7B-Instruct-v0.3-GGUF/resolve/main/Mistral-7B-Instruct-v0.3-Q4_K_M.gguf",
                None,
            )],
            description: "Mistral 7B Instruct v0.3 Q4 — proven quality, ~4.5 GB RAM",
            approx_size: 4_370_000_000,
            note: None,
        }),
        _ => None,
    }
}

fn embedding_models_status(models_dir: &std::path::Path) -> Vec<serde_json::Value> {
    ["all-minilm", "gemma-embed", "bge-m3"]
        .iter()
        .filter_map(|&id| {
            let info = ai_model_info(id)?;
            let dir = models_dir.join(info.subdir);
            let downloaded = info
                .files
                .iter()
                .all(|(fname, _, _)| dir.join(fname).exists());
            let actual_size: u64 = info
                .files
                .iter()
                .filter_map(|(fname, _, _)| {
                    let p = dir.join(fname);
                    if p.exists() {
                        std::fs::metadata(&p).map(|m| m.len()).ok()
                    } else {
                        None
                    }
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

fn ai_models_status(models_dir: &std::path::Path) -> Vec<serde_json::Value> {
    ["go-emotions", "pyannote-segmentation"]
        .iter()
        .filter_map(|&id| {
            let info = ai_model_info(id)?;
            let dir = models_dir.join(info.subdir);
            // Consider downloaded only if all required files exist
            let downloaded = info
                .files
                .iter()
                .all(|(fname, _, _)| dir.join(fname).exists());
            let actual_size: u64 = info
                .files
                .iter()
                .filter_map(|(fname, _, _)| {
                    let p = dir.join(fname);
                    if p.exists() {
                        std::fs::metadata(&p).map(|m| m.len()).ok()
                    } else {
                        None
                    }
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

/// LLM GGUF model IDs (pre-downloaded GGUF files).
const LLM_GGUF_IDS: &[&str] = &[
    "qwen3-0.6b",
    "qwen3-1.7b",
    "qwen3-4b",
    "qwen3-8b",
    "llama-3.2-3b",
    "phi3-mini-q4",
    "mistral-7b-q4",
];

/// HF-ISQ models — loaded from HuggingFace, quantized on-the-fly, cached as UQFF.
/// (id, description, note_if_not_cached)
const LLM_HF_ISQ_IDS: &[(&str, &str, &str)] = &[
    (
        "gemma-4-e2b",
        "Gemma 4 E2B — fast, ~3 GB RAM (auto-downloads from HuggingFace)",
        "First use downloads ~2 GB and quantizes locally. Cached for instant reload after that.",
    ),
    (
        "gemma-4-e4b",
        "Gemma 4 E4B — balanced quality, ~5 GB RAM (auto-downloads from HuggingFace)",
        "First use downloads ~5 GB and quantizes locally. Cached for instant reload after that.",
    ),
];

fn llm_models_status(models_dir: &std::path::Path) -> Vec<serde_json::Value> {
    let llm_dir = models_dir.join("llm");
    let uqff_dir = llm_dir.join("uqff");
    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut known_files: std::collections::HashSet<String> = std::collections::HashSet::new();

    // HF-ISQ models (Gemma 4) — always "available", downloaded on demand
    for &(id, desc, note) in LLM_HF_ISQ_IDS {
        let cached = uqff_dir.join(format!("{id}.uqff")).exists();
        let actual_size: u64 = if cached {
            std::fs::metadata(uqff_dir.join(format!("{id}.uqff")))
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            0
        };
        results.push(serde_json::json!({
            "id": id,
            "description": desc,
            "approx_size": 0,
            "downloaded": true,
            "actual_size": actual_size,
            "note": if cached { serde_json::Value::Null } else { serde_json::Value::String(note.to_string()) },
            "catalog": true,
            "hf_isq": true,
            "cached": cached,
        }));
    }

    // GGUF catalog models
    for &id in LLM_GGUF_IDS {
        if let Some(info) = ai_model_info(id) {
            let dir = models_dir.join(info.subdir);
            let downloaded = info
                .files
                .iter()
                .all(|(fname, _, _)| dir.join(fname).exists());
            let actual_size: u64 = info
                .files
                .iter()
                .filter_map(|(fname, _, _)| {
                    let p = dir.join(fname);
                    known_files.insert(fname.to_string());
                    if p.exists() {
                        std::fs::metadata(&p).map(|m| m.len()).ok()
                    } else {
                        None
                    }
                })
                .sum();
            results.push(serde_json::json!({
                "id": id,
                "description": info.description,
                "approx_size": info.approx_size,
                "downloaded": downloaded,
                "actual_size": actual_size,
                "note": info.note,
                "catalog": true,
            }));
        }
    }

    // Scan for custom GGUF files not in catalog
    if llm_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&llm_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".gguf") && !known_files.contains(&name) {
                    let size = std::fs::metadata(entry.path())
                        .map(|m| m.len())
                        .unwrap_or(0);
                    // Use filename (without .gguf) as the ID
                    let id = name.trim_end_matches(".gguf").to_string();
                    let note = if name.to_lowercase().contains("gemma") {
                        Some("Gemma GGUF is not supported by the inference engine — use Qwen, Llama, Phi, or Mistral")
                    } else {
                        None
                    };
                    results.push(serde_json::json!({
                        "id": id,
                        "description": format!("Custom model — {name}"),
                        "approx_size": size,
                        "downloaded": true,
                        "actual_size": size,
                        "note": note,
                        "catalog": false,
                        "filename": name,
                    }));
                }
            }
        }
    }

    results
}

/// Download a GGUF model from a custom URL into the llm/ models directory.
#[tauri::command]
pub async fn download_llm_from_url(
    app: tauri::AppHandle,
    url: String,
    filename: String,
) -> Result<String, String> {
    let models_dir = gravai_config::models_dir();
    let llm_dir = models_dir.join("llm");
    let _ = std::fs::create_dir_all(&llm_dir);

    if !filename.ends_with(".gguf") {
        return Err("Filename must end with .gguf".into());
    }

    let path = llm_dir.join(&filename);
    if path.exists() {
        return Ok(format!("{filename} already downloaded"));
    }

    info!("Downloading custom LLM from {url} as {filename}");

    use futures_util::StreamExt;
    let client = reqwest::Client::new();
    let _ = app.emit(
        "gravai:model-download",
        serde_json::json!({
            "model_id": filename, "status": "downloading", "progress": 0, "file": filename,
        }),
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("HTTP: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let tmp = llm_dir.join(format!("{filename}.tmp"));
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("Create: {e}"))?;
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_pct: u64 = 0;

    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write: {e}"))?;
        downloaded += chunk.len() as u64;
        let pct = if total > 0 {
            (downloaded * 100 / total).min(100)
        } else {
            0
        };
        if pct != last_pct {
            last_pct = pct;
            let _ = app.emit(
                "gravai:model-download",
                serde_json::json!({
                    "model_id": filename, "status": "downloading",
                    "progress": pct, "downloaded": downloaded, "total": total, "file": filename,
                }),
            );
        }
    }
    file.flush().await.map_err(|e| format!("Flush: {e}"))?;
    drop(file);

    std::fs::rename(&tmp, &path).map_err(|e| format!("Rename: {e}"))?;

    let _ = app.emit(
        "gravai:model-download",
        serde_json::json!({
            "model_id": filename, "status": "complete", "progress": 100,
        }),
    );

    info!("Custom LLM downloaded: {filename} ({downloaded} bytes)");
    Ok(format!("Downloaded {filename}"))
}

/// Delete a specific LLM model file from the llm/ directory.
#[tauri::command]
pub async fn delete_llm_model(model_id: String) -> Result<String, String> {
    let models_dir = gravai_config::models_dir();

    // Check catalog first
    if let Some(info) = ai_model_info(&model_id) {
        if info.subdir == "llm" {
            let dir = models_dir.join("llm");
            for (fname, _, _) in info.files {
                let p = dir.join(fname);
                if p.exists() {
                    std::fs::remove_file(&p).map_err(|e| format!("Delete: {e}"))?;
                }
            }
            info!("Deleted LLM model: {model_id}");
            return Ok(format!("Deleted {model_id}"));
        }
    }

    // Custom model: try filename directly, or with .gguf suffix
    let llm_dir = models_dir.join("llm");
    let candidates = [
        llm_dir.join(format!("{model_id}.gguf")),
        llm_dir.join(&model_id),
    ];
    for p in &candidates {
        if p.exists() {
            std::fs::remove_file(p).map_err(|e| format!("Delete: {e}"))?;
            info!("Deleted custom LLM: {}", p.display());
            return Ok(format!("Deleted {model_id}"));
        }
    }

    Err(format!("LLM model not found: {model_id}"))
}
