//! Profiles — named bundles of settings for different contexts.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A profile bundles multiple settings into a switchable context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Reference to a capture preset ID
    pub preset_id: Option<String>,
    /// Transcription overrides
    pub transcription_engine: Option<String>,
    pub transcription_model: Option<String>,
    pub transcription_language: Option<String>,
    /// Feature toggles
    pub diarization_enabled: Option<bool>,
    pub sentiment_enabled: Option<bool>,
    pub echo_suppression_enabled: Option<bool>,
    /// Background meeting-app scan. `None` leaves the saved config as-is.
    pub meeting_enabled: Option<bool>,
    /// VAD engine override: "webrtc" (light) or "silero" (heavier ONNX model).
    pub vad_engine: Option<String>,
    pub vad_pause: Option<f32>,
    pub vad_min: Option<f32>,
    pub vad_max: Option<f32>,
    /// Per-utterance LLM transcript correction toggle.
    pub correction_enabled: Option<bool>,
    /// LLM overrides
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    /// GGUF model ID override for local inference (e.g. "gemma-4-e2b").
    pub llm_local_model: Option<String>,
    /// Shortcut set ID
    pub shortcut_set_id: Option<String>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Default".into(),
            description: "Standard profile".into(),
            preset_id: None,
            transcription_engine: None,
            transcription_model: None,
            transcription_language: None,
            diarization_enabled: None,
            sentiment_enabled: None,
            echo_suppression_enabled: None,
            meeting_enabled: None,
            vad_engine: None,
            vad_pause: None,
            vad_min: None,
            vad_max: None,
            correction_enabled: None,
            llm_provider: None,
            llm_model: None,
            llm_local_model: None,
            shortcut_set_id: None,
        }
    }
}

impl Profile {
    /// Write every field this profile sets onto `config`. Unset fields are left alone.
    pub fn apply_to(&self, config: &mut crate::AppConfig) {
        if let Some(ref engine) = self.transcription_engine {
            config.transcription.engine = engine.clone();
        }
        if let Some(ref model) = self.transcription_model {
            config.transcription.model = model.clone();
        }
        if let Some(ref lang) = self.transcription_language {
            config.transcription.language = lang.clone();
        }
        if let Some(ref provider) = self.llm_provider {
            config.llm.provider = provider.clone();
        }
        if let Some(ref model) = self.llm_model {
            config.llm.model = model.clone();
        }
        if let Some(ref local_model) = self.llm_local_model {
            config.llm.local_model = local_model.clone();
        }
        // Re-run migration in case the profile has legacy provider values
        config.llm.migrate();
        if let Some(enabled) = self.diarization_enabled {
            config.features.diarization.enabled = enabled;
        }
        if let Some(enabled) = self.echo_suppression_enabled {
            config.features.echo_suppression.enabled = enabled;
        }
        if let Some(enabled) = self.correction_enabled {
            config.correction.enabled = enabled;
        }
        if let Some(enabled) = self.sentiment_enabled {
            config.features.sentiment.enabled = enabled;
        }
        if let Some(enabled) = self.meeting_enabled {
            config.features.meeting_detection.enabled = enabled;
        }
        if let Some(ref engine) = self.vad_engine {
            config.vad.engine = engine.clone();
        }
        if let Some(pause) = self.vad_pause {
            config.vad.pause_seconds = pause;
        }
        if let Some(min) = self.vad_min {
            config.vad.silero.min_utterance_seconds = min;
        }
        if let Some(max) = self.vad_max {
            config.vad.silero.max_utterance_seconds = max;
        }
    }
}

/// Built-in profiles.
pub fn builtin_profiles() -> Vec<Profile> {
    vec![
        Profile {
            id: "meeting".into(),
            name: "Meeting".into(),
            description: "Optimized for video calls".into(),
            preset_id: Some("meeting".into()),
            transcription_model: Some("medium".into()),
            diarization_enabled: Some(true),
            sentiment_enabled: Some(true),
            ..Default::default()
        },
        Profile {
            id: "podcast".into(),
            name: "Podcast".into(),
            description: "High-quality recording focus".into(),
            preset_id: Some("podcast".into()),
            transcription_model: Some("large-v3".into()),
            diarization_enabled: Some(true),
            ..Default::default()
        },
        Profile {
            id: "minimal".into(),
            name: "Minimal".into(),
            description: "Low resource usage".into(),
            preset_id: Some("minimal".into()),
            transcription_model: Some("tiny".into()),
            diarization_enabled: Some(false),
            sentiment_enabled: Some(false),
            echo_suppression_enabled: Some(false),
            meeting_enabled: Some(false),
            correction_enabled: Some(false),
            vad_engine: Some("webrtc".into()),
            ..Default::default()
        },
    ]
}

/// Profile store.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileStore {
    pub profiles: HashMap<String, Profile>,
    pub active_profile_id: Option<String>,
}

impl ProfileStore {
    pub fn load() -> Self {
        let path = crate::data_dir().join("profiles.json");
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut store) = serde_json::from_str::<Self>(&content) {
                    let mut migrated = false;
                    for profile in store.profiles.values_mut() {
                        if profile.vad_max.is_some_and(|max| (max - 30.0).abs() < 0.01) {
                            profile.vad_max = Some(12.0);
                            migrated = true;
                        }
                        if profile
                            .llm_local_model
                            .as_deref()
                            .is_some_and(|m| m.contains("gemma"))
                        {
                            profile.llm_local_model = Some("qwen3-4b".into());
                            migrated = true;
                        }
                        if profile
                            .llm_model
                            .as_deref()
                            .is_some_and(|m| m.to_lowercase().contains("gemma"))
                        {
                            profile.llm_model = None;
                            if profile.llm_local_model.is_none() {
                                profile.llm_local_model = Some("qwen3-4b".into());
                            }
                            migrated = true;
                        }
                        if let Some(ref p) = profile.llm_provider {
                            let new_p = match p.as_str() {
                                "ollama" => Some("local"),
                                "openai" | "anthropic" => Some("api"),
                                _ => None,
                            };
                            if let Some(np) = new_p {
                                profile.llm_provider = Some(np.to_string());
                                migrated = true;
                            }
                        }
                    }
                    if migrated {
                        let _ = store.save();
                    }
                    return store;
                }
            }
        }
        let mut store = Self::default();
        for profile in builtin_profiles() {
            store.profiles.insert(profile.id.clone(), profile);
        }
        store
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = crate::data_dir().join("profiles.json");
        let content = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, content)
    }
}
