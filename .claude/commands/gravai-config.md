---
description: Guide to gravai-config crate — AppConfig, setups, automations, shortcuts
allowed-tools: Read, Glob, Grep
---

You are helping with the `gravai-config` crate at `crates/gravai-config/`. Provide accurate, concise information about the requested topic.

## Crate Overview
Versioned JSON configuration at `~/.gravai/config.json`. Manages the full app config plus separate stores for setups, shortcuts, and automations. All persisted to disk. The live config is what the next recording uses; nothing is layered on top at start.

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

## Setups (`setups.rs`)
Named snapshots of what a recording does. Applying is one-shot: it writes the live config and nothing stays active.
```rust
pub struct Snapshot {          // capture, speech, assistant, sessions
    pub mic_enabled: bool, pub sys_enabled: bool,
    pub recording: RecordingConfig, pub transcription: TranscriptionConfig,
    pub vad: VadConfig, pub features: FeaturesConfig,
    pub llm: LlmSnapshot,      // provider, local_model, base_url, model, max_tokens — never api_key
    pub correction: CorrectionConfig, pub export: ExportConfig,
}
impl Snapshot { fn capture(&AppConfig) -> Self; fn apply_to(&self, &mut AppConfig); fn summary(&self) -> String }
pub struct Setup { pub id: String, pub name: String, pub description: String, pub builtin: bool, pub snapshot: Snapshot }
pub struct SetupStore { pub version: u32, pub setups: Vec<Setup> }   // ~/.gravai/setups.json, ordered
```
- Snapshots leave out mic device, captured app, API key, `embedding`, `updates`.
- Builtins: `"meeting"` (Meeting), `"podcast"` (Podcast), `"minimal"` (Light).
- `SetupStore::load(&live)`: first run turns `presets.json` + `profiles.json` into setups (each filled from the live config, shared ids merged, plus "Settings before update"), then renames them to `*.migrated.json`.
- The same `Snapshot` JSON is stored per recording in `sessions.config_snapshot` (recents, continuation).
- `legacy/{presets,profiles}.rs` (crate-private) are only read by migrations.

## Automations (`automations.rs`)
Event-driven automation rules.
```rust
pub enum AutomationTrigger {
    MeetingDetected, MeetingAppDetected { app_name: String },
    MeetingAppEnded { app_name: String },
    AudioSilent,  // mic+system both silent; evaluated by the silence monitor in lib.rs
    SessionStarted, SessionEnded,
    CalendarEventStarting, TimeOfDay { hour: u8, minute: u8 },
    AppForegrounded { app_name: String },
}
pub enum AutomationCondition {
    AppRunning { app_name: String }, DayOfWeek { days: Vec<String> },
    NoActiveSession, ActiveSession,
}
pub enum AutomationAction {
    ApplySetup { setup_id: String },   // old activate_profile/activate_preset JSON is rewritten on load
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

Builtins: `auto-meeting-record` (start only — never changes settings, defaults v3), `zoom-auto-stop`, `silence-auto-stop`, `auto-export-on-stop`.

## Shortcuts (`shortcuts.rs`)
`ShortcutBinding { action_id, key_sequence, description, is_global }`. Actions: `recording.start|stop|pause|bookmark`. `rebind(action, key, is_global)`; empty key = off. Load drops unknown actions and moves never-registered old defaults to `CmdOrCtrl+Alt+R/S/P`. Global keys are registered by `src-tauri/src/shortcuts.rs`; local ones by `StatusBar.svelte`. Both emit/handle `gravai:shortcut`.

## Launch order
`load_config` → `migrate_active_bundles` (folds leftover active profile/preset into config) → `SetupStore::load` (one-time setup migration).

## Tauri Commands for Config
- `get_config()` / `update_config(patch)` / `export_config()` / `import_config()`
- Setups in `commands/config_extras.rs`: `list_setups`, `save_current_setup`, `rename_setup`, `delete_setup`, `restore_builtin_setups`, `apply_setup`, `apply_recent`, `apply_snapshot` (Undo), `list_recent_starts`, `get_run_summary`. Apply commands return the replaced `Snapshot` and emit `gravai:config-changed`.

---

Now answer the user's question about `gravai-config`: $ARGUMENTS
