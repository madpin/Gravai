//! Speaker embeddings from a WeSpeaker ONNX model.
//!
//! The file is `~/.gravai/models/diarization/embedding.onnx`, the sherpa-onnx
//! export `wespeaker_en_voxceleb_resnet34_LM`. It expects 80-dim filterbank
//! features named `feats` with shape `[1, frames, 80]`. Waveforms are scaled
//! to the int16 range before the filterbank, which is what that export was
//! built with (`normalize_samples = 0`).

use super::fbank;
use gravai_core::GravaiError;
use ort::session::Session;
use ort::value::Tensor;
use std::sync::Mutex;

const MIN_SAMPLES: usize = 1_600;

/// Turns 16 kHz mono speech into one speaker vector.
pub trait SpeakerEmbedder: Send + Sync {
    fn embed(&self, audio_16khz_mono: &[f32]) -> Result<Vec<f32>, GravaiError>;
    fn name(&self) -> &str;
}

pub struct WeSpeakerEmbedder {
    session: Mutex<Session>,
    input_name: String,
}

impl WeSpeakerEmbedder {
    pub fn try_load() -> Option<Self> {
        Self::load_from(&super::embedding_model_path())
    }

    fn load_from(model_path: &std::path::Path) -> Option<Self> {
        if !model_path.exists() {
            tracing::debug!(
                "Speaker embedding model not found at {} — remote voices stay labeled Remote",
                model_path.display()
            );
            return None;
        }

        let session = match Session::builder()
            .and_then(|mut builder| builder.commit_from_file(model_path))
        {
            Ok(session) => session,
            Err(error) => {
                tracing::warn!("Failed to load speaker embedding model: {error}");
                return None;
            }
        };
        let input_name = session
            .inputs()
            .first()
            .map(|input| input.name().to_string())
            .unwrap_or_else(|| "feats".to_string());
        tracing::info!("WeSpeaker embedding model loaded ({input_name})");
        Some(Self {
            session: Mutex::new(session),
            input_name,
        })
    }
}

impl SpeakerEmbedder for WeSpeakerEmbedder {
    fn embed(&self, audio_16khz_mono: &[f32]) -> Result<Vec<f32>, GravaiError> {
        if audio_16khz_mono.len() < MIN_SAMPLES {
            return Err(GravaiError::Model(
                "audio is too short to embed a voice".into(),
            ));
        }
        let scaled: Vec<f32> = audio_16khz_mono
            .iter()
            .map(|sample| sample * 32768.0)
            .collect();
        let features = fbank::log_mel(&scaled);
        let frames = features.len() / 80;
        if frames == 0 {
            return Err(GravaiError::Model("filterbank produced no frames".into()));
        }

        let tensor = Tensor::from_array((vec![1i64, frames as i64, 80i64], features))
            .map_err(|error| GravaiError::Model(format!("speaker embedding input: {error}")))?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| GravaiError::Model("speaker embedding session lock poisoned".into()))?;
        let outputs = session
            .run(ort::inputs![self.input_name.as_str() => tensor])
            .map_err(|error| GravaiError::Model(format!("speaker embedding inference: {error}")))?;
        let output = outputs
            .iter()
            .next()
            .ok_or_else(|| GravaiError::Model("speaker embedding returned no output".into()))?;
        let (_, data) = output
            .1
            .try_extract_tensor::<f32>()
            .map_err(|error| GravaiError::Model(format!("speaker embedding output: {error}")))?;
        normalize(data).ok_or_else(|| GravaiError::Model("speaker embedding was empty".into()))
    }

    fn name(&self) -> &str {
        "wespeaker"
    }
}

fn normalize(values: &[f32]) -> Option<Vec<f32>> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm < 1e-6 {
        return None;
    }
    Some(values.iter().map(|value| value / norm).collect())
}
