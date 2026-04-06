//! Stub for a future external HTTP transcription provider.
//!
//! This module defines the shape of an HTTP-based transcription provider
//! that sends audio to a user-supplied endpoint and receives timed segments.
//! Not yet functional — placeholder for Phase 3+ when external engines are wired.

use crate::TranscriptionSegment;

/// Configuration for an external HTTP transcription endpoint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HttpTranscriptionConfig {
    /// Base URL of the transcription service (e.g. "http://localhost:9000")
    pub base_url: String,
    /// Optional API key for authentication
    pub api_key: Option<String>,
    /// Model identifier to request (service-specific)
    pub model: Option<String>,
    /// Language hint
    pub language: Option<String>,
    /// Request timeout in seconds
    pub timeout_seconds: u32,
}

impl Default for HttpTranscriptionConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:9000".into(),
            api_key: None,
            model: None,
            language: Some("en".into()),
            timeout_seconds: 30,
        }
    }
}

/// Future HTTP transcription provider.
///
/// When implemented, this will:
/// 1. POST audio as WAV/raw PCM to `{base_url}/transcribe`
/// 2. Receive JSON array of `TranscriptionSegment`
/// 3. Return them through the standard `TranscriptionProvider` trait
pub struct HttpTranscriptionProvider {
    _config: HttpTranscriptionConfig,
}

impl HttpTranscriptionProvider {
    pub fn new(config: HttpTranscriptionConfig) -> Self {
        Self { _config: config }
    }
}

impl crate::TranscriptionProvider for HttpTranscriptionProvider {
    fn transcribe(
        &self,
        _audio_16khz_mono: &[f32],
    ) -> Result<Vec<TranscriptionSegment>, gravai_core::GravaiError> {
        Err(gravai_core::GravaiError::Provider(
            "HTTP transcription provider not yet implemented. \
             Configure 'whisper' engine in settings for on-device transcription."
                .into(),
        ))
    }

    fn name(&self) -> &str {
        "http"
    }
}
