# iOS Companion App — Architecture Document

## Overview

The Gravai iOS companion app provides mobile meeting recording and transcription,
syncing data with the macOS app via CloudKit.

## Architecture

```
┌─────────────────────────────────────┐
│         Gravai iOS App              │
│         (SwiftUI + Swift)           │
│                                     │
│  ┌───────────┐  ┌────────────────┐  │
│  │ Audio     │  │ Transcription  │  │
│  │ AVAudio   │──│ Parakeet/      │  │
│  │ Session   │  │ CoreML Whisper │  │
│  └───────────┘  └────────────────┘  │
│         │              │            │
│         ▼              ▼            │
│  ┌──────────────────────────────┐   │
│  │     Local Storage (SQLite)   │   │
│  └──────────────────────────────┘   │
│         │                           │
│         ▼                           │
│  ┌──────────────────────────────┐   │
│  │     CloudKit Sync Engine     │   │
│  └──────────────────────────────┘   │
└─────────────────────────────────────┘
         │
    iCloud (encrypted)
         │
┌─────────────────────────────────────┐
│     Gravai macOS (Tauri + Rust)     │
│     CloudKit Sync Consumer          │
└─────────────────────────────────────┘
```

## Technology Stack

| Component | Technology |
|---|---|
| UI | SwiftUI (iOS 17+) |
| Audio Capture | AVAudioSession (mic only; no system audio on iOS) |
| Transcription | Parakeet (Apple CoreML, battery-efficient) or whisper.cpp via CoreML |
| Local Storage | SQLite (same schema as macOS, shared via CloudKit) |
| Sync | CloudKit (CKRecord, encrypted private database) |
| Export Formats | Shared with macOS: Markdown, PDF, Obsidian, Notion |

## Data Model (shared with macOS)

- `Session` — id, title, started_at, ended_at, duration, meeting_app, state
- `Utterance` — id, session_id, timestamp, source, speaker, text, confidence
- `SessionSummary` — id, session_id, tldr, key_decisions, action_items, open_questions

## Sync Strategy

1. Sessions + utterances synced via CloudKit `CKRecord` in the private database
2. Audio files synced as `CKAsset` (large binary blobs)
3. Conflict resolution: last-write-wins for metadata; append-only for utterances
4. Encryption: CloudKit encrypts at rest; no additional encryption layer needed for v1

## iOS-Specific Constraints

- **No system audio capture** — iOS only allows microphone input
- **Background recording** — requires `audio` background mode entitlement
- **Battery** — Parakeet model optimized for Apple Neural Engine; Whisper fallback for accuracy
- **Storage** — on-device models cached in app container; ~200MB for Parakeet, ~1.5GB for Whisper medium
- **Privacy** — microphone permission required; no data leaves device except CloudKit sync

## Build Plan

1. **Phase 1**: SwiftUI shell, AVAudioSession recording, local SQLite
2. **Phase 2**: Parakeet transcription via CoreML
3. **Phase 3**: CloudKit sync with macOS
4. **Phase 4**: Summary generation (on-device or delegated to macOS via CloudKit)
5. **Phase 5**: App Store submission

## Shared Code

The iOS app does NOT share Rust code with macOS (separate native codebase).
Shared via compatible formats:
- SQLite schema (identical migrations)
- JSON export format
- Markdown/PDF templates
- Notion API client (Swift reimplementation)
