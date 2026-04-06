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
}

/// Callback invoked when a new utterance is finalized.
pub type OnUtterance = Arc<dyn Fn(Utterance) + Send + Sync>;

/// Configuration for the pipeline.
pub struct PipelineConfig {
    pub pause_seconds: f32,
    pub min_utterance_seconds: f32,
    pub max_utterance_seconds: f32,
    pub sample_rate: u32, // 16000 for transcription path
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            pause_seconds: 0.5,
            min_utterance_seconds: 0.3,
            max_utterance_seconds: 30.0,
            sample_rate: 16000,
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
        }
    }
}

/// All inputs needed to run a transcription pipeline for one audio source.
pub struct PipelineInput {
    pub rx: mpsc::Receiver<Vec<f32>>,
    pub source: String,
    pub vad: Box<dyn VadProvider>,
    pub transcriber: Option<Arc<dyn gravai_transcription::TranscriptionProvider>>,
    pub diarizer:
        Option<Arc<tokio::sync::Mutex<Box<dyn gravai_intelligence::DiarizationProvider>>>>,
    pub echo_suppressor: Arc<tokio::sync::Mutex<EchoSuppressor>>,
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
        diarizer,
        echo_suppressor,
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
            speech_buffer.extend_from_slice(&chunk);
            silence_count = 0;

            // Force-split long utterances
            if speech_buffer.len() >= max_samples {
                if let Some(result) = finalize(
                    &speech_buffer,
                    &source,
                    &transcriber,
                    &diarizer,
                    &echo_suppressor,
                    &last_text,
                )
                .await
                {
                    last_text = Some(result.text.clone());
                    on_utterance(Utterance {
                        text: result.text,
                        source: source.clone(),
                        speaker: result.speaker,
                        timestamp: chrono::Utc::now(),
                    });
                }
                speech_buffer.clear();
            }
        } else {
            silence_count += chunk.len();
            if !speech_buffer.is_empty() && silence_count >= pause_samples {
                if speech_buffer.len() >= min_samples {
                    if let Some(result) = finalize(
                        &speech_buffer,
                        &source,
                        &transcriber,
                        &diarizer,
                        &echo_suppressor,
                        &last_text,
                    )
                    .await
                    {
                        last_text = Some(result.text.clone());
                        on_utterance(Utterance {
                            text: result.text,
                            source: source.clone(),
                            speaker: result.speaker,
                            timestamp: chrono::Utc::now(),
                        });
                    }
                }
                speech_buffer.clear();
                silence_count = 0;
            }
        }
    }

    // Flush remaining speech
    if !speech_buffer.is_empty() && speech_buffer.len() >= min_samples {
        if let Some(result) = finalize(
            &speech_buffer,
            &source,
            &transcriber,
            &diarizer,
            &echo_suppressor,
            &last_text,
        )
        .await
        {
            on_utterance(Utterance {
                text: result.text,
                source: source.clone(),
                speaker: result.speaker,
                timestamp: chrono::Utc::now(),
            });
        }
    }

    debug!("Transcription pipeline for {source} ended");
}

/// Result of finalize: transcribed text + optional speaker label.
struct FinalizeResult {
    text: String,
    speaker: Option<String>,
}

/// Transcribe accumulated audio, run diarization, and apply filters.
#[allow(clippy::too_many_arguments)]
async fn finalize(
    audio: &[f32],
    source: &str,
    transcriber: &Option<Arc<dyn gravai_transcription::TranscriptionProvider>>,
    diarizer: &Option<Arc<tokio::sync::Mutex<Box<dyn gravai_intelligence::DiarizationProvider>>>>,
    echo_suppressor: &Arc<tokio::sync::Mutex<EchoSuppressor>>,
    last_text: &Option<String>,
) -> Option<FinalizeResult> {
    let engine = transcriber.as_ref()?;
    let engine = Arc::clone(engine);

    // Run transcription in blocking task (CPU-bound)
    let audio_owned = audio.to_vec();
    let segments = tokio::task::spawn_blocking(move || engine.transcribe(&audio_owned))
        .await
        .ok()?
        .ok()?;

    if segments.is_empty() {
        return None;
    }

    let text: String = segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }

    // Repeat check
    if let Some(ref prev) = last_text {
        if text.trim().to_lowercase() == prev.trim().to_lowercase() {
            debug!(
                "Repeat suppressed ({source}): {}",
                &text[..text.len().min(60)]
            );
            return None;
        }
    }

    // Echo suppression
    {
        let mut es = echo_suppressor.lock().await;
        if es.is_echo(&text, source) {
            debug!(
                "Echo suppressed ({source}): {}",
                &text[..text.len().min(60)]
            );
            return None;
        }
        es.add(&text, source);
    }

    // Run diarization if available
    let speaker = if let Some(ref diarizer) = diarizer {
        let d = diarizer.lock().await;
        match d.diarize(audio) {
            Ok(segments) if !segments.is_empty() => {
                // Pick the speaker with the most coverage
                let mut counts: std::collections::HashMap<&str, u64> =
                    std::collections::HashMap::new();
                for seg in &segments {
                    *counts.entry(&seg.speaker_id).or_default() += seg.end_ms - seg.start_ms;
                }
                counts
                    .into_iter()
                    .max_by_key(|&(_, dur)| dur)
                    .map(|(id, _)| id.to_string())
            }
            _ => None,
        }
    } else {
        None
    };

    Some(FinalizeResult { text, speaker })
}
