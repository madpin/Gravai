//! Whisper transcription via whisper-rs (whisper.cpp bindings).
//!
//! Ported from ears-rust-api transcription/whisper.rs.

use std::sync::Mutex;

use gravai_config::TranscriptionConfig;
use tracing::{debug, info};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::TranscriptionSegment;

pub struct WhisperEngine {
    /// One state for the whole session. Creating a state allocates the KV cache;
    /// doing that on every utterance is a large part of why a call feels heavy.
    /// Mic and system audio share it, so only one transcription runs at a time
    /// and the machine is not decoding two streams at once.
    state: Mutex<whisper_rs::WhisperState>,
    language: String,
    hallucination_blocklist: Vec<String>,
    hallucination_repeat_blocklist: Vec<String>,
}

impl WhisperEngine {
    pub fn new(config: &TranscriptionConfig) -> Result<Self, gravai_core::GravaiError> {
        let model_path = gravai_config::models_dir().join(format!("ggml-{}.bin", config.model));

        if !model_path.exists() {
            return Err(gravai_core::GravaiError::Model(format!(
                "Whisper '{}' model not found. Go to Settings → Models to download it.",
                config.model
            )));
        }

        // Check for corrupted downloads (model files should be > 1MB)
        let file_size = std::fs::metadata(&model_path).map(|m| m.len()).unwrap_or(0);
        if file_size < 1_000_000 {
            return Err(gravai_core::GravaiError::Model(format!(
                "Whisper '{}' model appears corrupted ({} bytes). Delete it in Settings → Models and re-download.",
                config.model, file_size
            )));
        }

        let params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(model_path.to_str().unwrap_or(""), params)
            .map_err(|e| {
                gravai_core::GravaiError::Model(format!(
                    "Failed to load Whisper '{}' model ({}). The file may be corrupted — try deleting and re-downloading in Settings → Models.",
                    config.model, e
                ))
            })?;
        let state = ctx.create_state().map_err(|e| {
            gravai_core::GravaiError::Model(format!(
                "Failed to create Whisper state for '{}' ({e}).",
                config.model
            ))
        })?;

        info!(
            "Whisper engine loaded: model={}, language={}, threads={}",
            config.model,
            config.language,
            whisper_threads()
        );

        Ok(Self {
            state: Mutex::new(state),
            language: config.language.clone(),
            hallucination_blocklist: config
                .hallucination_blocklist
                .iter()
                .map(|s| s.to_lowercase())
                .collect(),
            hallucination_repeat_blocklist: config
                .hallucination_repeat_blocklist
                .iter()
                .map(|s| s.to_lowercase())
                .collect(),
        })
    }

    /// Check if text matches any hallucination blocklist entry.
    pub fn is_hallucination(&self, text: &str) -> bool {
        let trimmed = normalize_text(text);
        self.hallucination_blocklist.contains(&trimmed)
    }

    /// Check if text is in the repeat blocklist.
    pub fn is_repeat_hallucination(&self, text: &str) -> bool {
        let trimmed = normalize_text(text);
        self.hallucination_repeat_blocklist.contains(&trimmed)
    }

    /// Raw transcription (returns plain text, no segments). Used internally.
    pub fn transcribe_raw(&self, audio: &[f32]) -> Option<String> {
        let language = self.language.clone();
        let params = live_params(&language, false);
        let mut state = self.state.lock().ok()?;

        if let Err(e) = state.full(params, audio) {
            debug!("Whisper transcription failed: {e}");
            return None;
        }

        let num_segments = state.full_n_segments().ok().unwrap_or(0);
        if num_segments == 0 {
            return None;
        }

        let mut text = String::new();
        for i in 0..num_segments {
            if let Ok(segment) = state.full_get_segment_text(i) {
                text.push_str(&segment);
            }
        }

        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return None;
        }

        // Hallucination filter. Field access, not a &self method, so it can
        // run while the state mutex is held.
        if self
            .hallucination_blocklist
            .contains(&normalize_text(&trimmed))
        {
            debug!(
                "Hallucination filtered: {}",
                &trimmed[..trimmed.len().min(60)]
            );
            return None;
        }

        Some(trimmed)
    }
}

impl crate::TranscriptionProvider for WhisperEngine {
    fn transcribe(
        &self,
        audio_16khz_mono: &[f32],
    ) -> Result<Vec<TranscriptionSegment>, gravai_core::GravaiError> {
        let language = self.language.clone();
        let params = live_params(&language, true);
        let mut state = self
            .state
            .lock()
            .map_err(|e| gravai_core::GravaiError::Transcription(format!("Lock: {e}")))?;

        state
            .full(params, audio_16khz_mono)
            .map_err(|e| gravai_core::GravaiError::Transcription(format!("Transcribe: {e}")))?;

        let num_segments = state.full_n_segments().unwrap_or(0);
        let mut segments = Vec::new();

        for i in 0..num_segments {
            let text = match state.full_get_segment_text(i) {
                Ok(t) => t.trim().to_string(),
                Err(_) => continue,
            };

            if text.is_empty()
                || self
                    .hallucination_blocklist
                    .contains(&normalize_text(&text))
            {
                continue;
            }

            let start_ms = state
                .full_get_segment_t0(i)
                .map(|t| (t * 10) as u64)
                .unwrap_or(0);
            let end_ms = state
                .full_get_segment_t1(i)
                .map(|t| (t * 10) as u64)
                .unwrap_or(0);

            segments.push(TranscriptionSegment {
                start_ms,
                end_ms,
                text,
                confidence: 0.0, // whisper.cpp doesn't expose per-segment confidence easily
                language: Some(self.language.clone()),
            });
        }

        Ok(segments)
    }

    fn name(&self) -> &str {
        "whisper"
    }
}

/// Threads for one live pass. Mic and system already share one state, so this
/// is the only Whisper work running. Leave the other cores for the call.
fn whisper_threads() -> std::ffi::c_int {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as std::ffi::c_int)
        .unwrap_or(4);
    (cores / 2).clamp(2, 4)
}

fn live_params<'a>(language: &'a str, timestamps: bool) -> FullParams<'a, 'static> {
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    if language.is_empty() || language.eq_ignore_ascii_case("auto") {
        params.set_language(None);
    } else {
        params.set_language(Some(language));
    }
    params.set_n_threads(whisper_threads());
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(timestamps);
    params.set_suppress_blank(true);
    // VAD already cut this into one utterance. Skip the segment search.
    params.set_single_segment(true);
    params.set_no_context(true);
    // The default temperature ladder re-decodes noisy meeting audio.
    params.set_temperature(0.0);
    params.set_temperature_inc(0.0);
    params
}

/// Normalize text for comparison: lowercase + strip punctuation edges.
fn normalize_text(text: &str) -> String {
    text.to_lowercase()
        .trim()
        .trim_matches(|c: char| c.is_ascii_punctuation())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hallucination_detection() {
        let config = TranscriptionConfig::default();
        // Can't create engine without model, but test the normalize + blocklist logic
        let blocklist: Vec<String> = config
            .hallucination_blocklist
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        let normalized = normalize_text("  Thanks for watching!  ");
        assert!(blocklist.iter().any(|h| normalized == *h));

        let normalized = normalize_text("Hello world");
        assert!(!blocklist.iter().any(|h| normalized == *h));
    }
}
