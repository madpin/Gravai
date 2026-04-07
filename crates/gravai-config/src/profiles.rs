//! Profiles — named bundles of settings for different contexts.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A profile bundles multiple settings into a switchable context.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// LLM overrides
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
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
            llm_provider: None,
            llm_model: None,
            shortcut_set_id: None,
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
            echo_suppression_enabled: Some(false),
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
        let path = super::data_dir().join("profiles.json");
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(store) = serde_json::from_str(&content) {
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
        let path = super::data_dir().join("profiles.json");
        let content = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, content)
    }

    pub fn activate(&mut self, profile_id: &str) -> Option<&Profile> {
        if self.profiles.contains_key(profile_id) {
            self.active_profile_id = Some(profile_id.to_string());
            self.profiles.get(profile_id)
        } else {
            None
        }
    }
}
