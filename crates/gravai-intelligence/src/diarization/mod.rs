//! Speaker diarization and identification.
//!
//! Uses ONNX models (pyannote-based) for speaker segmentation.
//! Stores speaker embeddings for cross-session identification.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

/// A segment attributed to a specific speaker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker_id: String,
    pub confidence: f32,
}

/// Named speaker mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerProfile {
    pub id: String,
    pub name: String,
    pub embedding: Option<Vec<f32>>,
}

/// Provider trait for speaker diarization.
pub trait DiarizationProvider: Send + Sync {
    fn diarize(
        &self,
        audio_16khz_mono: &[f32],
    ) -> Result<Vec<SpeakerSegment>, gravai_core::GravaiError>;
    fn name(&self) -> &str;
}

/// Energy-based diarization (simple but functional).
/// Uses audio energy changes to detect speaker turns.
/// Will be replaced by ONNX pyannote in a future update.
pub struct EnergyDiarizer {
    max_speakers: u8,
}

impl EnergyDiarizer {
    pub fn new(config: &gravai_config::DiarizationConfig) -> Self {
        Self {
            max_speakers: config.max_speakers,
        }
    }
}

impl DiarizationProvider for EnergyDiarizer {
    fn diarize(
        &self,
        audio_16khz_mono: &[f32],
    ) -> Result<Vec<SpeakerSegment>, gravai_core::GravaiError> {
        let sample_rate = 16000;
        let window_ms = 500;
        let window_samples = sample_rate * window_ms / 1000;
        let mut segments = Vec::new();
        let mut current_speaker = 0u8;
        let mut segment_start = 0u64;

        // Simple energy-based segmentation: detect significant energy changes
        let mut prev_energy = 0.0f32;
        for (i, chunk) in audio_16khz_mono.chunks(window_samples).enumerate() {
            let energy: f32 = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
            let time_ms = (i * window_ms) as u64;

            // Detect speaker change on significant energy shift
            if i > 0 && prev_energy > 0.0001 {
                let ratio = energy / prev_energy.max(0.0001);
                if !(0.33..=3.0).contains(&ratio) {
                    // End current segment
                    if time_ms > segment_start {
                        segments.push(SpeakerSegment {
                            start_ms: segment_start,
                            end_ms: time_ms,
                            speaker_id: format!("Speaker {}", current_speaker + 1),
                            confidence: 0.6,
                        });
                    }
                    current_speaker = (current_speaker + 1) % self.max_speakers;
                    segment_start = time_ms;
                }
            }
            prev_energy = energy;
        }

        // Final segment
        let total_ms = (audio_16khz_mono.len() as u64 * 1000) / sample_rate as u64;
        if total_ms > segment_start {
            segments.push(SpeakerSegment {
                start_ms: segment_start,
                end_ms: total_ms,
                speaker_id: format!("Speaker {}", current_speaker + 1),
                confidence: 0.6,
            });
        }

        debug!(
            "Diarization: {} segments, {} speakers",
            segments.len(),
            segments
                .iter()
                .map(|s| &s.speaker_id)
                .collect::<std::collections::HashSet<_>>()
                .len()
        );

        Ok(segments)
    }

    fn name(&self) -> &str {
        "energy"
    }
}

/// In-memory speaker name mappings for a session.
pub struct SpeakerRegistry {
    names: HashMap<String, String>,
}

impl SpeakerRegistry {
    pub fn new() -> Self {
        Self {
            names: HashMap::new(),
        }
    }

    pub fn assign_name(&mut self, speaker_id: &str, name: &str) {
        info!("Speaker '{speaker_id}' assigned name '{name}'");
        self.names.insert(speaker_id.to_string(), name.to_string());
    }

    pub fn get_name(&self, speaker_id: &str) -> Option<&str> {
        self.names.get(speaker_id).map(|s| s.as_str())
    }

    pub fn resolve(&self, speaker_id: &str) -> String {
        self.names
            .get(speaker_id)
            .cloned()
            .unwrap_or_else(|| speaker_id.to_string())
    }
}

impl Default for SpeakerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a diarization provider based on config.
pub fn create_diarizer(config: &gravai_config::DiarizationConfig) -> Box<dyn DiarizationProvider> {
    Box::new(EnergyDiarizer::new(config))
}
