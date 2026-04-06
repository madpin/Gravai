# Gravai Development Plan

## Context

Gravai is a greenfield macOS-native Rust application that unifies multi-source audio capture with on-device AI meeting intelligence. The repo currently contains only `PRD.md`. This plan defines the full implementation roadmap, reusing proven patterns from the `ears-rust-api` reference codebase (`/Users/tpinto/code/ears-rust-api`).

**Key architectural decisions:**
- **UI Framework: Tauri v2** (Rust backend + web frontend via WKWebView) — best balance of UI complexity needs, macOS integration, and Rust reuse
- **Frontend: SolidJS or Svelte** — lightweight, reactive, good DX
- **Storage: SQLite** (via rusqlite) replacing ears' JSON file storage
- **Config: Versioned JSON** with migration layer (extends ears' TOML approach)
- **All ears patterns reused**: AppState, EventBus, Session FSM, audio capture, VAD, Whisper, LLM client, model downloader, preflight, logging

---

## Architecture: Cargo Workspace Layout

```
gravai/
  Cargo.toml                     # [workspace]
  crates/
    gravai-core/                 # AppState, EventBus (typed enum), Session FSM, error types, preflight, logging
    gravai-audio/                # cpal + SCK capture, resampler (48kHz→16kHz), multi-track WAV writer, mixer, encoder, VAD
    gravai-transcription/        # TranscriptionProvider trait + WhisperEngine impl
    gravai-intelligence/         # SummarizationProvider, DiarizationProvider, EmbeddingProvider, LLM client, chat/RAG
    gravai-config/               # Versioned config schema, profiles, presets, shortcuts, automations
    gravai-storage/              # SQLite schema + migrations, session CRUD, FTS5 + vector search
    gravai-models/               # Model downloader + registry (from ears)
    gravai-export/               # Markdown, PDF, Obsidian, Notion
    gravai-meeting/              # Meeting detection (process monitor), calendar integration
  src-tauri/                     # Tauri v2 glue: #[tauri::command] handlers, event bridge, tray
  src-frontend/                  # Web UI (SolidJS/Svelte)
```

### How ears patterns map to Gravai

| ears module | Gravai location | Change |
|---|---|---|
| `app_state.rs` | `gravai-core` | Same Arc\<AppState\>+RwLock, add profile/preset fields |
| `event_bus.rs` | `gravai-core` | Typed enum `GravaiEvent` instead of `serde_json::Value` |
| `session.rs` | `gravai-core::session` | Same AtomicU8 FSM, add multi-track + dual-rate pipeline |
| `audio/capture.rs` + `screencapturekit.rs` | `gravai-audio` | 48kHz stereo capture + 16kHz mono resampler branch |
| `audio/vad_*.rs` | `gravai-audio::vad` | Behind `VadProvider` trait |
| `transcription/whisper.rs` | `gravai-transcription` | Behind `TranscriptionProvider` trait |
| `analysis/*` | `gravai-intelligence` | Behind `SummarizationProvider` trait; LLM client reused directly |
| `sentiment/*` | `gravai-intelligence::sentiment` | Direct port, behind provider trait |
| `config.rs` | `gravai-config` | Versioned JSON + migration + profiles/presets/shortcuts |
| `storage.rs` | `gravai-storage` | SQLite replaces JSON files |
| `downloader.rs` | `gravai-models` | Same pattern, adapted paths (`~/.gravai/models/`) |
| `preflight.rs` | `gravai-core::preflight` | Same checks, extended |
| `routers/*` + `ws.rs` | **REMOVED** | No HTTP API; Tauri commands replace routes |
| `tray.rs` | Tauri system tray plugin | Native Tauri v2 tray |

---

## Phase 0: Foundation (Weeks 1–4)

**Goal**: Project infrastructure, workspace skeleton, CI, core abstractions.

### P0-1: Cargo workspace scaffolding
- Create all `crates/gravai-*/Cargo.toml` with dependencies
- Wire `src-tauri/Cargo.toml` to depend on workspace crates
- `cargo check --workspace` passes with stub `lib.rs` files

### P0-2: Tauri v2 app shell
- Initialize Tauri v2 + web frontend (SolidJS or Svelte)
- `tauri.conf.json`: identifier `com.gravai.app`, entitlements for mic + screen recording
- Minimal window with sidebar navigation (Recording / Archive / Settings)
- Activation policy: `Regular` (Dock icon, not Accessory like ears)
- **AC**: `cargo tauri dev` launches native macOS window with hot-reload

### P0-3: Core abstractions — AppState + EventBus
- Port `AppState` from ears `app_state.rs`, expand with `active_profile`, `active_preset` fields
- Replace ears' `serde_json::Value` event bus with typed `GravaiEvent` enum
- **AC**: `AppState::new()`, `EventBus::publish()`/`subscribe()` compile and pass unit tests

### P0-4: Error hierarchy
- `GravaiError` via `thiserror`: Audio, Transcription, Storage, Config, Model, Permission, Provider, Internal
- Transport-agnostic (no HTTP status mapping unlike ears' `AppError`)

### P0-5: Logging infrastructure
- Port `log_capture.rs` ring buffer from ears
- Tracing subscriber: stderr + file (`~/.gravai/gravai.log`) + ring buffer
- Structured fields: `session_id`, `source`, `phase`

### P0-6: CI pipeline (GitHub Actions)
- `cargo check/test/clippy/fmt --workspace`
- Frontend: `lint + typecheck + build`
- Tauri build on macOS Apple Silicon runner

### P0-7: Provider trait definitions
- Define all core engine traits (compile, no implementations yet):
  - `VadProvider` in `gravai-audio`
  - `TranscriptionProvider` in `gravai-transcription`
  - `SummarizationProvider` in `gravai-intelligence`
  - `DiarizationProvider` in `gravai-intelligence`
  - `EmbeddingProvider` in `gravai-intelligence`

### P0-8: SQLite storage foundation
- `rusqlite` with `bundled` feature
- Initial schema: `sessions`, `utterances`, `utterances_fts` (FTS5)
- Migration runner with `schema_version` table
- Data directory: `~/.gravai/`
- **AC**: `Database::open()` creates/migrates schema; CRUD for sessions + utterances works

---

## Phase 1: Alpha v0.1 — Audio Capture (Weeks 5–8)

**PRD features**: F1 (Audio Capture), F2 (Recording/Transcription Inputs), F12 subset (Settings shell)

### P1-1: Port audio capture from ears
- Port `AudioCaptureManager` from ears `audio/capture.rs` + `screencapturekit.rs`
- Key change: capture at device native rate (48kHz stereo) not 16kHz mono
- Retain ears' `mpsc::channel` + `AtomicBool` patterns

### P1-2: Dual-rate audio pipeline
- **Recording path**: 48kHz/24-bit stereo → WAV/CAF writer
- **Transcription path**: Resample to 16kHz mono via `rubato` crate → VAD + Whisper
- `AudioCaptureManager` produces two channel sets per source: `hq_tx` (48kHz) + `lq_tx` (16kHz)

### P1-3: Multi-track recorder
- `hound` crate for 24-bit PCM WAV writing
- Each source → own file, all time-aligned; plus mixed master track
- Files at `~/.gravai/sessions/{session_id}/`

### P1-4: Per-source volume and panning
- `gain: f32` (0.0–2.0) + `pan: f32` (-1.0 to 1.0) per source
- Applied before writing to tracks (per PRD: reflected in recorded output)

### P1-5: VU metering
- Port `rms_db()` from ears; compute at 10Hz per source
- Emit `GravaiEvent::VolumeLevel`; frontend renders animated VU meters

### P1-6: Session FSM
- Port `SessionState` + `AtomicU8` from ears `session.rs`
- States: Idle → Recording ↔ Paused → Stopped
- Start: create dir, open WAV writers, start capture
- Stop: flush writers, persist to SQLite

### P1-7: Port VAD from ears
- Port `vad_webrtc.rs` + `vad_silero.rs` behind `VadProvider` trait
- Operates on 16kHz mono stream

### P1-8: Recording UI
- Source list (mic devices + running apps), per-source toggles/sliders/VU meters
- Record/Pause/Stop buttons, session timer
- "Recording inputs" vs "Transcription inputs" sections (F2)
- Tauri commands: `list_audio_devices`, `list_running_apps`, `start_session`, etc.

### P1-9: Config system (F12 subset)
- Versioned JSON at `~/.gravai/config.json`; load with defaults, deep merge, save
- Basic Settings UI: audio, transcription, general categories
- Import/export stub

### P1-10: Model downloader
- Port `downloader.rs` from ears, adapt to `~/.gravai/models/`
- Progress via `GravaiEvent::DownloadProgress` (UI progress bar, not terminal)

### P1-11: Preflight checks
- Port `preflight.rs` from ears; check macOS version, permissions, models, devices

**Phase 1 AC**: Record multi-track WAV at 48kHz/24-bit with VU meters, per-source volume, config persistence. No virtual audio device.

---

## Phase 2: Alpha v0.2 — Transcription + Meeting Detection (Weeks 9–12)

**PRD features**: F3 (Transcription), F4 (Meeting Detection), provider abstraction

### P2-1: Whisper transcription engine
- Port `WhisperEngine` from ears `transcription/whisper.rs`
- Implement `TranscriptionProvider` trait; return `Vec<TranscriptionSegment>` with timestamps + confidence
- `tokio::spawn_blocking` for CPU-bound inference (same as ears)
- Port hallucination filtering

### P2-2: VAD-triggered transcription pipeline
- Port speech accumulation logic from ears' `session.rs` (`process_audio_loop`)
- Utterances written to SQLite as they arrive + emit `GravaiEvent::TranscriptUpdated`

### P2-3: Provider abstraction wiring
- Provider factory: config selects engine → `Box<dyn TranscriptionProvider>`
- Stub for future external HTTP provider

### P2-4: Live transcript UI
- Scrolling utterance list, timestamps, source labels, confidence highlighting
- Auto-scroll with snap-to-bottom toggle

### P2-5: Meeting detection (F4)
- Poll running processes for Zoom/Meet/Teams/Slack/FaceTime/Discord
- Browser window title scanning for web-based meetings
- Mic activation detection via CoreAudio
- UI: "Meeting detected — Record?" banner with confirm/dismiss/always-allow
- On meeting close while recording: alert user, do NOT auto-stop

### P2-6: Calendar integration (F4)
- Apple Calendar via EventKit FFI
- Auto-name sessions with meeting title + participants
- Configurable lead time

### P2-7: Echo suppression
- Port `EchoSuppressor` from ears (Sorensen-Dice similarity via `strsim`)

**Phase 2 AC**: Live transcription <3s latency, fully offline. Meeting detection for Zoom/Meet. Calendar auto-naming. Utterances in SQLite.

---

## Phase 3: Beta v0.5 — Intelligence + Presets (Weeks 13–20)

**PRD features**: F5 (Diarization), F6 (AI Summaries), F7 (Presets), F12 (full profiles/shortcuts/automations)

### P3-1: Speaker diarization (F5)
- ONNX pyannote model via `ort` crate (same ORT setup as ears' sentiment)
- Merge diarization segments with transcription by time alignment
- UI: colored speaker labels

### P3-2: Speaker identification (F5)
- Extract speaker embeddings; store encrypted in macOS Keychain (`security-framework`)
- Auto-recognize known speakers; UI for name assignment

### P3-3: AI summaries (F6)
- Port LLM client from ears `analysis/llm_client.rs`
- `SummarizationProvider` trait: `LocalLlmProvider` (Ollama) + `ByokProvider` (OpenAI/Anthropic)
- Structured output: TL;DR, Key Decisions, Action Items (per owner), Open Questions
- Minijinja prompt templates (same as ears)

### P3-4: Summary UI
- Editable summary panel below transcript; regenerate button; email draft composer

### P3-5: Capture presets (F7)
- Preset = sources + volumes + format + output folder; built-in templates
- One-click activation < 500ms; import/export JSON

### P3-6: Profiles (F12)
- Profile bundles: preset ref + transcription engine + diarization toggle + summary provider + shortcuts + automations
- Switch from UI, menu bar, or global shortcut

### P3-7: Keyboard shortcuts (F12)
- Shortcut map: `ActionId → KeySequence`; in-app + global (Tauri plugin)
- Shortcuts editor UI with conflict detection; persist + export/import

### P3-8: Automations (F12)
- Trigger + condition(s) + action(s); templates for common flows
- Execution log; enable/disable toggle

### P3-9: Audio export formats
- AAC-LC / ALAC in M4A via CoreAudio FFI (`AudioToolbox`)
- AIFF via hound; CAF via CoreAudio

**Phase 3 AC**: Diarization ≥85% on 2-speaker calls. Summary within 60s. BYOK works. Presets switch <500ms. Shortcuts remappable. Automations trigger correctly.

---

## Phase 4: Beta v0.8 — Search + Export (Weeks 21–28)

**PRD features**: F8 (Semantic Search), F9 (Ask Gravai), F10 (Export)

### P4-1: Embedding generation
- `all-MiniLM-L6-v2` via ORT; 384-dim vectors stored in SQLite

### P4-2: Full-text search
- SQLite FTS5; filters: date, participants, app, duration

### P4-3: Semantic search
- Embed query → cosine similarity; hybrid ranking (BM25 + embedding)

### P4-4: Archive UI
- Session list with search bar, filter sidebar, audio/transcript sync playback
- Inline transcript editor with version tracking

### P4-5: Ask Gravai chat (F9)
- RAG pipeline: embed query → retrieve top-K utterances → LLM with grounding prompt
- Per-session and cross-archive modes; citations with timestamps

### P4-6: Chat UI
- Message input, conversation history, clickable citation links

### P4-7–10: Export integrations (F10)
- **Markdown**: template-based, YAML frontmatter, selective sections
- **PDF**: `genpdf` crate, A4 ≥11pt, mobile-readable
- **Obsidian**: Markdown + YAML frontmatter auto-pushed to vault folder
- **Notion**: API v1 via reqwest, API key (no OAuth)

### P4-11: Auto-export on session end
- Per-integration toggle via automation system

**Phase 4 AC**: Semantic search top-3 accuracy ≥80%. Audio sync ±1s. Ask Gravai <5s latency. All exports well-formatted. 10K+ sessions performant.

---

## Phase 5: v1.0 GA (Weeks 29–36)

**PRD features**: iOS companion planning, F11 (Silence Trim), polish, App Store

### P5-1: Silence trimming (F11)
- RMS-based detection; preview in UI; non-destructive (original preserved)

### P5-2: iOS companion planning
- Architecture doc: separate SwiftUI app, Parakeet model for transcription, CloudKit sync

### P5-3: App Store preparation
- Sandbox testing, code signing, entitlements, privacy manifest

### P5-4: Performance optimization
- Whisper benchmarks on M1/M2/M3; memory audit (<2GB RSS); SQLite query tuning

### P5-5: Accessibility
- VoiceOver, keyboard navigation, Reduce Motion, Dynamic Type

### P5-6: Polish
- Onboarding wizard, tooltip/help text, error states, animations

---

## Cross-Cutting Concerns

### Testing Strategy

| Layer | Approach |
|---|---|
| Unit | `#[cfg(test)]` per crate; mock providers via trait objects |
| Integration | Workspace `tests/`; real SQLite; session lifecycle end-to-end |
| Audio | Pre-recorded WAV fixtures; resampler, VAD, transcription accuracy |
| UI | Playwright/Tauri test driver; component snapshots |
| Manual QA | Matrix: M1/M3, Sonoma/Sequoia, Zoom/Meet/Teams |

### Security
- Voice fingerprints + API keys: macOS Keychain (`security-framework`)
- No network by default; only for BYOK + Notion (user-initiated)
- Privacy manifest for App Store

### Concurrency (same as ears)
- `tokio` runtime; `spawn_blocking` for CPU inference
- Audio callbacks → `mpsc::channel` → tokio tasks
- `Arc<AppState>` + `RwLock` for shared state

---

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| SCK per-app capture doesn't work for all apps | Medium | High | Fallback to full system audio; test top 20 apps early |
| Whisper large-v3 too slow on M1 for real-time | Medium | High | Default to distil-whisper; model size toggle; benchmark in Phase 2 |
| App Store sandbox blocks SCK/audio | Medium | High | Submit test build early (Phase 3); prepare notarized DMG as parallel path |
| pyannote diarization degrades on compressed Zoom audio | Medium | Medium | Test with real recordings early; manual speaker assignment fallback |
| Ollama not installed by user | High | Low | Clear error + install link; consider bundling `llama-cpp-rs` |
| Memory pressure from concurrent models | Medium | High | Load on-demand, unload when idle; monitor RSS; memory budget per model |
| CoreAudio AAC/ALAC FFI fragility | Low | Medium | `audiotoolbox-sys`; fallback WAV-only |

---

## Critical Files to Port from ears

| ears file | Purpose | Target crate |
|---|---|---|
| `src/audio/capture.rs` | AudioCaptureManager (cpal + SCK, channels, AtomicBool) | `gravai-audio` |
| `src/audio/screencapturekit.rs` | ScreenCaptureKit system audio | `gravai-audio` |
| `src/audio/vad_webrtc.rs` | WebRTC VAD | `gravai-audio::vad` |
| `src/audio/vad_silero.rs` | Silero VAD via ORT | `gravai-audio::vad` |
| `src/session.rs` | Session FSM + audio processing orchestration | `gravai-core::session` |
| `src/transcription/whisper.rs` | WhisperEngine + hallucination filtering | `gravai-transcription` |
| `src/analysis/llm_client.rs` | OpenAI-compatible LLM client | `gravai-intelligence` |
| `src/analysis/conversation.rs` | LLM conversation analysis + prompts | `gravai-intelligence` |
| `src/config.rs` | Config structs + deep merge | `gravai-config` |
| `src/app_state.rs` | Arc\<AppState\> + RwLock pattern | `gravai-core` |
| `src/event_bus.rs` | Tokio broadcast EventBus | `gravai-core` |
| `src/downloader.rs` | Model download with progress | `gravai-models` |
| `src/preflight.rs` | Startup health checks | `gravai-core::preflight` |
| `src/log_capture.rs` | Ring buffer logging layer | `gravai-core::logging` |

---

## Verification

After each phase:
1. `cargo check --workspace` and `cargo test --workspace` pass
2. `cargo tauri dev` launches and relevant features are accessible in UI
3. Manual QA against phase acceptance criteria (listed per phase above)
4. CI pipeline green on all checks
