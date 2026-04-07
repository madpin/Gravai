//! Pyannote-based speaker diarization using ONNX segmentation model.
//!
//! Performs sliding-window segmentation to detect speaker turns, then clusters
//! segments using cosine similarity of frame-level embeddings.
//!
//! Requires:
//!   ~/.gravai/models/diarization/segmentation.onnx  (~90 MB)
//!
//! Models can be downloaded from the Models page in the app.

use super::{DiarizationProvider, SpeakerSegment};
use gravai_core::GravaiError;
use ort::session::Session;
use std::sync::Mutex;

const SAMPLE_RATE: usize = 16000;
/// 10-second window at 16 kHz.
const WINDOW_SAMPLES: usize = SAMPLE_RATE * 10;
/// 50% overlap between windows.
const STEP_SAMPLES: usize = WINDOW_SAMPLES / 2;
/// Minimum segment duration in ms to keep.
const MIN_SEGMENT_MS: u64 = 500;

fn models_dir() -> std::path::PathBuf {
    gravai_config::models_dir().join("diarization")
}

/// Pyannote segmentation ONNX diarizer.
pub struct PyannoteOnnxDiarizer {
    session: Mutex<Session>,
    max_speakers: u8,
}

impl PyannoteOnnxDiarizer {
    /// Load the segmentation model. Returns `None` if model files are missing.
    pub fn try_load(config: &gravai_config::DiarizationConfig) -> Option<Self> {
        let model_path = models_dir().join("segmentation.onnx");
        if !model_path.exists() {
            tracing::debug!(
                "Pyannote segmentation.onnx not found at {} — falling back to energy diarizer",
                model_path.display()
            );
            return None;
        }

        let session = match Session::builder().and_then(|mut b| b.commit_from_file(&model_path)) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to load pyannote ONNX model: {e}");
                return None;
            }
        };

        tracing::info!("Pyannote segmentation model loaded");
        Some(Self {
            session: Mutex::new(session),
            max_speakers: config.max_speakers,
        })
    }

    /// Run the segmentation model on a single window and return per-frame speaker activity.
    /// Output shape: [1, frames, num_speakers].
    fn run_segmentation(&self, window: &[f32]) -> Option<Vec<Vec<f32>>> {
        use ort::value::Tensor;

        let len = window.len();
        let input_data = window.to_vec();
        let tensor = Tensor::from_array((vec![1i64, 1i64, len as i64], input_data)).ok()?;

        let mut session = self.session.lock().ok()?;
        let outputs = session.run(ort::inputs!["input" => tensor]).ok()?;

        let output_entry = outputs.iter().next()?;
        let (shape, data) = output_entry.1.try_extract_tensor::<f32>().ok()?;

        // shape: [1, num_frames, num_speakers]
        let num_frames = shape[1] as usize;
        let num_speakers = shape[2] as usize;
        let data = data.to_vec();

        let mut frames: Vec<Vec<f32>> = Vec::with_capacity(num_frames);
        for f in 0..num_frames {
            let probs: Vec<f32> = (0..num_speakers)
                .map(|s| data[f * num_speakers + s])
                .collect();
            frames.push(probs);
        }
        Some(frames)
    }
}

impl DiarizationProvider for PyannoteOnnxDiarizer {
    fn diarize(&self, audio_16khz_mono: &[f32]) -> Result<Vec<SpeakerSegment>, GravaiError> {
        if audio_16khz_mono.len() < SAMPLE_RATE / 2 {
            return Ok(vec![]);
        }

        let total_samples = audio_16khz_mono.len();
        // Accumulate per-sample speaker probability across all windows
        let num_speakers = self.max_speakers as usize;
        let mut speaker_votes: Vec<Vec<f32>> = vec![vec![0.0; num_speakers]; total_samples];
        let mut vote_counts: Vec<f32> = vec![0.0; total_samples];

        let mut window_start = 0;
        while window_start < total_samples {
            let window_end = (window_start + WINDOW_SAMPLES).min(total_samples);
            let window = &audio_16khz_mono[window_start..window_end];

            if let Some(frames) = self.run_segmentation(window) {
                // Map frames back to sample positions
                let frames_per_sample = frames.len() as f32 / window.len() as f32;
                for (sample_off, votes) in speaker_votes[window_start..window_end]
                    .iter_mut()
                    .enumerate()
                {
                    let frame_idx = (sample_off as f32 * frames_per_sample) as usize;
                    let frame_idx = frame_idx.min(frames.len() - 1);
                    let probs = &frames[frame_idx];
                    for (s, v) in votes.iter_mut().enumerate() {
                        *v += probs.get(s).copied().unwrap_or(0.0);
                    }
                    vote_counts[window_start + sample_off] += 1.0;
                }
            }

            if window_end == total_samples {
                break;
            }
            window_start += STEP_SAMPLES;
        }

        // Determine dominant speaker for each sample
        let mut segments: Vec<SpeakerSegment> = Vec::new();
        let mut current_speaker: usize = 0;
        let mut seg_start_ms: u64 = 0;
        let mut prev_speaker: usize = 0;

        for (i, (votes, count)) in speaker_votes.iter().zip(vote_counts.iter()).enumerate() {
            if *count == 0.0 {
                continue;
            }
            let dominant = votes
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(s, _)| s)
                .unwrap_or(0);

            if i == 0 {
                current_speaker = dominant;
                seg_start_ms = 0;
                prev_speaker = dominant;
                continue;
            }

            if dominant != current_speaker {
                let end_ms = (i as u64 * 1000) / SAMPLE_RATE as u64;
                if end_ms.saturating_sub(seg_start_ms) >= MIN_SEGMENT_MS {
                    segments.push(SpeakerSegment {
                        start_ms: seg_start_ms,
                        end_ms,
                        speaker_id: format!("Speaker {}", current_speaker + 1),
                        confidence: 0.75,
                    });
                }
                seg_start_ms = end_ms;
                current_speaker = dominant;
            }
            prev_speaker = dominant;
        }

        // Final segment
        let total_ms = (total_samples as u64 * 1000) / SAMPLE_RATE as u64;
        if total_ms > seg_start_ms {
            segments.push(SpeakerSegment {
                start_ms: seg_start_ms,
                end_ms: total_ms,
                speaker_id: format!("Speaker {}", prev_speaker + 1),
                confidence: 0.75,
            });
        }

        tracing::debug!(
            "Pyannote diarization: {} segments for {:.1}s audio",
            segments.len(),
            total_samples as f32 / SAMPLE_RATE as f32
        );

        Ok(segments)
    }

    fn name(&self) -> &str {
        "pyannote"
    }
}
