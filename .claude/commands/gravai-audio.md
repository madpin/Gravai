---
description: Guide to gravai-audio crate — capture, VAD pipeline, resampling, multi-track WAV, echo suppression
allowed-tools: Read, Glob, Grep
---

You are helping with the `gravai-audio` crate at `crates/gravai-audio/`. Provide accurate, concise information about the requested topic.

## Crate Overview
All audio capture, processing, and recording. Two audio sources: microphone (CPAL) and system audio (ScreenCaptureKit on macOS).

## Key Modules & Types

### `capture.rs` — Microphone Capture
- `AudioCaptureManager`: initializes CPAL streams for mic input
- `AudioChunk { sample_rate, channels, samples: Vec<f32>, timestamp }` — the unit of audio data
- Runs on a **dedicated OS thread** (CPAL streams are not `Send`)
- Supports device selection and per-device config

### `screencapturekit.rs` — System Audio (macOS only)
- `list_capturable_apps()` → apps with bundle IDs (requires Screen Recording permission)
- Captures per-app system audio using ScreenCaptureKit framework
- Also captures all system audio when no specific app is selected

### `resampler.rs` — Sample Rate Conversion
- Uses `rubato` crate
- Converts native capture rate (typically 48kHz) → 16kHz mono for transcription
- Two channels per source: HQ (original rate, for WAV recording) and LQ (16kHz, for transcription)
- Channel types: `std::sync::mpsc` for CPAL→resampler, `tokio::sync::mpsc` for resampler→pipeline

### `vad.rs` — Voice Activity Detection
```rust
pub trait VadProvider: Send {
    fn is_speech(&mut self, samples: &[f32]) -> bool;
}
```
- `WebRtcVad`: WebRTC-based VAD, aggressiveness 0–3
- `SileroVad`: ONNX model (`~/.gravai/models/silero_vad.onnx`), more accurate
- Both operate on 16kHz mono 30ms frames

### `pipeline.rs` — Core VAD→Transcription Loop
```rust
pub struct PipelineConfig {
    pub pause_seconds: f32,           // silence to trigger transcription (e.g., 1.5s)
    pub min_utterance_seconds: f32,   // ignore too-short speech
    pub max_utterance_seconds: f32,   // force split long utterances (default 30s)
    pub sample_rate: u32,             // 16000
}

pub struct PipelineInput {
    pub audio_rx: mpsc::Receiver<AudioChunk>,
    pub source_name: String,          // "mic" or "system"
    pub vad: Box<dyn VadProvider>,
    pub transcriber: Box<dyn TranscriptionProvider>,
    pub echo_suppressor: Option<EchoSuppressor>,
    pub diarizer: Option<Box<dyn DiarizationProvider>>,
    pub on_utterance: Arc<dyn Fn(Utterance) + Send + Sync>,
    pub active: Arc<AtomicBool>,
}
```
- `run_pipeline()` async: receive 16kHz mono chunks → VAD decision → accumulate speech → transcribe on pause
- Handles force-split for `>max_utterance_seconds` and minimum silence (`0.5s` default)
- `Utterance { text, source, speaker, timestamp, confidence, start_ms, end_ms }`

### `recorder.rs` — Multi-Track WAV
- `TrackWriter`: WAV file writer (float32 PCM), lazy header init, gain control
- `MultiTrackRecorder { tracks: HashMap<String, TrackWriter> }`: named tracks ("mic", "system")
- `finalize()` **must** be called explicitly to write correct WAV header (sample count)
- Drop impl provides safety fallback

### `echo.rs` — Echo/Feedback Suppression
- `EchoSuppressor`: detects near-duplicate text across mic/system using string similarity
- Prevents same speech appearing twice (e.g., meeting room echo)

### `encoder.rs` — Audio Export Formats
- Supported: WAV (float32), M4A-AAC, M4A-ALAC, AIFF, CAF
- Used by `export_session_audio()` Tauri command

### `silence.rs`
- Silence duration detection, separate from VAD (complements it)
- Used by the silence monitor in `lib.rs` to detect audio device failures

## Audio Flow
```
CPAL mic stream (48kHz)   ──→ mpsc HQ ──→ TrackWriter (mic.wav)
                           └──→ mpsc LQ ──→ Resampler ──→ 16kHz mono
                                                           ──→ VAD pipeline ──→ Whisper
ScreenCaptureKit (48kHz)  ──→ mpsc HQ ──→ TrackWriter (system.wav)
                           └──→ mpsc LQ ──→ Resampler ──→ 16kHz mono
                                                           ──→ VAD pipeline ──→ Whisper
```

## Threading Notes
- CPAL stream callback runs on OS audio thread — use `std::sync::mpsc` to cross thread boundary
- VAD pipeline runs as `tokio::spawn` task
- `MultiTrackRecorder` shared via `Arc<Mutex<MultiTrackRecorder>>` across threads

---

Now answer the user's question about `gravai-audio`: $ARGUMENTS
