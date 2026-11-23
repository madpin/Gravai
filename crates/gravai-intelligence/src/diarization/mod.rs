//! Speaker diarization and identification.
//!
//! Segmentation finds turns inside one buffer. A speaker embedding plus
//! [`SpeakerBank`] keeps those turns as the same Remote for the whole recording.

pub mod bank;
pub mod embedder;
mod fbank;
pub mod pyannote;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

pub use bank::{cluster_speaker_embeddings, EmbeddingSample, SpeakerAssignment, SpeakerBank};
pub use embedder::{SpeakerEmbedder, WeSpeakerEmbedder};

/// `~/.gravai/models/diarization/embedding.onnx`
pub fn embedding_model_path() -> PathBuf {
    gravai_config::models_dir()
        .join("diarization")
        .join("embedding.onnx")
}

/// `~/.gravai/models/diarization/segmentation.onnx`
pub fn segmentation_model_path() -> PathBuf {
    gravai_config::models_dir()
        .join("diarization")
        .join("segmentation.onnx")
}

/// Load the voice model used to tell remotes apart. `None` when the file is missing.
pub fn try_load_embedder() -> Option<std::sync::Arc<dyn SpeakerEmbedder>> {
    WeSpeakerEmbedder::try_load()
        .map(|embedder| std::sync::Arc::new(embedder) as std::sync::Arc<dyn SpeakerEmbedder>)
}

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

/// Labels a whole system-audio chunk as one remote.
///
/// Loudness is not identity, so this does not invent Speaker 1, Speaker 2, …
/// Separate remotes come from the embedding model.
pub struct EnergyDiarizer {
    #[allow(dead_code)]
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
        if audio_16khz_mono.is_empty() {
            return Ok(Vec::new());
        }
        let total_ms = (audio_16khz_mono.len() as u64 * 1000) / 16_000;
        debug!("Energy diarization: one remote over {total_ms} ms");
        Ok(vec![SpeakerSegment {
            start_ms: 0,
            end_ms: total_ms.max(1),
            speaker_id: "Remote".to_string(),
            confidence: 0.3,
        }])
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
    if config.model == "pyannote" {
        if let Some(d) = pyannote::PyannoteOnnxDiarizer::try_load(config) {
            info!("Using pyannote ONNX diarizer");
            return Box::new(d);
        }
        tracing::warn!("Pyannote model not found, falling back to energy diarizer");
    }
    Box::new(EnergyDiarizer::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_diarizer_returns_a_single_remote() {
        let config = gravai_config::DiarizationConfig {
            enabled: true,
            max_speakers: 10,
            model: "energy".into(),
        };
        let diarizer = EnergyDiarizer::new(&config);
        let mut audio = vec![0.01f32; 16_000];
        audio.extend(std::iter::repeat_n(0.2, 16_000));
        let segments = diarizer.diarize(&audio).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].speaker_id, "Remote");
        assert_eq!(segments[0].start_ms, 0);
        assert_eq!(segments[0].end_ms, 2_000);
    }
}
