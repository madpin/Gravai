---
description: Full Gravai architecture overview — stack, crates, data flows, and key patterns
allowed-tools: Read, Glob, Grep
---

You are helping with the Gravai codebase. Provide a concise, accurate overview of the requested architectural topic.

## Stack
- **Backend**: Rust workspace (9 crates) + Tauri v2
- **Frontend**: Svelte 5 + TypeScript
- **Target**: macOS 13+, Apple Silicon
- **Storage**: SQLite + FTS5 + vector embeddings at `~/.gravai/`
- **AI**: On-device only — Whisper (ONNX) for transcription, Ollama/OpenAI-compatible LLM for intelligence

## Cargo Workspace (`crates/`)

| Crate | Responsibility |
|-------|----------------|
| `gravai-core` | `AppState`, `EventBus`/`GravaiEvent`, Session FSM, logging, preflight |
| `gravai-audio` | `AudioCaptureManager`, resampler (48kHz→16kHz), VAD, multi-track WAV, echo suppression |
| `gravai-transcription` | `TranscriptionProvider` trait + Whisper ONNX engine |
| `gravai-intelligence` | LLM client, summarization, diarization, sentiment, embeddings, RAG chat, correction |
| `gravai-config` | Versioned JSON config at `~/.gravai/config.json`, presets, profiles, automations |
| `gravai-storage` | SQLite CRUD, FTS5 search, vector embeddings, sessions, utterances, conversations, knowledge |
| `gravai-models` | Whisper + Silero VAD model downloader (HuggingFace → `~/.gravai/models/`) |
| `gravai-export` | Markdown, PDF, Obsidian, Notion exporters |
| `gravai-meeting` | Process-polling meeting detection, calendar integration (osascript) |

## Tauri Layer (`src-tauri/src/`)
- `lib.rs` — App init, event bridge (`GravaiEvent` → `"gravai:*"` window events), system tray, automations engine
- `commands/` — Thin Tauri command handlers: `session.rs`, `audio.rs`, `intelligence.rs`, `storage.rs`, `search.rs`, `export.rs`, `models.rs`, `config_extras.rs`, `knowledge.rs`, `tools.rs`
- **Rule**: Commands are always thin — logic lives in crates, not in `src-tauri/`

## Frontend (`src-frontend/src/`)
- Pages: `Recording`, `Archive`, `Chat`, `Presets`, `Profiles`, `Knowledge`, `Models`, `Shortcuts`, `Automations`, `Storage`, `Settings`
- State: Svelte writable stores in `lib/store.ts`
- API: `lib/tauri.ts` wraps `window.__TAURI__.core.invoke()` and `.event.listen()`
- Communication: **only** via Tauri `invoke`/`listen` — no direct Rust access from frontend

## Recording Pipeline
```
Mic (CPAL) ──┐
             ├──→ Multi-track WAV (48kHz 24-bit, ~/.gravai/sessions/{id}/)
Sys (SCK) ───┘
      │
      ├──→ Resampler (48kHz → 16kHz mono, per track)
      ├──→ VAD (WebRTC or Silero ONNX — utterance boundary detection)
      ├──→ Whisper transcription (real-time, <3s latency target)
      ├──→ Optional: diarization, sentiment, echo suppression
      └──→ SQLite (utterances + speaker labels) + event bus
```

## Key Patterns
- **AppState**: `Arc<RwLock<State>>` shared across Tauri commands and background threads
- **EventBus**: typed `GravaiEvent` enum, broadcast channel (256-capacity), loosely-coupled
- **Provider Traits**: audio capture, transcription, VAD, LLM — swap implementations freely
- **Tauri Commands**: thin; `Result<T, String>` (GravaiError serialized as string)
- **Session FSM**: atomic state `Idle(0)→Recording(1)→Paused(2)→Stopped(3)` (SeqCst)
- **Config**: versioned JSON, deep-merge patches, auto-migrates on load
- **File safety**: atomic temp→rename for model downloads; explicit WAV `finalize()` for correct headers

## Data Locations
- User data: `~/.gravai/` (or `~/.gravai-dev/` in debug builds)
- Models: `~/.gravai/models/`
- Sessions: `~/.gravai/sessions/{id}/`
- Config: `~/.gravai/config.json`
- Logs: `~/.gravai/gravai.log`

## Versioning
Three files must stay in sync: `Cargo.toml` (workspace), `Cargo.lock`, `src-tauri/tauri.conf.json`.
Use `make version V=X.Y.Z` to update all atomically.

---

Now answer the user's question about Gravai's architecture: $ARGUMENTS
