//! Transcription provider abstraction and implementations.

pub mod http_stub;
pub mod whisper;

use serde::{Deserialize, Serialize};

/// A single transcription segment with timing and confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub confidence: f32,
    pub language: Option<String>,
}

/// Provider trait for transcription engines.
pub trait TranscriptionProvider: Send + Sync {
    /// Transcribe an audio buffer (16kHz mono f32) into timed segments.
    fn transcribe(
        &self,
        audio_16khz_mono: &[f32],
    ) -> Result<Vec<TranscriptionSegment>, gravai_core::GravaiError>;

    /// Provider name for logging/config.
    fn name(&self) -> &str;
}

/// Create a transcription provider based on config.
pub fn create_provider(
    config: &gravai_config::TranscriptionConfig,
) -> Result<Box<dyn TranscriptionProvider>, gravai_core::GravaiError> {
    match config.engine.as_str() {
        "http" => {
            let http_config = http_stub::HttpTranscriptionConfig::default();
            Ok(Box::new(http_stub::HttpTranscriptionProvider::new(
                http_config,
            )))
        }
        // Default to Whisper for "whisper" or any other value
        _ => {
            let engine = whisper::WhisperEngine::new(config)?;
            Ok(Box::new(engine))
        }
    }
}
