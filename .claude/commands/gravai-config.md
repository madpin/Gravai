---
description: Guide to gravai-config crate — AppConfig, presets, profiles, automations, shortcuts
allowed-tools: Read, Glob, Grep
---

You are helping with the `gravai-config` crate at `crates/gravai-config/`. Provide accurate, concise information about the requested topic.

## Crate Overview
Versioned JSON configuration at `~/.gravai/config.json`. Manages the full app config plus separate stores for presets, profiles, shortcuts, and automations. All persisted to disk.

## `AppConfig` Top-Level Structure
```rust
pub struct AppConfig {
    pub version: u32,               // schema version, auto-migrates
    pub audio: AudioConfig,
    pub transcription: TranscriptionConfig,
    pub vad: VadConfig,
    pub features: FeaturesConfig,
    pub llm: LlmConfig,
    pub embedding: EmbeddingConfig,
    pub export: ExportConfig,
    pub updates: UpdatesConfig,
    pub correction: CorrectionConfig,
}
```

### `AudioConfig`
```rust
pub struct AudioConfig {
    pub microphone: MicrophoneConfig,  // enabled, device_id, gain (0.0–2.0)
    pub system_audio: SystemAudioConfig,  // enabled, app_bundle_id, gain
    pub recording: RecordingConfig,    // sample_rate (48000), bit_depth (24), channels (1)
}
```

### `TranscriptionConfig`
```rust
pub struct TranscriptionConfig {
    pub engine: String,           // "whisper"
    pub model: String,            // "base", "small", "medium", "large-v3-turbo", etc.
    pub language: Option<String>, // None = auto-detect
    pub hallucination_blocklist: Vec<String>,  // phrases to filter
}
```

### `VadConfig`
```rust
pub struct VadConfig {
    pub engine: String,           // "webrtc" or "silero"
    pub pause_seconds: f32,       // silence threshold to end utterance
    pub webrtc: WebRtcVadConfig,  // aggressiveness: 0–3
    pub silero: SileroVadConfig,  // threshold: 0.0–1.0
}
```

### `LlmConfig`
```rust
pub struct LlmConfig {
    pub provider: String,         // "local" or "api"
    pub local_model: String,      // GGUF model ID for local inference (e.g. "gemma3-4b-q4")
    pub base_url: String,         // API endpoint URL (only for provider == "api")
    pub model: String,            // API model name (only for provider == "api")
    pub api_key: Option<String>,
    pub max_tokens: u32,
}
```

## Config Functions
```rust
load_config() -> Result<AppConfig>       // reads ~/.gravai/config.json, migrates if needed
save_config(config: &AppConfig) -> Result<()>
deep_merge(base: &mut AppConfig, patch: Value) // merges partial JSON patch into config
```

## Path Helpers
```rust
data_dir() -> PathBuf      // ~/.gravai/ (or ~/.gravai-dev/ in debug)
sessions_dir() -> PathBuf  // ~/.gravai/sessions/
models_dir() -> PathBuf    // ~/.gravai/models/
log_file_path() -> PathBuf // ~/.gravai/gravai.log
```

## Presets (`presets.rs`)
Bundle audio capture settings for different scenarios.
```rust
pub struct CapturePreset {
    pub id: String,
    pub name: String,
    pub mic_enabled: bool, pub mic_gain: f32,
    pub system_enabled: bool, pub system_gain: f32,
    pub sample_rate: u32, pub bit_depth: u16, pub channels: u16,
    pub export_format: String,  // "wav", "m4a-aac", etc.
    pub output_folder: Option<String>,
}
pub struct PresetStore {
    pub presets: HashMap<String, CapturePreset>,
    pub active_preset_id: Option<String>,
}
```
Builtins: `"meeting"`, `"podcast"`, `"streaming"`, `"interview"`, `"minimal"`

## Profiles (`profiles.rs`)
Bundle transcription + AI feature settings.
```rust
pub struct Profile {
    pub id: String,
    pub name: String,
    pub transcription_engine: Option<String>,
    pub transcription_model: Option<String>,
    pub language: Option<String>,
    pub diarization: Option<bool>,
    pub sentiment: Option<bool>,
    pub echo_suppression: Option<bool>,
    pub llm_overrides: Option<LlmConfig>,
    pub shortcut_set: Option<String>,
}
pub struct ProfileStore {
    pub profiles: HashMap<String, Profile>,
    pub active_profile_id: Option<String>,
}
```
Builtins: `"meeting"`, `"podcast"`, `"minimal"`

## Automations (`automations.rs`)
Event-driven automation rules.
```rust
pub enum AutomationTrigger {
    MeetingDetected, MeetingAppDetected { app_name: String },
    MeetingAppEnded, SessionStarted, SessionEnded,
    CalendarEventStarting, TimeOfDay { time: String },
    AppForegrounded { app_name: String },
}
pub enum AutomationCondition {
    AppRunning { app_name: String }, DayOfWeek { days: Vec<String> },
    NoActiveSession, ActiveSession,
}
pub enum AutomationAction {
    ActivateProfile { profile_id: String }, ActivatePreset { preset_id: String },
    StartRecording, StopRecording,
    ShowNotification { title: String, body: String },
    RunExport { format: String },
}
pub struct Automation {
    pub id: String, pub name: String, pub enabled: bool,
    pub trigger: AutomationTrigger,
    pub conditions: Vec<AutomationCondition>,
    pub actions: Vec<AutomationAction>,
    pub last_run: Option<DateTime<Utc>>,
    pub run_count: u32,
}
```

## How Profile/Preset Activation Works at Session Start
1. `start_session()` reads `AppState.active_profile_id` + `active_preset_id`
2. Loads the profile/preset from their respective stores
3. Overrides relevant config fields (engine, model, mic_enabled, gain, etc.)
4. Uses the merged config for the session — **does not persist to `config.json`**

## Tauri Commands for Config
- `get_config()` / `update_config(patch)` / `export_config()` / `import_config()`
- Profile/Preset CRUD in `commands/config_extras.rs`

---

Now answer the user's question about `gravai-config`: $ARGUMENTS
