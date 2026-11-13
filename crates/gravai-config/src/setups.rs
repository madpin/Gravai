//! Setups — named snapshots of what a recording does.
//!
//! A setup is a bookmark, not a mode. Applying one writes the live config
//! once and nothing stays active. The same [`Snapshot`] is stored with each
//! recording, so a previous start can be restored and a continued recording
//! keeps the settings it started with.

use serde::{Deserialize, Serialize};

use crate::{
    legacy::{presets, profiles},
    AppConfig, CorrectionConfig, ExportConfig, FeaturesConfig, RecordingConfig,
    TranscriptionConfig, VadConfig,
};

/// Assistant values a snapshot carries. The API key is never copied.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmSnapshot {
    pub provider: String,
    pub local_model: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
}

impl Default for LlmSnapshot {
    fn default() -> Self {
        let llm = crate::LlmConfig::default();
        Self {
            provider: llm.provider,
            local_model: llm.local_model,
            base_url: llm.base_url,
            model: llm.model,
            max_tokens: llm.max_tokens,
        }
    }
}

/// Everything that shapes one recording: capture, speech, assistant, sessions.
///
/// Leaves out the mic device and captured app (they belong to this machine
/// and this moment), the API key, search, and updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub mic_enabled: bool,
    pub sys_enabled: bool,
    pub recording: RecordingConfig,
    pub transcription: TranscriptionConfig,
    pub vad: VadConfig,
    pub features: FeaturesConfig,
    pub llm: LlmSnapshot,
    pub correction: CorrectionConfig,
    pub export: ExportConfig,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self::capture(&AppConfig::default())
    }
}

impl Snapshot {
    pub fn capture(config: &AppConfig) -> Self {
        Self {
            mic_enabled: config.audio.microphone.enabled,
            sys_enabled: config.audio.system_audio.enabled,
            recording: config.audio.recording.clone(),
            transcription: config.transcription.clone(),
            vad: config.vad.clone(),
            features: config.features.clone(),
            llm: LlmSnapshot {
                provider: config.llm.provider.clone(),
                local_model: config.llm.local_model.clone(),
                base_url: config.llm.base_url.clone(),
                model: config.llm.model.clone(),
                max_tokens: config.llm.max_tokens,
            },
            correction: config.correction.clone(),
            export: config.export.clone(),
        }
    }

    pub fn apply_to(&self, config: &mut AppConfig) {
        config.audio.microphone.enabled = self.mic_enabled;
        config.audio.system_audio.enabled = self.sys_enabled;
        config.audio.recording = self.recording.clone();
        config.transcription = self.transcription.clone();
        config.vad = self.vad.clone();
        config.features = self.features.clone();
        config.llm.provider = self.llm.provider.clone();
        config.llm.local_model = self.llm.local_model.clone();
        config.llm.base_url = self.llm.base_url.clone();
        config.llm.model = self.llm.model.clone();
        config.llm.max_tokens = self.llm.max_tokens;
        config.correction = self.correction.clone();
        config.export = self.export.clone();
        config.llm.migrate();
        config.correction.migrate();
        config.vad.silero.migrate();
    }

    /// One line describing what this snapshot runs, e.g.
    /// `Whisper medium · English · qwen3-4b · Speakers`.
    pub fn summary(&self) -> String {
        let mut parts = vec![
            format!("Whisper {}", self.transcription.model),
            language_name(&self.transcription.language).to_string(),
            if self.llm.provider == "api" {
                let model = if self.llm.model.is_empty() {
                    "API model"
                } else {
                    &self.llm.model
                };
                format!("{model} (API)")
            } else {
                self.llm.local_model.clone()
            },
        ];
        if !self.mic_enabled || !self.sys_enabled {
            parts.push(
                match (self.mic_enabled, self.sys_enabled) {
                    (true, false) => "Mic only",
                    (false, true) => "System audio only",
                    _ => "No audio sources",
                }
                .into(),
            );
        }
        if self.features.diarization.enabled {
            parts.push("Speakers".into());
        }
        if self.features.sentiment.enabled {
            parts.push("Emotion".into());
        }
        if self.correction.enabled {
            parts.push("Correction".into());
        }
        parts.join(" · ")
    }
}

/// Display name for a Whisper language code.
pub fn language_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "pt" => "Portuguese",
        "es" => "Spanish",
        "fr" => "French",
        "de" => "German",
        "it" => "Italian",
        "nl" => "Dutch",
        "pl" => "Polish",
        "ja" => "Japanese",
        "zh" => "Chinese",
        "auto" => "Auto language",
        other => other,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Shipped with the app. "Restore built-ins" brings these back.
    #[serde(default)]
    pub builtin: bool,
    pub snapshot: Snapshot,
}

/// Built-in templates: Meeting, Podcast, Light.
pub fn builtin_setups() -> Vec<Setup> {
    let profiles = profiles::builtin_profiles();
    let presets = presets::builtin_presets();
    let make = |id: &str, name: &str, description: &str| {
        let mut config = AppConfig::default();
        if let Some(p) = profiles.iter().find(|p| p.id == id) {
            p.apply_to(&mut config);
        }
        if let Some(p) = presets.iter().find(|p| p.id == id) {
            p.apply_to(&mut config);
        }
        Setup {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            builtin: true,
            snapshot: Snapshot::capture(&config),
        }
    };
    vec![
        make(
            "meeting",
            "Meeting",
            "Video calls: mic and app audio, speaker labels",
        ),
        make(
            "podcast",
            "Podcast",
            "High-quality mic recording, most accurate model",
        ),
        make(
            "minimal",
            "Light",
            "Mic only, smallest model, no extra analysis",
        ),
    ]
}

const SETUPS_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupStore {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub setups: Vec<Setup>,
}

impl Default for SetupStore {
    fn default() -> Self {
        Self {
            version: SETUPS_VERSION,
            setups: builtin_setups(),
        }
    }
}

impl SetupStore {
    fn path() -> std::path::PathBuf {
        super::data_dir().join("setups.json")
    }

    /// Load `setups.json`. The first time, build it from the old presets
    /// and profiles (see [`SetupStore::from_legacy`]) and move those files aside.
    pub fn load(live: &AppConfig) -> Self {
        let path = Self::path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<Self>(&content) {
                Ok(store) => return store,
                Err(e) => tracing::warn!("Could not parse setups.json: {e}"),
            }
            return Self::default();
        }
        let dir = super::data_dir();
        let presets_path = dir.join("presets.json");
        let profiles_path = dir.join("profiles.json");
        let has_legacy = presets_path.exists() || profiles_path.exists();
        let store = if has_legacy {
            let presets = presets::PresetStore::load();
            let profiles = profiles::ProfileStore::load();
            Self::from_legacy(live, &presets, &profiles)
        } else {
            Self::default()
        };
        if store.save().is_ok() && has_legacy {
            for p in [presets_path, profiles_path] {
                if p.exists() {
                    let _ = std::fs::rename(&p, p.with_extension("migrated.json"));
                }
            }
            tracing::info!("Moved presets and profiles into setups.json");
        }
        store
    }

    /// Turn presets and profiles into full-snapshot setups. Fields the old
    /// object did not store come from the live config. A preset and a
    /// profile with the same id become one setup.
    pub fn from_legacy(
        live: &AppConfig,
        presets: &presets::PresetStore,
        profiles: &profiles::ProfileStore,
    ) -> Self {
        let mut ids: Vec<String> = profiles.profiles.keys().cloned().collect();
        for id in presets.presets.keys() {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        let builtin_order = ["meeting", "podcast", "minimal"];
        ids.sort_by_key(|id| {
            (
                builtin_order
                    .iter()
                    .position(|b| b == id)
                    .unwrap_or(usize::MAX),
                id.clone(),
            )
        });

        let mut setups = vec![Setup {
            id: "before-update".into(),
            name: "Settings before update".into(),
            description: "What your recordings used before setups replaced presets and profiles"
                .into(),
            builtin: false,
            snapshot: Snapshot::capture(live),
        }];
        for id in ids {
            let mut config = live.clone();
            let profile = profiles.profiles.get(&id);
            let preset = presets.presets.get(&id);
            if let Some(p) = profile {
                p.apply_to(&mut config);
            }
            if let Some(p) = preset {
                p.apply_to(&mut config);
            }
            let (name, description) = match (profile, preset) {
                (Some(p), _) => (p.name.clone(), p.description.clone()),
                (None, Some(p)) => (p.name.clone(), p.description.clone()),
                (None, None) => continue,
            };
            let name = if id == "minimal" && name == "Minimal" {
                "Light".into()
            } else {
                name
            };
            setups.push(Setup {
                builtin: builtin_order.contains(&id.as_str()),
                id,
                name,
                description,
                snapshot: Snapshot::capture(&config),
            });
        }
        // "Before update" is only worth keeping if no other setup holds the same values.
        let before = serde_json::to_value(&setups[0].snapshot).ok();
        if setups[1..]
            .iter()
            .any(|s| serde_json::to_value(&s.snapshot).ok() == before)
        {
            setups.remove(0);
        }
        Self {
            version: SETUPS_VERSION,
            setups,
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = super::data_dir();
        std::fs::create_dir_all(&dir)?;
        let content = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(Self::path(), content)
    }

    pub fn get(&self, id: &str) -> Option<&Setup> {
        self.setups.iter().find(|s| s.id == id)
    }

    /// Store `snapshot` under `name`. Saving with an existing name replaces it.
    pub fn save_as(&mut self, name: &str, snapshot: Snapshot) -> Setup {
        let name = name.trim();
        if let Some(existing) = self
            .setups
            .iter_mut()
            .find(|s| s.name.eq_ignore_ascii_case(name))
        {
            existing.snapshot = snapshot;
            return existing.clone();
        }
        let setup = Setup {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: String::new(),
            builtin: false,
            snapshot,
        };
        self.setups.push(setup.clone());
        setup
    }

    pub fn rename(&mut self, id: &str, name: &str) -> bool {
        match self.setups.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                s.name = name.trim().to_string();
                true
            }
            None => false,
        }
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let before = self.setups.len();
        self.setups.retain(|s| s.id != id);
        self.setups.len() != before
    }

    /// Put back any built-in that was deleted. Edited built-ins are left alone.
    pub fn restore_builtins(&mut self) {
        for (i, builtin) in builtin_setups().into_iter().enumerate() {
            if self.get(&builtin.id).is_none() {
                let at = i.min(self.setups.len());
                self.setups.insert(at, builtin);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trips() {
        let mut config = AppConfig::default();
        config.transcription.language = "pt".into();
        config.features.diarization.enabled = true;
        config.audio.recording.bit_depth = 24;
        let snap = Snapshot::capture(&config);
        let mut other = AppConfig::default();
        snap.apply_to(&mut other);
        assert_eq!(other.transcription.language, "pt");
        assert!(other.features.diarization.enabled);
        assert_eq!(other.audio.recording.bit_depth, 24);
    }

    #[test]
    fn snapshot_leaves_devices_key_and_search_alone() {
        let mut source = AppConfig::default();
        source.audio.microphone.device_name = "Old Mic".into();
        source.audio.system_audio.app_bundle_id = "us.zoom.xos".into();
        source.llm.api_key = Some("secret".into());
        source.embedding.model = "bge-m3".into();
        let snap = Snapshot::capture(&source);
        let json = serde_json::to_string(&snap).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("Old Mic"));

        let mut target = AppConfig::default();
        target.audio.microphone.device_name = "New Mic".into();
        target.llm.api_key = Some("mine".into());
        snap.apply_to(&mut target);
        assert_eq!(target.audio.microphone.device_name, "New Mic");
        assert_eq!(target.audio.system_audio.app_bundle_id, "");
        assert_eq!(target.llm.api_key.as_deref(), Some("mine"));
        assert_eq!(target.embedding.model, "bag-of-words");
    }

    #[test]
    fn builtins_differ() {
        let b = builtin_setups();
        assert_eq!(b.len(), 3);
        let light = b.iter().find(|s| s.id == "minimal").unwrap();
        assert_eq!(light.name, "Light");
        assert!(!light.snapshot.sys_enabled);
        assert_eq!(light.snapshot.transcription.model, "tiny");
        let podcast = b.iter().find(|s| s.id == "podcast").unwrap();
        assert_eq!(podcast.snapshot.transcription.model, "large-v3");
    }

    #[test]
    fn legacy_presets_and_profiles_merge_by_id() {
        let mut live = AppConfig::default();
        live.transcription.language = "de".into();
        let mut presets = presets::PresetStore::default();
        for p in presets::builtin_presets() {
            presets.presets.insert(p.id.clone(), p);
        }
        let mut profiles = profiles::ProfileStore::default();
        for p in profiles::builtin_profiles() {
            profiles.profiles.insert(p.id.clone(), p);
        }
        let store = SetupStore::from_legacy(&live, &presets, &profiles);
        assert_eq!(store.setups[0].id, "before-update");
        assert_eq!(store.setups[1].id, "meeting");
        let podcast = store.get("podcast").unwrap();
        // Profile part
        assert_eq!(podcast.snapshot.transcription.model, "large-v3");
        // Preset part
        assert!(!podcast.snapshot.sys_enabled);
        // Filled from live
        assert_eq!(podcast.snapshot.transcription.language, "de");
        // Preset-only ids survive
        assert!(store.get("streaming").is_some());
        assert_eq!(store.get("minimal").unwrap().name, "Light");
    }

    #[test]
    fn before_update_is_dropped_when_it_duplicates_a_setup() {
        let mut presets = presets::PresetStore::default();
        let mut profiles = profiles::ProfileStore::default();
        for p in presets::builtin_presets() {
            presets.presets.insert(p.id.clone(), p);
        }
        for p in profiles::builtin_profiles() {
            profiles.profiles.insert(p.id.clone(), p);
        }
        // The live config is exactly what Meeting produces.
        let mut live = AppConfig::default();
        profiles.profiles["meeting"].apply_to(&mut live);
        presets.presets["meeting"].apply_to(&mut live);
        let store = SetupStore::from_legacy(&live, &presets, &profiles);
        assert!(store.get("before-update").is_none());
        assert_eq!(store.setups[0].id, "meeting");
    }

    #[test]
    fn save_as_replaces_same_name_and_restore_brings_builtins_back() {
        let mut store = SetupStore::default();
        let a = store.save_as("Calls", Snapshot::default());
        let b = store.save_as("calls", Snapshot::default());
        assert_eq!(a.id, b.id);
        assert!(store.delete("meeting"));
        store.restore_builtins();
        assert_eq!(store.setups[0].id, "meeting");
        assert_eq!(store.setups.len(), 4);
    }

    #[test]
    fn summary_reads_cleanly() {
        let mut config = AppConfig::default();
        config.features.diarization.enabled = true;
        config.audio.system_audio.enabled = false;
        let s = Snapshot::capture(&config).summary();
        assert_eq!(
            s,
            "Whisper medium · English · qwen3-4b · Mic only · Speakers"
        );
    }
}
