//! Capture presets — saved audio configuration bundles.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A capture preset: complete audio source + recording configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturePreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub mic_enabled: bool,
    /// Unused. Nothing in the pipeline applies gain; kept so older files parse.
    #[serde(default = "unity_gain")]
    pub mic_gain: f32,
    pub sys_enabled: bool,
    /// Unused, see `mic_gain`.
    #[serde(default = "unity_gain")]
    pub sys_gain: f32,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub channels: u16,
    pub export_format: String,
    pub output_folder: Option<String>,
}

fn unity_gain() -> f32 {
    1.0
}

impl CapturePreset {
    /// Write this preset's capture settings onto `config`.
    pub fn apply_to(&self, config: &mut crate::AppConfig) {
        config.audio.microphone.enabled = self.mic_enabled;
        config.audio.system_audio.enabled = self.sys_enabled;
        config.audio.recording.sample_rate = self.sample_rate;
        config.audio.recording.bit_depth = self.bit_depth;
        config.audio.recording.channels = self.channels;
        config.audio.recording.export_format = self.export_format.clone();
        if let Some(ref folder) = self.output_folder {
            config.audio.recording.output_folder = Some(folder.clone());
        }
    }
}

impl Default for CapturePreset {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Default".into(),
            description: "Standard recording preset".into(),
            mic_enabled: true,
            mic_gain: 1.0,
            sys_enabled: true,
            sys_gain: 1.0,
            sample_rate: 48000,
            bit_depth: 24,
            channels: 2,
            export_format: "wav".into(),
            output_folder: None,
        }
    }
}

/// Built-in preset templates.
pub fn builtin_presets() -> Vec<CapturePreset> {
    vec![
        CapturePreset {
            id: "meeting".into(),
            name: "Meeting".into(),
            description: "Balanced for video calls".into(),
            mic_enabled: true,
            mic_gain: 1.0,
            sys_enabled: true,
            sys_gain: 1.0,
            sample_rate: 48000,
            bit_depth: 16,
            channels: 1,
            export_format: "m4a-aac".into(),
            output_folder: None,
        },
        CapturePreset {
            id: "podcast".into(),
            name: "Podcast".into(),
            description: "High-quality stereo recording".into(),
            mic_enabled: true,
            mic_gain: 1.2,
            sys_enabled: false,
            sys_gain: 1.0,
            sample_rate: 48000,
            bit_depth: 24,
            channels: 2,
            export_format: "wav".into(),
            output_folder: None,
        },
        CapturePreset {
            id: "streaming".into(),
            name: "Streaming".into(),
            description: "Capture mic + game/app audio".into(),
            mic_enabled: true,
            mic_gain: 1.0,
            sys_enabled: true,
            sys_gain: 0.8,
            sample_rate: 48000,
            bit_depth: 24,
            channels: 2,
            export_format: "wav".into(),
            output_folder: None,
        },
        CapturePreset {
            id: "interview".into(),
            name: "Interview".into(),
            description: "Mono, speech-optimized".into(),
            mic_enabled: true,
            mic_gain: 1.1,
            sys_enabled: true,
            sys_gain: 1.0,
            sample_rate: 48000,
            bit_depth: 24,
            channels: 1,
            export_format: "wav".into(),
            output_folder: None,
        },
        CapturePreset {
            id: "minimal".into(),
            name: "Minimal".into(),
            description: "Low resource usage".into(),
            mic_enabled: true,
            mic_gain: 1.0,
            sys_enabled: false,
            sys_gain: 1.0,
            sample_rate: 16000,
            bit_depth: 16,
            channels: 1,
            export_format: "m4a-aac".into(),
            output_folder: None,
        },
    ]
}

/// Preset store backed by a JSON file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PresetStore {
    pub presets: HashMap<String, CapturePreset>,
    pub active_preset_id: Option<String>,
}

impl PresetStore {
    pub fn load() -> Self {
        let path = crate::data_dir().join("presets.json");
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(store) = serde_json::from_str(&content) {
                    return store;
                }
            }
        }
        // Initialize with built-in presets
        let mut store = Self::default();
        for preset in builtin_presets() {
            store.presets.insert(preset.id.clone(), preset);
        }
        store
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = crate::data_dir().join("presets.json");
        let content = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, content)
    }
}
