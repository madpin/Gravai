---
description: Guide to gravai-core crate — AppState, EventBus, Session FSM, logging, preflight
allowed-tools: Read, Glob, Grep
---

You are helping with the `gravai-core` crate at `crates/gravai-core/`. Provide accurate, concise information about the requested topic.

## Crate Overview
Central abstractions shared by all other crates. No audio, no transcription, no storage — just state, events, session FSM, and cross-cutting concerns.

## Key Types

### `AppState` (`app_state.rs`)
```rust
pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub event_bus: EventBus,
    pub session: RwLock<Option<Arc<Session>>>,
    pub active_profile_id: RwLock<Option<String>>,
    pub active_preset_id: RwLock<Option<String>>,
}
```
- Managed as `Arc<AppState>` via `tauri::manage()`
- All Tauri commands access it via `State<'_, Arc<AppState>>`

### `Session` (`session.rs`)
```rust
pub struct Session {
    pub id: String,           // YYYYMMDD_HHMMSS format
    pub started_at: DateTime<Utc>,
    pub session_dir: PathBuf, // ~/.gravai/sessions/{id}/
    // atomic FSM state: Idle(0), Recording(1), Paused(2), Stopped(3)
    // background task handles for cleanup on stop
}
```
- `SessionState` enum: `Idle`, `Recording`, `Paused`, `Stopped` (stored as AtomicU8, SeqCst)
- `generate_session_id()` → `"YYYYMMDD_HHMMSS"` string

### `EventBus` + `GravaiEvent` (`event_bus.rs`)
```rust
pub enum GravaiEvent {
    SessionStateChanged { session_id: String, state: SessionState },
    TranscriptUpdated { session_id: String, utterance: UtteranceRecord },
    VolumeLevel { source: String, level: f32 },
    MeetingDetected { app_name: String, title: Option<String> },
    MeetingEnded { app_name: String },
    PresetActivated { preset_id: String },
    ProfileSwitched { profile_id: String },
    DownloadProgress { model_id: String, downloaded: u64, total: u64 },
    Error { message: String },
    TranscriptCorrected { utterance_id: String, corrected_text: String },
}
```
- `EventBus`: wraps tokio `broadcast::channel(256)`
- All crates publish; event bridge in `lib.rs` converts to Tauri window events

### `GravaiError` (`error.rs`)
```rust
pub enum GravaiError {
    Audio(String), Transcription(String), Storage(String),
    Config(String), Model(String), Permission(String),
    Provider(String), Session(String), NotFound(String), Internal(String),
}
```
- Serializable — Tauri commands return `Result<T, String>` via `.to_string()`

### Other Modules
- `logging.rs` — Init at startup; ring buffer for recent logs queryable via `get_recent_logs()`
- `perf.rs` — CPU/memory performance monitoring
- `preflight.rs` — Permission health checks (microphone, screen recording, calendar)

## Usage Pattern
```rust
// In any Tauri command:
#[tauri::command]
async fn my_command(state: State<'_, Arc<AppState>>) -> Result<T, String> {
    let config = state.config.read().await;
    let session = state.session.read().await;
    state.event_bus.publish(GravaiEvent::SomeEvent { ... }).await;
    Ok(result)
}
```

---

Now answer the user's question about `gravai-core`: $ARGUMENTS
