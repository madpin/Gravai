//! VAD-triggered transcription pipeline.
//!
//! Ported from ears-rust-api session.rs process_source + finalize_utterance.
//! Receives 16kHz mono audio, runs VAD, accumulates speech, transcribes on pause.

use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::echo::EchoSuppressor;
use crate::vad::VadProvider;

/// Result of processing a finalized utterance.
#[derive(Debug, Clone)]
pub struct Utterance {
    pub text: String,
    pub source: String,
    pub speaker: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Milliseconds from the start of the recording session when speech began.
    pub start_ms: i64,
    /// Milliseconds from the start of the recording session when speech ended.
    pub end_ms: i64,
    /// Voice vector for this line, stored so the recording can be relabeled on stop.
    pub embedding: Option<Vec<f32>>,
}

/// Callback invoked when a new utterance is finalized.
pub type OnUtterance = Arc<dyn Fn(Utterance) + Send + Sync>;

/// Configuration for the pipeline.
pub struct PipelineConfig {
    pub pause_seconds: f32,
    pub min_utterance_seconds: f32,
    pub max_utterance_seconds: f32,
    pub sample_rate: u32, // 16000 for transcription path
    /// How many other people this recording can name.
    pub max_speakers: u8,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            pause_seconds: 0.5,
            min_utterance_seconds: 0.3,
            max_utterance_seconds: 30.0,
            sample_rate: 16000,
            max_speakers: 10,
        }
    }
}

impl PipelineConfig {
    pub fn from_app_config(config: &gravai_config::AppConfig) -> Self {
        Self {
            pause_seconds: config.vad.pause_seconds,
            min_utterance_seconds: config.vad.silero.min_utterance_seconds,
            max_utterance_seconds: config.vad.silero.max_utterance_seconds,
            sample_rate: 16000,
            max_speakers: config.features.diarization.max_speakers,
        }
    }
}

/// All inputs needed to run a transcription pipeline for one audio source.
pub struct PipelineInput {
    pub rx: mpsc::Receiver<Vec<f32>>,
    pub source: String,
    pub vad: Box<dyn VadProvider>,
    pub transcriber: Option<Arc<dyn gravai_transcription::TranscriptionProvider>>,
    pub echo_suppressor: Arc<tokio::sync::Mutex<EchoSuppressor>>,
    /// When false, skip the cross-source similarity check entirely.
    pub echo_enabled: bool,
    pub diarizer: Option<Arc<dyn gravai_intelligence::DiarizationProvider>>,
    /// Voice model that keeps Remote 1 and Remote 2 stable across sentences.
    pub embedder: Option<Arc<dyn gravai_intelligence::SpeakerEmbedder>>,
    pub config: PipelineConfig,
    pub on_utterance: OnUtterance,
    pub active: Arc<std::sync::atomic::AtomicBool>,
}

/// Run the VAD-triggered transcription pipeline for one audio source.
///
/// This is the core audio processing loop ported from ears' `process_source`.
/// It receives 16kHz mono audio chunks, runs VAD, accumulates speech segments,
/// and calls the transcription provider when silence is detected.
pub async fn run_pipeline(input: PipelineInput) {
    let PipelineInput {
        mut rx,
        source,
        mut vad,
        transcriber,
        echo_suppressor,
        echo_enabled,
        diarizer,
        embedder,
        config: pipeline_config,
        on_utterance,
        active,
    } = input;

    let sr = pipeline_config.sample_rate;
    let pause_samples = (sr as f32 * pipeline_config.pause_seconds) as usize;
    let min_samples = (sr as f32 * pipeline_config.min_utterance_seconds) as usize;
    let max_samples = (sr as f32 * pipeline_config.max_utterance_seconds) as usize;

    let mut speech_buffer: Vec<f32> = Vec::new();
    let mut silence_count: usize = 0;
    let mut last_text: Option<String> = None;
    let mut speaker_bank = embedder
        .as_ref()
        .map(|_| gravai_intelligence::SpeakerBank::new(pipeline_config.max_speakers as usize));

    let mut total_samples: usize = 0;
    let mut speech_start_sample: usize = 0;
    let mut speech_end_sample: usize = 0;

    info!("Transcription pipeline started for source: {source}");

    while active.load(std::sync::atomic::Ordering::Relaxed) {
        let chunk = tokio::select! {
            c = rx.recv() => {
                match c {
                    Some(c) => c,
                    None => break,
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                continue;
            }
        };

        let is_speech = vad.is_speech(&chunk);

        if is_speech {
            if speech_buffer.is_empty() {
                speech_start_sample = total_samples;
            }
            speech_end_sample = total_samples + chunk.len();

            speech_buffer.extend_from_slice(&chunk);
            silence_count = 0;

            // Force-split long utterances
            if speech_buffer.len() >= max_samples {
                let results = finalize(
                    &speech_buffer,
                    &source,
                    &transcriber,
                    &echo_suppressor,
                    echo_enabled,
                    &last_text,
                    &diarizer,
                    &embedder,
                    speaker_bank.as_mut(),
                )
                .await;
                emit_lines(
                    results,
                    &mut last_text,
                    speech_start_sample,
                    speech_end_sample,
                    sr,
                    &source,
                    &on_utterance,
                );
                speech_buffer.clear();
            }
        } else {
            silence_count += chunk.len();
            if !speech_buffer.is_empty() && silence_count >= pause_samples {
                if speech_buffer.len() >= min_samples {
                    let results = finalize(
                        &speech_buffer,
                        &source,
                        &transcriber,
                        &echo_suppressor,
                        echo_enabled,
                        &last_text,
                        &diarizer,
                        &embedder,
                        speaker_bank.as_mut(),
                    )
                    .await;
                    emit_lines(
                        results,
                        &mut last_text,
                        speech_start_sample,
                        speech_end_sample,
                        sr,
                        &source,
                        &on_utterance,
                    );
                }
                speech_buffer.clear();
                silence_count = 0;
            }
        }

        total_samples += chunk.len();
    }

    // Flush remaining speech
    if !speech_buffer.is_empty() && speech_buffer.len() >= min_samples {
        let results = finalize(
            &speech_buffer,
            &source,
            &transcriber,
            &echo_suppressor,
            echo_enabled,
            &last_text,
            &diarizer,
            &embedder,
            speaker_bank.as_mut(),
        )
        .await;
        emit_lines(
            results,
            &mut last_text,
            speech_start_sample,
            speech_end_sample,
            sr,
            &source,
            &on_utterance,
        );
    }

    debug!("Transcription pipeline for {source} ended");
}

/// Result of finalize: one speaker turn of transcribed text.
struct FinalizeResult {
    text: String,
    speaker: Option<String>,
    embedding: Option<Vec<f32>>,
    start_offset_ms: i64,
    end_offset_ms: i64,
}

struct SpeechTurn {
    start_ms: u64,
    end_ms: u64,
    speaker: String,
    embedding: Option<Vec<f32>>,
}

fn emit_lines(
    results: Vec<FinalizeResult>,
    last_text: &mut Option<String>,
    speech_start_sample: usize,
    speech_end_sample: usize,
    sample_rate: u32,
    source: &str,
    on_utterance: &OnUtterance,
) {
    if results.is_empty() {
        return;
    }
    *last_text = Some(
        results
            .iter()
            .map(|result| result.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    );
    for result in results {
        let (start_ms, end_ms) =
            utterance_window_ms(speech_start_sample, speech_end_sample, sample_rate, &result);
        on_utterance(Utterance {
            text: result.text,
            source: source.to_string(),
            speaker: result.speaker,
            timestamp: chrono::Utc::now(),
            start_ms,
            end_ms,
            embedding: result.embedding,
        });
    }
}

fn utterance_window_ms(
    speech_start_sample: usize,
    speech_end_sample: usize,
    sample_rate: u32,
    result: &FinalizeResult,
) -> (i64, i64) {
    let utterance_start_ms = (speech_start_sample as i64 * 1000) / sample_rate as i64;
    let utterance_end_ms = (speech_end_sample as i64 * 1000) / sample_rate as i64;
    let utterance_duration_ms = (utterance_end_ms - utterance_start_ms).max(0);

    let start_offset_ms = result.start_offset_ms.clamp(0, utterance_duration_ms);
    let end_offset_ms = result
        .end_offset_ms
        .clamp(start_offset_ms, utterance_duration_ms);

    (
        utterance_start_ms + start_offset_ms,
        utterance_start_ms + end_offset_ms,
    )
}

/// Transcribe accumulated audio, label who is speaking, and apply filters.
#[allow(clippy::too_many_arguments)]
async fn finalize(
    audio: &[f32],
    source: &str,
    transcriber: &Option<Arc<dyn gravai_transcription::TranscriptionProvider>>,
    echo_suppressor: &Arc<tokio::sync::Mutex<EchoSuppressor>>,
    echo_enabled: bool,
    last_text: &Option<String>,
    diarizer: &Option<Arc<dyn gravai_intelligence::DiarizationProvider>>,
    embedder: &Option<Arc<dyn gravai_intelligence::SpeakerEmbedder>>,
    speaker_bank: Option<&mut gravai_intelligence::SpeakerBank>,
) -> Vec<FinalizeResult> {
    let Some(engine) = transcriber.as_ref() else {
        return Vec::new();
    };
    let engine = Arc::clone(engine);

    let audio_owned = audio.to_vec();
    let Ok(Ok(segments)) =
        tokio::task::spawn_blocking(move || engine.transcribe(&audio_owned)).await
    else {
        return Vec::new();
    };
    if segments.is_empty() {
        return Vec::new();
    }

    let text = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let text = text.trim().to_string();
    if text.is_empty() {
        return Vec::new();
    }

    if let Some(previous) = last_text {
        if text.trim().to_lowercase() == previous.trim().to_lowercase() {
            debug!(
                "Repeat suppressed ({source}): {}",
                &text[..text.len().min(60)]
            );
            return Vec::new();
        }
    }

    if echo_enabled {
        let mut suppressor = echo_suppressor.lock().await;
        if suppressor.is_echo(&text, source) {
            debug!(
                "Echo suppressed ({source}): {}",
                &text[..text.len().min(60)]
            );
            return Vec::new();
        }
        suppressor.add(&text, source);
    }

    let audio_ms = ((audio.len() as u64) * 1000) / 16_000;
    let turns = if source == "microphone" || source == "mic" {
        vec![SpeechTurn {
            start_ms: 0,
            end_ms: audio_ms.max(1),
            speaker: "You".to_string(),
            embedding: None,
        }]
    } else {
        label_remote_turns(audio, diarizer, embedder, speaker_bank).await
    };
    split_by_turns(&segments, &turns)
}

async fn label_remote_turns(
    audio: &[f32],
    diarizer: &Option<Arc<dyn gravai_intelligence::DiarizationProvider>>,
    embedder: &Option<Arc<dyn gravai_intelligence::SpeakerEmbedder>>,
    mut speaker_bank: Option<&mut gravai_intelligence::SpeakerBank>,
) -> Vec<SpeechTurn> {
    let audio_ms = ((audio.len() as u64) * 1000) / 16_000;
    let whole = || SpeechTurn {
        start_ms: 0,
        end_ms: audio_ms.max(1),
        speaker: "Remote".to_string(),
        embedding: None,
    };
    let Some(embedder) = embedder else {
        return vec![whole()];
    };

    let audio_owned = audio.to_vec();
    let diarizer = diarizer.clone();
    let embedder = Arc::clone(embedder);
    let prepared = tokio::task::spawn_blocking(move || {
        prepare_speaker_groups(&audio_owned, diarizer.as_deref(), embedder.as_ref())
    })
    .await
    .unwrap_or_default();
    if prepared.is_empty() {
        return vec![whole()];
    }

    let mut turns = Vec::new();
    for group in prepared {
        let speaker = match (speaker_bank.as_deref_mut(), group.embedding.as_ref()) {
            (Some(bank), Some(embedding)) => bank.assign(embedding, group.duration_ms),
            _ => "Remote".to_string(),
        };
        for (start_ms, end_ms) in group.ranges {
            turns.push(SpeechTurn {
                start_ms,
                end_ms,
                speaker: speaker.clone(),
                embedding: group.embedding.clone(),
            });
        }
    }
    if turns.is_empty() {
        vec![whole()]
    } else {
        turns
    }
}

struct PreparedGroup {
    ranges: Vec<(u64, u64)>,
    embedding: Option<Vec<f32>>,
    duration_ms: u64,
}

fn prepare_speaker_groups(
    audio: &[f32],
    diarizer: Option<&dyn gravai_intelligence::DiarizationProvider>,
    embedder: &dyn gravai_intelligence::SpeakerEmbedder,
) -> Vec<PreparedGroup> {
    let total_ms = ((audio.len() as u64) * 1000) / 16_000;
    let mut groups: Vec<(Vec<(u64, u64)>, u64)> = Vec::new();
    let segmented = diarizer
        .filter(|diarizer| diarizer.name() != "energy" && audio.len() >= 8_000)
        .and_then(|diarizer| diarizer.diarize(audio).ok())
        .filter(|segments| segments.len() >= 2);
    if let Some(segments) = segmented {
        let mut order = Vec::new();
        let mut ranges: std::collections::HashMap<String, Vec<(u64, u64)>> =
            std::collections::HashMap::new();
        for segment in segments {
            if segment.end_ms <= segment.start_ms {
                continue;
            }
            if !ranges.contains_key(&segment.speaker_id) {
                order.push(segment.speaker_id.clone());
            }
            ranges
                .entry(segment.speaker_id)
                .or_default()
                .push((segment.start_ms, segment.end_ms));
        }
        for speaker_id in order {
            if let Some(speaker_ranges) = ranges.remove(&speaker_id) {
                let duration_ms = speaker_ranges
                    .iter()
                    .map(|(start, end)| end.saturating_sub(*start))
                    .sum();
                groups.push((speaker_ranges, duration_ms));
            }
        }
        groups.sort_by_key(|(ranges, _)| ranges.first().map(|(start, _)| *start).unwrap_or(0));
    }
    if groups.is_empty() {
        groups.push((vec![(0, total_ms.max(1))], total_ms.max(1)));
    }

    groups
        .into_iter()
        .map(|(ranges, duration_ms)| {
            let slice = slice_audio(audio, &ranges);
            let embedding = embedder
                .embed(&slice)
                .ok()
                .filter(|vector| !vector.is_empty());
            PreparedGroup {
                ranges,
                embedding,
                duration_ms,
            }
        })
        .collect()
}

fn slice_audio(audio: &[f32], ranges: &[(u64, u64)]) -> Vec<f32> {
    let mut out = Vec::new();
    for &(start_ms, end_ms) in ranges {
        let start = ((start_ms as usize) * 16_000) / 1000;
        let end = ((end_ms as usize) * 16_000) / 1000;
        let start = start.min(audio.len());
        let end = end.min(audio.len()).max(start);
        out.extend_from_slice(&audio[start..end]);
    }
    out
}

/// Assign each Whisper segment to the turn it overlaps most, then merge
/// consecutive segments from the same person into one line.
fn split_by_turns(
    segments: &[gravai_transcription::TranscriptionSegment],
    turns: &[SpeechTurn],
) -> Vec<FinalizeResult> {
    if segments.is_empty() {
        return Vec::new();
    }
    if turns.is_empty() {
        let text = segments
            .iter()
            .map(|segment| segment.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            return Vec::new();
        }
        let start_ms = segments
            .iter()
            .map(|segment| segment.start_ms)
            .min()
            .unwrap_or(0);
        let end_ms = segments
            .iter()
            .map(|segment| segment.end_ms)
            .max()
            .unwrap_or(start_ms);
        return vec![FinalizeResult {
            text,
            speaker: Some("Remote".to_string()),
            embedding: None,
            start_offset_ms: start_ms as i64,
            end_offset_ms: end_ms as i64,
        }];
    }

    let mut grouped: Vec<(usize, Vec<usize>)> = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let turn = best_turn(segment, turns);
        let same_speaker = grouped
            .last()
            .is_some_and(|(first_turn, _)| turns[*first_turn].speaker == turns[turn].speaker);
        if same_speaker {
            grouped.last_mut().unwrap().1.push(index);
        } else {
            grouped.push((turn, vec![index]));
        }
    }

    grouped
        .into_iter()
        .filter_map(|(turn, indexes)| {
            let text = indexes
                .iter()
                .map(|index| segments[*index].text.trim())
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if text.is_empty() {
                return None;
            }
            let start_ms = indexes
                .iter()
                .map(|index| segments[*index].start_ms)
                .min()
                .unwrap_or(0);
            let end_ms = indexes
                .iter()
                .map(|index| segments[*index].end_ms)
                .max()
                .unwrap_or(start_ms);
            Some(FinalizeResult {
                text,
                speaker: Some(turns[turn].speaker.clone()),
                embedding: turns[turn].embedding.clone(),
                start_offset_ms: start_ms as i64,
                end_offset_ms: end_ms as i64,
            })
        })
        .collect()
}

fn best_turn(segment: &gravai_transcription::TranscriptionSegment, turns: &[SpeechTurn]) -> usize {
    let mut best_index = 0usize;
    let mut best_overlap = 0u64;
    for (index, turn) in turns.iter().enumerate() {
        let start = segment.start_ms.max(turn.start_ms);
        let end = segment.end_ms.min(turn.end_ms);
        let overlap = end.saturating_sub(start);
        if overlap > best_overlap {
            best_overlap = overlap;
            best_index = index;
        }
    }
    if best_overlap > 0 {
        return best_index;
    }
    let midpoint = segment.start_ms / 2 + segment.end_ms / 2;
    turns
        .iter()
        .enumerate()
        .min_by_key(|(_, turn)| {
            let center = turn.start_ms / 2 + turn.end_ms / 2;
            midpoint.abs_diff(center)
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utterance_window_uses_transcription_segment_offsets() {
        let result = FinalizeResult {
            text: "hello there".into(),
            speaker: Some("You".into()),
            embedding: None,
            start_offset_ms: 420,
            end_offset_ms: 1310,
        };

        let (start_ms, end_ms) = utterance_window_ms(16000, 48000, 16000, &result);

        assert_eq!(start_ms, 1420);
        assert_eq!(end_ms, 2310);
    }

    #[test]
    fn utterance_window_clamps_invalid_offsets_to_vad_window() {
        let result = FinalizeResult {
            text: "hello there".into(),
            speaker: Some("You".into()),
            embedding: None,
            start_offset_ms: -200,
            end_offset_ms: 5000,
        };

        let (start_ms, end_ms) = utterance_window_ms(8000, 12000, 16000, &result);

        assert_eq!(start_ms, 500);
        assert_eq!(end_ms, 750);
    }

    fn segment(
        start_ms: u64,
        end_ms: u64,
        text: &str,
    ) -> gravai_transcription::TranscriptionSegment {
        gravai_transcription::TranscriptionSegment {
            start_ms,
            end_ms,
            text: text.into(),
            confidence: 1.0,
            language: None,
        }
    }

    fn turn(start_ms: u64, end_ms: u64, speaker: &str) -> SpeechTurn {
        SpeechTurn {
            start_ms,
            end_ms,
            speaker: speaker.into(),
            embedding: Some(vec![1.0, 0.0]),
        }
    }

    #[test]
    fn two_speakers_in_one_chunk_become_two_lines() {
        let segments = vec![
            segment(0, 1_000, "hello there"),
            segment(1_000, 2_000, "good morning"),
        ];
        let turns = vec![turn(0, 1_000, "Remote 1"), turn(1_000, 2_000, "Remote 2")];
        let lines = split_by_turns(&segments, &turns);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "hello there");
        assert_eq!(lines[0].speaker.as_deref(), Some("Remote 1"));
        assert_eq!(lines[1].text, "good morning");
        assert_eq!(lines[1].speaker.as_deref(), Some("Remote 2"));
        assert!(lines[0].embedding.is_some());
    }

    #[test]
    fn one_speaker_stays_one_line() {
        let segments = vec![segment(0, 800, "hello"), segment(800, 1_600, "there")];
        let turns = vec![turn(0, 2_000, "Remote 1")];
        let lines = split_by_turns(&segments, &turns);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "hello there");
        assert_eq!(lines[0].speaker.as_deref(), Some("Remote 1"));
        assert_eq!(lines[0].start_offset_ms, 0);
        assert_eq!(lines[0].end_offset_ms, 1_600);
    }
}
