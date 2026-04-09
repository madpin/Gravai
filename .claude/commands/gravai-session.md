---
description: Guide to session lifecycle — start, stop, pause, FSM, Tauri commands, background tasks
allowed-tools: Read, Glob, Grep
---

You are helping with the Gravai session lifecycle. The key file is `src-tauri/src/commands/session.rs`.

## Session FSM
```
Idle(0) ──start──→ Recording(1) ──pause──→ Paused(2)
                        ↑                      │
                        └────────resume─────────┘
                        │
                       stop
                        ↓
                    Stopped(3)
```
- Stored as `AtomicU8` in `Session` struct (SeqCst ordering)
- Published via `GravaiEvent::SessionStateChanged` on every transition

## `start_session()` — Full Sequence
1. **Guard**: `SESSION_STARTING` atomic + RAII `SessionStartGuard` prevents concurrent starts
2. **Config**: Load active profile/preset overrides (transcription engine/model, mic enabled, gain, etc.)
3. **ID**: `generate_session_id()` → `"YYYYMMDD_HHMMSS"` format
4. **Auto-name**: query calendar (EventKit) or Zoom window title for session title
5. **DB**: Create `SessionRecord` in SQLite early (utterances need the session_id FK)
6. **AppState**: Store `Arc<Session>` in `AppState.session`
7. **State**: Set to `Recording(1)`, publish `SessionStateChanged`
8. **Multi-track recorder**: Create `MultiTrackRecorder` for "mic" and/or "system" tracks
9. **Audio capture** (on OS thread, non-async):
   - CPAL mic stream → two `std::sync::mpsc` channels: HQ (raw) + LQ (for resampling)
   - ScreenCaptureKit system audio → same HQ/LQ pattern
10. **Resampler tasks** (`tokio::spawn`): HQ → WAV writer, LQ → 16kHz mono → pipeline channel
11. **VAD pipeline tasks** (one per source, `tokio::spawn`):
    - Receive 16kHz mono `AudioChunk`
    - VAD decision (WebRTC or Silero)
    - Accumulate speech, transcribe on pause detection
    - Optional: diarization → speaker labels
    - Optional: echo suppression → filter repeats
    - Optional: sentiment → emotion labels
    - Insert `UtteranceRecord` in DB
    - Publish `TranscriptUpdated` event
12. **Background task handles** stored in `Session` for cleanup on stop
13. Return session info to frontend

## `stop_session()`
1. Signal CPAL thread to stop via `CAPTURE_STOP` AtomicBool
2. `finalize()` all `TrackWriter` instances (writes WAV header with correct sample count)
3. Abort all stored background task `JoinHandle`s
4. Process remaining VAD buffers (flush final utterance)
5. Update DB: `ended_at`, `duration_seconds`, `state = "stopped"`
6. Publish `SessionStateChanged { state: Stopped }`
7. Return final session info

## `pause_session()` / `resume_session()`
- Atomic state update only: `Recording ↔ Paused`
- Audio capture continues (hardware doesn't pause) — utterances are not written during `Paused`
- Publish `SessionStateChanged` event

## Data Emitted During Recording
- `GravaiEvent::VolumeLevel { source, level }` — every audio chunk (for VU meters)
- `GravaiEvent::TranscriptUpdated { session_id, utterance }` — per transcribed utterance
- `GravaiEvent::SessionStateChanged` — on every FSM transition

## Frontend Integration
```typescript
// Start
const session = await invoke("start_session");

// Listen for real-time transcript
await listen("gravai:transcript", (e) => {
    liveUtterances.update(u => [...u, e.payload]);
});

// Listen for volume
await listen("gravai:volume", (e) => updateVuMeter(e.payload));

// Stop
await invoke("stop_session");
```

## Silence Monitor
- Background task in `lib.rs`, runs every 2s during recording
- If no `VolumeLevel` event in >10s → emit warning alert to frontend
- Detects frozen audio hardware or capture failures

---

Now answer the user's question about the Gravai session lifecycle: $ARGUMENTS
