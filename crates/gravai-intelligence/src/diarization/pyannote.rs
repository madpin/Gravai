//! Pyannote segmentation 3.0.
//!
//! The model outputs powerset classes (combinations of up to three local
//! speakers), not stable speaker identities. This module decodes those classes,
//! links local speakers across overlapping windows, and returns turns. Who those
//! turns belong to is decided later by a speaker embedding.
//!
//! Requires:
//!   ~/.gravai/models/diarization/segmentation.onnx  (~90 MB)

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
        let mut windows = Vec::new();
        let mut window_start = 0;
        while window_start < total_samples {
            let window_end = (window_start + WINDOW_SAMPLES).min(total_samples);
            let window = &audio_16khz_mono[window_start..window_end];
            if let Some(frames) = self.run_segmentation(window) {
                if !frames.is_empty() {
                    let speakers: Vec<Vec<f32>> = frames
                        .iter()
                        .map(|frame| marginal_speaker_probs(frame))
                        .collect();
                    windows.push(WindowSpeakers {
                        start_sample: window_start,
                        samples_per_frame: window.len() as f32 / speakers.len() as f32,
                        probs: speakers,
                    });
                }
            }
            if window_end == total_samples {
                break;
            }
            window_start += STEP_SAMPLES;
        }

        let segments = segments_from_windows(&windows, total_samples, SAMPLE_RATE as u64);
        tracing::debug!(
            max_speakers = self.max_speakers,
            "Pyannote segmentation: {} turns for {:.1}s audio",
            segments.len(),
            total_samples as f32 / SAMPLE_RATE as f32
        );
        Ok(segments)
    }

    fn name(&self) -> &str {
        "pyannote"
    }
}

/// One segmentation window after powerset classes have been turned into
/// per-speaker probabilities.
struct WindowSpeakers {
    start_sample: usize,
    samples_per_frame: f32,
    probs: Vec<Vec<f32>>,
}

const BIN_SAMPLES: usize = 320;
const SPEAKER_ON: f32 = 0.5;
const MERGE_GAP_MS: u64 = 200;

/// Combinations of `num_speakers` of size 1, then 2, then 3. Segmentation 3.0
/// uses 3 speakers, which is 7 classes.
fn powerset_mapping(num_speakers: usize) -> Vec<Vec<usize>> {
    let mut mapping = Vec::new();
    for size in 1..=num_speakers {
        let mut choice = vec![0usize; size];
        powerset_combinations(num_speakers, size, 0, &mut choice, 0, &mut mapping);
    }
    mapping
}

fn powerset_combinations(
    num_speakers: usize,
    size: usize,
    start: usize,
    choice: &mut [usize],
    filled: usize,
    out: &mut Vec<Vec<usize>>,
) {
    if filled == size {
        out.push(choice.to_vec());
        return;
    }
    for speaker in start..num_speakers {
        choice[filled] = speaker;
        powerset_combinations(num_speakers, size, speaker + 1, choice, filled + 1, out);
    }
}

/// Turn one frame of class scores into a probability per local speaker.
pub(crate) fn marginal_speaker_probs(class_values: &[f32]) -> Vec<f32> {
    if class_values.is_empty() {
        return Vec::new();
    }
    let distribution = as_distribution(class_values);
    if class_values.len() == 7 {
        let mapping = powerset_mapping(3);
        let mut speakers = vec![0.0f32; 3];
        for (class, probability) in distribution.iter().enumerate() {
            if let Some(set) = mapping.get(class) {
                for &speaker in set {
                    speakers[speaker] += *probability;
                }
            }
        }
        return speakers;
    }
    distribution
}

fn as_distribution(values: &[f32]) -> Vec<f32> {
    let sum: f32 = values.iter().sum();
    let non_negative = values.iter().all(|value| *value >= -0.01);
    if non_negative && sum > 0.0 && (sum - 1.0).abs() < 0.15 {
        return values.iter().map(|value| value.max(0.0) / sum).collect();
    }
    let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exp: Vec<f32> = values.iter().map(|value| (value - max).exp()).collect();
    let total: f32 = exp.iter().sum();
    if total == 0.0 || !total.is_finite() {
        return vec![0.0; values.len()];
    }
    exp.iter().map(|value| value / total).collect()
}

fn segments_from_windows(
    windows: &[WindowSpeakers],
    total_samples: usize,
    sample_rate: u64,
) -> Vec<SpeakerSegment> {
    if total_samples == 0 || windows.is_empty() || sample_rate == 0 {
        return Vec::new();
    }

    struct Node {
        window: usize,
        local: usize,
        global_id: usize,
    }

    let mut nodes: Vec<Node> = Vec::new();
    let mut next_global = 0usize;
    for (window_index, window) in windows.iter().enumerate() {
        let local_count = window.probs.first().map(Vec::len).unwrap_or(0);
        let mut active = vec![false; local_count];
        for frame in &window.probs {
            for (speaker, probability) in frame.iter().enumerate() {
                if *probability >= SPEAKER_ON {
                    active[speaker] = true;
                }
            }
        }
        let active_locals: Vec<usize> = active
            .iter()
            .enumerate()
            .filter_map(|(speaker, on)| on.then_some(speaker))
            .collect();
        if active_locals.is_empty() {
            continue;
        }

        let mut matched = vec![None; active_locals.len()];
        if let Some(previous) = nodes.iter().map(|node| node.window).max() {
            let previous_locals: Vec<usize> = nodes
                .iter()
                .filter(|node| node.window == previous)
                .map(|node| node.local)
                .collect();
            let mut pairs = Vec::new();
            for (active_index, &local) in active_locals.iter().enumerate() {
                for &previous_local in &previous_locals {
                    let score =
                        overlap_score(windows, previous, previous_local, window_index, local);
                    if score > 0.0 {
                        pairs.push((score, active_index, previous_local));
                    }
                }
            }
            pairs.sort_by(|left, right| {
                right
                    .0
                    .partial_cmp(&left.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut used_previous = std::collections::HashSet::new();
            let mut used_current = std::collections::HashSet::new();
            for (_, active_index, previous_local) in pairs {
                if used_current.contains(&active_index) || used_previous.contains(&previous_local) {
                    continue;
                }
                used_current.insert(active_index);
                used_previous.insert(previous_local);
                if let Some(global_id) = nodes
                    .iter()
                    .find(|node| node.window == previous && node.local == previous_local)
                    .map(|node| node.global_id)
                {
                    matched[active_index] = Some(global_id);
                }
            }
        }

        for (active_index, &local) in active_locals.iter().enumerate() {
            let global_id = matched[active_index].unwrap_or_else(|| {
                let id = next_global;
                next_global += 1;
                id
            });
            nodes.push(Node {
                window: window_index,
                local,
                global_id,
            });
        }
    }

    if next_global == 0 {
        return Vec::new();
    }

    let bin_count = total_samples.div_ceil(BIN_SAMPLES);
    let mut sums = vec![vec![0.0f32; next_global]; bin_count];
    for node in &nodes {
        let window = &windows[node.window];
        for (frame_index, frame) in window.probs.iter().enumerate() {
            let probability = frame.get(node.local).copied().unwrap_or(0.0);
            if probability <= 0.0 {
                continue;
            }
            let start =
                window.start_sample + (frame_index as f32 * window.samples_per_frame) as usize;
            let end = window.start_sample
                + ((frame_index as f32 + 1.0) * window.samples_per_frame) as usize;
            let bin_start = start / BIN_SAMPLES;
            let bin_end = end.max(start + 1).div_ceil(BIN_SAMPLES).min(bin_count);
            for scores in sums.iter_mut().take(bin_end).skip(bin_start) {
                scores[node.global_id] += probability;
            }
        }
    }

    let mut labels = vec![None; bin_count];
    for (bin, scores) in sums.iter().enumerate() {
        if let Some((speaker, score)) = scores.iter().enumerate().max_by(|left, right| {
            left.1
                .partial_cmp(right.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            if *score >= SPEAKER_ON {
                labels[bin] = Some(speaker);
            }
        }
    }

    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    let mut current: Option<(usize, usize)> = None;
    for (bin, label) in labels.iter().enumerate() {
        match (current, label) {
            (Some((start, speaker)), Some(id)) if *id == speaker => {
                current = Some((start, speaker))
            }
            (Some((start, speaker)), other) => {
                runs.push((start, bin, speaker));
                current = other.map(|id| (bin, id));
            }
            (None, Some(id)) => current = Some((bin, *id)),
            (None, None) => {}
        }
    }
    if let Some((start, speaker)) = current {
        runs.push((start, bin_count, speaker));
    }

    let to_ms = |bin: usize| (bin as u64 * BIN_SAMPLES as u64 * 1000) / sample_rate;
    let mut segments: Vec<SpeakerSegment> = runs
        .into_iter()
        .filter_map(|(start, end, speaker)| {
            let start_ms = to_ms(start);
            let end_ms = to_ms(end);
            (end_ms.saturating_sub(start_ms) >= MIN_SEGMENT_MS).then(|| SpeakerSegment {
                start_ms,
                end_ms,
                speaker_id: format!("Speaker {}", speaker + 1),
                confidence: 0.75,
            })
        })
        .collect();

    if segments.len() >= 2 {
        let mut merged = vec![segments.remove(0)];
        for segment in segments {
            let last = merged.last_mut().unwrap();
            if last.speaker_id == segment.speaker_id
                && segment.start_ms.saturating_sub(last.end_ms) <= MERGE_GAP_MS
            {
                last.end_ms = segment.end_ms;
            } else {
                merged.push(segment);
            }
        }
        segments = merged;
    }
    segments
}

fn overlap_score(
    windows: &[WindowSpeakers],
    left_window: usize,
    left_local: usize,
    right_window: usize,
    right_local: usize,
) -> f32 {
    let left = &windows[left_window];
    let right = &windows[right_window];
    let left_end = left.start_sample + (left.probs.len() as f32 * left.samples_per_frame) as usize;
    let right_end =
        right.start_sample + (right.probs.len() as f32 * right.samples_per_frame) as usize;
    let start = left.start_sample.max(right.start_sample);
    let end = left_end.min(right_end);
    if end <= start {
        return 0.0;
    }
    let mut score = 0.0;
    let mut sample = start;
    while sample < end {
        score +=
            probability_at(left, left_local, sample) * probability_at(right, right_local, sample);
        sample += BIN_SAMPLES;
    }
    score
}

fn probability_at(window: &WindowSpeakers, local: usize, sample: usize) -> f32 {
    if sample < window.start_sample || window.samples_per_frame <= 0.0 {
        return 0.0;
    }
    let frame = ((sample - window.start_sample) as f32 / window.samples_per_frame) as usize;
    window
        .probs
        .get(frame)
        .and_then(|frame| frame.get(local))
        .copied()
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powerset_class_zero_is_the_first_speaker() {
        let mut logits = vec![0.0f32; 7];
        logits[0] = 10.0;
        let speakers = marginal_speaker_probs(&logits);
        assert!(speakers[0] > 0.9, "{speakers:?}");
        assert!(speakers[1] < 0.05 && speakers[2] < 0.05, "{speakers:?}");
    }

    #[test]
    fn powerset_overlap_class_activates_both_speakers() {
        let mut logits = vec![0.0f32; 7];
        logits[3] = 10.0;
        let speakers = marginal_speaker_probs(&logits);
        assert!(speakers[0] > 0.9 && speakers[1] > 0.9, "{speakers:?}");
        assert!(speakers[2] < 0.05, "{speakers:?}");
    }

    #[test]
    fn overlapping_windows_keep_one_speaker_when_the_channel_moves() {
        let windows = vec![
            WindowSpeakers {
                start_sample: 0,
                samples_per_frame: 1600.0,
                probs: vec![vec![0.9, 0.05, 0.05]; 10],
            },
            WindowSpeakers {
                start_sample: 8_000,
                samples_per_frame: 1600.0,
                probs: vec![vec![0.05, 0.9, 0.05]; 10],
            },
        ];
        let segments = segments_from_windows(&windows, 24_000, 16_000);
        assert_eq!(segments.len(), 1, "{segments:?}");
        assert_eq!(segments[0].speaker_id, "Speaker 1");
        assert_eq!(segments[0].start_ms, 0);
        assert!(segments[0].end_ms >= 1_400, "{}", segments[0].end_ms);
    }

    #[test]
    fn one_window_splits_two_speakers() {
        let mut probs = vec![vec![0.9, 0.05, 0.02]; 10];
        probs.extend(std::iter::repeat_n(vec![0.05, 0.9, 0.02], 10));
        let windows = vec![WindowSpeakers {
            start_sample: 0,
            samples_per_frame: 800.0,
            probs,
        }];
        let segments = segments_from_windows(&windows, 16_000, 16_000);
        assert_eq!(segments.len(), 2, "{segments:?}");
        assert_eq!(segments[0].speaker_id, "Speaker 1");
        assert_eq!(segments[1].speaker_id, "Speaker 2");
        assert!(segments[0].end_ms <= segments[1].start_ms);
    }
}
