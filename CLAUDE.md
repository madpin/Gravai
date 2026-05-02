# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

### Development
```bash
make dev              # Run with hot-reload (Rust + Vite)
make run              # Build and run debug app
make run-release      # Build release bundle and open the app
```

### Quality Checks
```bash
make check            # fmt + clippy + tests + typecheck (run before committing)
make lint             # clippy + fmt check + svelte-check
make test             # cargo test --workspace --lib
make typecheck        # pnpm typecheck (Svelte/TS)
make fmt              # Auto-format Rust code
```

### Build & Release
```bash
make bundle           # Build .app + .dmg
make version V=1.2.3  # Set specific version (updates all 3 version files)
make clean            # Remove build artifacts
make clean-data       # Remove user data (~/.gravai/)
```

## Architecture

Gravai is a privacy-first macOS app for audio capture, transcription, and AI meeting intelligence. All processing is on-device (Whisper + mistral.rs for local GGUF inference); no audio is sent to the cloud by default.

**Stack:** Rust (9 crates) + Tauri v2 + Svelte 5. Targets macOS 13+ / Apple Silicon.

### Cargo Workspace (`crates/`)

| Crate | Responsibility |
|-------|---------------|
| `gravai-core` | `AppState` (Arc+RwLock), `EventBus` (typed `GravaiEvent`), Session FSM (Idle→Recording→Paused→Stopped), logging (ring buffer+file), preflight checks |
| `gravai-audio` | `AudioCaptureManager`, resampler (48kHz→16kHz), multi-track WAV recorder, VAD (WebRTC/Silero), echo suppression, silence detection |
| `gravai-transcription` | `TranscriptionProvider` trait + Whisper engine implementation |
| `gravai-intelligence` | LLM client, summarization, diarization, embeddings, RAG chat, Minijinja prompt templates |
| `gravai-config` | Versioned JSON config at `~/.gravai/config.json`, presets, profiles, shortcuts, automations |
| `gravai-storage` | SQLite + FTS5 + vector embeddings; session CRUD, full-text search |
| `gravai-models` | Model downloader (Whisper, Silero VAD from HuggingFace → `~/.gravai/models/`) |
| `gravai-export` | Markdown, PDF, Obsidian, Notion exporters |
| `gravai-meeting` | Meeting detection (process polling), calendar integration (osascript) |

### Tauri App (`src-tauri/src/`)

- `lib.rs` — 47 Tauri commands, event bridge (`GravaiEvent` → frontend `gravai:*` events), system tray, automations engine
- `commands/` — Thin handlers wrapping core crate logic (audio, session, models, search, intelligence, storage, export, config, tools, knowledge)

### Frontend (`src-frontend/`)

Svelte 5 + TypeScript. Pages: Recording, Archive, Chat, Presets, Profiles, Knowledge, Storage, Settings. Frontend communicates exclusively via Tauri `invoke`/`listen` — no direct Rust access.

### Recording Pipeline

```
Mic (cpal) ──┐
             ├──→ Multi-track WAV (48kHz, 24-bit, ~/.gravai/sessions/{id}/)
Sys (SCK) ───┘
      │
      ├──→ Resampler (48kHz → 16kHz mono, per track)
      ├──→ VAD (utterance boundary detection)
      ├──→ Whisper transcription (real-time, <3s latency)
      └──→ SQLite (utterances + speaker labels)
```

## Key Patterns

- **AppState**: Central `Arc<RwLock<State>>` shared across all Tauri commands and background threads
- **EventBus**: Typed `GravaiEvent` enum for loosely-coupled communication between crates
- **Provider Traits**: Audio capture, transcription, VAD, and LLM are all behind traits — swap implementations without changing callers
- **Tauri Commands**: Always thin; logic lives in the relevant crate, not in `src-tauri/`
- **Configuration**: Versioned JSON auto-migrates on load; presets bundle capture settings, profiles bundle transcription+VAD+LLM settings

## Data & Config Locations

- User data: `~/.gravai/`
- Models: `~/.gravai/models/`
- Sessions: `~/.gravai/sessions/{id}/`
- Config: `~/.gravai/config.json`
- Logs: `~/.gravai/gravai.log`

## Versioning

Version must be kept in sync across three files: `Cargo.toml` (workspace), `Cargo.lock`, and `src-tauri/tauri.conf.json`. Use `make version V=X.Y.Z` to update all three atomically.
