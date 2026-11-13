//! ML model download and management.
//!
//! Ported from ears-rust-api downloader.rs, adapted for Gravai paths.

use std::path::PathBuf;

use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use tracing::{info, warn};

pub fn models_dir() -> PathBuf {
    gravai_config::models_dir()
}

/// Ensure all required models are downloaded.
pub async fn ensure_models(config: &gravai_config::AppConfig) {
    let dir = models_dir();
    let _ = std::fs::create_dir_all(&dir);

    // Download Whisper model
    let model_name = &config.transcription.model;
    let whisper_file = dir.join(format!("ggml-{model_name}.bin"));
    let whisper_url =
        format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model_name}.bin");
    download_if_missing(
        &whisper_file,
        &whisper_url,
        &format!("Whisper {model_name}"),
    )
    .await;

    // Download Silero VAD if configured
    if config.vad.engine == "silero" {
        download_if_missing(&dir.join(SILERO_VAD_FILE), SILERO_VAD_URL, "Silero VAD").await;
    }
}

pub const SILERO_VAD_FILE: &str = "silero_vad.onnx";
const SILERO_VAD_URL: &str =
    "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx";

/// Download the Silero voice-detection model into the models folder.
pub async fn download_silero_vad() -> Result<(), String> {
    let dir = models_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(SILERO_VAD_FILE);
    if path.exists() {
        return Ok(());
    }
    download_file(SILERO_VAD_URL, &path, "Silero VAD").await
}

async fn download_if_missing(path: &PathBuf, url: &str, label: &str) {
    if path.exists() {
        info!("{label} already downloaded: {}", path.display());
        return;
    }

    info!("Downloading {label} from {url}...");
    match download_file(url, path, label).await {
        Ok(()) => info!("{label} downloaded to {}", path.display()),
        Err(e) => warn!("Failed to download {label}: {e}"),
    }
}

async fn download_file(url: &str, path: &PathBuf, label: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HTTP request: {e}"))?;

    let total_size = response.content_length();

    let pb = ProgressBar::new(total_size.unwrap_or(0));
    pb.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "{{spinner}} {label} [{{bar:40.cyan/blue}}] {{bytes}}/{{total_bytes}} ({{eta}})"
            ))
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );

    // Write to temp file, then rename
    let temp_path = path.with_extension("tmp");
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| format!("Create temp file: {e}"))?;

    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream: {e}"))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| format!("Write: {e}"))?;
        downloaded += chunk.len() as u64;
        pb.set_position(downloaded);
    }

    pb.finish_with_message("done");

    // Rename temp to final
    tokio::fs::rename(&temp_path, path)
        .await
        .map_err(|e| format!("Rename: {e}"))?;

    Ok(())
}
