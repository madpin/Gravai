---
description: Guide to Gravai event system — GravaiEvent enum, EventBus, Tauri event bridge, frontend listeners
allowed-tools: Read, Glob, Grep
---

You are helping with the Gravai event system. Events flow from Rust crates → EventBus → lib.rs bridge → Tauri window events → Svelte frontend.

## `GravaiEvent` Enum (gravai-core)
All internal events use this typed enum — no generic `Value` passing.

```rust
pub enum GravaiEvent {
    SessionStateChanged {
        session_id: String,
        state: SessionState,   // Idle/Recording/Paused/Stopped
    },
    TranscriptUpdated {
        session_id: String,
        utterance: UtteranceRecord,  // full utterance with text, speaker, timestamps
    },
    VolumeLevel {
        source: String,   // "mic" or "system"
        level: f32,       // 0.0–1.0 RMS amplitude
    },
    MeetingDetected {
        app_name: String,          // "Zoom", "Microsoft Teams", etc.
        title: Option<String>,     // meeting title from calendar/window
    },
    MeetingEnded {
        app_name: String,
    },
    PresetActivated {
        preset_id: String,
    },
    ProfileSwitched {
        profile_id: String,
    },
    DownloadProgress {
        model_id: String,
        downloaded: u64,  // bytes
        total: u64,       // bytes
    },
    Error {
        message: String,
    },
    TranscriptCorrected {
        utterance_id: String,
        corrected_text: String,
    },
}
```

## `EventBus` (gravai-core)
```rust
pub struct EventBus {
    sender: broadcast::Sender<GravaiEvent>,
}

impl EventBus {
    pub async fn publish(&self, event: GravaiEvent)
    pub fn subscribe(&self) -> broadcast::Receiver<GravaiEvent>
}
```
- Tokio `broadcast::channel(256)` — all subscribers receive all events
- Any crate that has `&EventBus` can publish or subscribe
- `AppState.event_bus` is the single instance shared across the app

## Event Bridge (`src-tauri/src/lib.rs`)
Converts `GravaiEvent` → Tauri window events. Runs in a `tokio::spawn` loop at startup.

| `GravaiEvent` | Tauri Event String | Notes |
|---------------|-------------------|-------|
| `SessionStateChanged` | `"gravai:session"` | Also updates tray menu enable/disable |
| `TranscriptUpdated` | `"gravai:transcript"` | Payload: serialized `UtteranceRecord` |
| `VolumeLevel` | `"gravai:volume"` | Also updates `last_audio_time` for silence monitor |
| `MeetingDetected` | `"gravai:meeting"` | Also triggers automation evaluation |
| `MeetingEnded` | `"gravai:meeting-ended"` | |
| `PresetActivated` | `"gravai:preset"` | |
| `ProfileSwitched` | `"gravai:profile"` | |
| `DownloadProgress` | `"gravai:model-download"` | |
| `Error` | `"gravai:error"` | |
| `TranscriptCorrected` | `"gravai:correction"` | |

## Publishing Events (Rust)
```rust
// In any async context with access to AppState
state.event_bus.publish(GravaiEvent::TranscriptUpdated {
    session_id: session.id.clone(),
    utterance: record,
}).await;

// From a sync context (e.g., pipeline callback)
let bus = event_bus.clone();
tokio::spawn(async move {
    bus.publish(GravaiEvent::VolumeLevel { source: "mic".into(), level: rms }).await;
});
```

## Subscribing to Events (Rust)
```rust
// In lib.rs event bridge:
let mut rx = state.event_bus.subscribe();
loop {
    match rx.recv().await {
        Ok(GravaiEvent::TranscriptUpdated { session_id, utterance }) => {
            window.emit("gravai:transcript", &utterance).ok();
        }
        Ok(GravaiEvent::VolumeLevel { source, level }) => {
            // update last_audio_time for silence monitor
            window.emit("gravai:volume", json!({ "source": source, "level": level })).ok();
        }
        // ...
    }
}
```

## Listening in Frontend (Svelte)
```typescript
import { listen } from "$lib/tauri";

// In onMount or $effect:
const unlisten = await listen("gravai:transcript", (event) => {
    const utterance = event.payload as UtteranceRecord;
    liveUtterances.update(u => [...u, utterance]);
});

// Cleanup in onDestroy:
onDestroy(() => unlisten());
```

## Silence Monitor
A secondary consumer of `VolumeLevel` events in `lib.rs`:
- Records `last_audio_time` on every `VolumeLevel` event
- Background task runs every 2s during `Recording` state
- If `now - last_audio_time > 10s` → emits `"gravai:silence-warning"` to frontend

## Automation Trigger
`MeetingDetected` events are also handled synchronously in the bridge to evaluate automations:
1. Load all enabled `Automation` rules from config
2. Match `AutomationTrigger::MeetingDetected` or `MeetingAppDetected { app_name }`
3. Evaluate conditions (process running, day of week, session state)
4. Execute actions (start recording, switch profile/preset, show notification)

---

Now answer the user's question about the Gravai event system: $ARGUMENTS
