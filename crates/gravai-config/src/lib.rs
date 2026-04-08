//! Versioned configuration, profiles, presets, shortcuts, automations.
//!
//! Ported from ears-rust-api config.rs, adapted for Gravai:
//! - JSON instead of TOML (easier programmatic merge)
//! - Version field for schema migration
//! - Expanded with Gravai-specific sections

pub mod automations;
pub mod presets;
pub mod profiles;
pub mod shortcuts;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Directory helpers
// ---------------------------------------------------------------------------

pub fn data_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Cannot determine home directory")
        .join(".gravai")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

pub fn log_file_path() -> PathBuf {
    data_dir().join("gravai.log")
}

// ---------------------------------------------------------------------------
// Config structs
// ---------------------------------------------------------------------------

/// Current config schema version. Bump when making breaking changes.
pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MicrophoneConfig {
    pub enabled: bool,
    /// -1 = system default
    pub device_index: i32,
}

impl Default for MicrophoneConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            device_index: -1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemAudioConfig {
    pub enabled: bool,
    pub method: String,
    pub device_index: i32,
    pub app_bundle_id: String,
}

impl Default for SystemAudioConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            method: "screencapturekit".into(),
            device_index: -1,
            app_bundle_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingConfig {
    /// Whether to save audio files to disk (can be disabled to only transcribe)
    pub enabled: bool,
    /// Recording sample rate in Hz
    pub sample_rate: u32,
    /// Bits per sample for recording
    pub bit_depth: u16,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u16,
    /// Default export format: "wav", "aiff", "caf", "m4a-aac", "m4a-alac"
    pub export_format: String,
    /// AAC bitrate in kbps (for m4a-aac export)
    pub aac_bitrate_kbps: u32,
    /// Custom output folder (None = default ~/.gravai/sessions/)
    pub output_folder: Option<String>,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sample_rate: 48_000,
            bit_depth: 24,
            channels: 2,
            export_format: "m4a-aac".into(),
            aac_bitrate_kbps: 192,
            output_folder: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct AudioConfig {
    pub microphone: MicrophoneConfig,
    pub system_audio: SystemAudioConfig,
    pub recording: RecordingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptionConfig {
    pub engine: String,
    pub model: String,
    pub language: String,
    pub hallucination_blocklist: Vec<String>,
    pub hallucination_repeat_blocklist: Vec<String>,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            engine: "whisper".into(),
            model: "medium".into(),
            language: "en".into(),
            hallucination_blocklist: vec![
                "thanks for watching".into(),
                "thank you for watching".into(),
                "please subscribe".into(),
                "like and subscribe".into(),
                "thanks for listening".into(),
                "thank you for listening".into(),
                "see you next time".into(),
                "subtitles by".into(),
                "subtitles".into(),
                "the end".into(),
                "silence".into(),
                "music".into(),
                "applause".into(),
                "laughter".into(),
            ],
            hallucination_repeat_blocklist: vec![
                "thank you".into(),
                "thanks".into(),
                "bye".into(),
                "goodbye".into(),
                "see you".into(),
                "okay".into(),
                "ok".into(),
                "yes".into(),
                "yeah".into(),
                "no".into(),
                "hmm".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SileroVadConfig {
    pub threshold: f32,
    pub min_utterance_seconds: f32,
    pub max_utterance_seconds: f32,
}

impl Default for SileroVadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_utterance_seconds: 0.3,
            max_utterance_seconds: 30.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebrtcVadConfig {
    pub aggressiveness: i32,
}

impl Default for WebrtcVadConfig {
    fn default() -> Self {
        Self { aggressiveness: 3 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VadConfig {
    pub engine: String,
    pub pause_seconds: f32,
    pub silero: SileroVadConfig,
    pub webrtc: WebrtcVadConfig,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            engine: "webrtc".into(),
            pause_seconds: 0.5,
            silero: SileroVadConfig::default(),
            webrtc: WebrtcVadConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EchoSuppressionConfig {
    pub enabled: bool,
    pub similarity_threshold: f64,
}

impl Default for EchoSuppressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            similarity_threshold: 0.55,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub provider: String,
    pub base_url: String,
    pub model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "ollama".into(),
            base_url: "http://localhost:11434/v1".into(),
            model: "gemma3:4b".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiarizationConfig {
    pub enabled: bool,
    pub max_speakers: u8,
    /// Diarization engine: "energy" (default, no model needed) or "pyannote" (ONNX, needs model files).
    pub model: String,
}

impl Default for DiarizationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_speakers: 10,
            model: "energy".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MeetingDetectionConfig {
    pub enabled: bool,
    pub lead_time_seconds: u32,
    pub idle_timeout_seconds: u32,
    pub allowed_apps: Vec<String>,
}

impl Default for MeetingDetectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            lead_time_seconds: 0,
            idle_timeout_seconds: 30,
            allowed_apps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct FeaturesConfig {
    pub echo_suppression: EchoSuppressionConfig,
    pub diarization: DiarizationConfig,
    pub meeting_detection: MeetingDetectionConfig,
}

// ---------------------------------------------------------------------------
// Export config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportConfig {
    /// Auto-export transcript on session end
    pub auto_export_transcript: bool,
    /// Auto-export audio on session end
    pub auto_export_audio: bool,
    /// Default folder for markdown/summary exports (None = ~/.gravai/exports/)
    pub transcript_folder: Option<String>,
    /// Default folder for audio exports (None = same as session folder)
    pub audio_folder: Option<String>,
    /// Default transcript format: "markdown", "pdf", "txt"
    pub transcript_format: String,
    /// Auto-save transcript in real-time (crash-safe)
    pub realtime_save: bool,
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            auto_export_transcript: false,
            auto_export_audio: false,
            transcript_folder: None,
            audio_folder: None,
            transcript_format: "markdown".into(),
            realtime_save: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Updates config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatesConfig {
    /// Check for updates automatically when the app launches.
    pub auto_check: bool,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self { auto_check: true }
    }
}

// ---------------------------------------------------------------------------
// Top-level AppConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub version: u32,
    pub audio: AudioConfig,
    pub transcription: TranscriptionConfig,
    pub vad: VadConfig,
    pub features: FeaturesConfig,
    pub llm: LlmConfig,
    pub export: ExportConfig,
    pub updates: UpdatesConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            audio: AudioConfig::default(),
            transcription: TranscriptionConfig::default(),
            vad: VadConfig::default(),
            features: FeaturesConfig::default(),
            llm: LlmConfig::default(),
            export: ExportConfig::default(),
            updates: UpdatesConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers (ported from ears config.rs)
// ---------------------------------------------------------------------------

/// Recursively merge `patch` into `base`, returning a new Value.
pub fn deep_merge(base: &serde_json::Value, patch: &serde_json::Value) -> serde_json::Value {
    match (base, patch) {
        (serde_json::Value::Object(b), serde_json::Value::Object(p)) => {
            let mut merged = b.clone();
            for (key, value) in p {
                let base_val = merged.get(key).cloned().unwrap_or(serde_json::Value::Null);
                merged.insert(key.clone(), deep_merge(&base_val, value));
            }
            serde_json::Value::Object(merged)
        }
        (_, patch) => patch.clone(),
    }
}

/// Load config from disk, falling back to defaults for missing keys.
pub fn load_config() -> AppConfig {
    let path = config_path();
    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
                Ok(config) => {
                    tracing::info!("Config loaded from {}", path.display());
                    return config;
                }
                Err(e) => {
                    tracing::warn!("Failed to parse config: {e}, using defaults");
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read config: {e}, using defaults");
            }
        }
    } else {
        tracing::info!("No config file found, using defaults");
    }
    AppConfig::default()
}

/// Persist config to JSON.
pub fn save_config(config: &AppConfig) -> std::io::Result<()> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    let content = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(config_path(), content)?;
    tracing::info!("Config saved to {}", config_path().display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_serializes() {
        let config = AppConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, CONFIG_VERSION);
        assert_eq!(parsed.audio.recording.sample_rate, 48_000);
    }

    #[test]
    fn deep_merge_works() {
        let base = serde_json::json!({"a": 1, "b": {"c": 2, "d": 3}});
        let patch = serde_json::json!({"b": {"c": 99}, "e": 5});
        let merged = deep_merge(&base, &patch);
        assert_eq!(merged["a"], 1);
        assert_eq!(merged["b"]["c"], 99);
        assert_eq!(merged["b"]["d"], 3);
        assert_eq!(merged["e"], 5);
    }
}
