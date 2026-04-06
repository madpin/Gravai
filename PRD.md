# Product Requirements Document: Gravai — Audio Capture & AI Meeting Intelligence

**Version:** 1.0  
**Status:** Draft  
**Author:** Thiago M. Pinto  
**Date:** April 2026  

***

## Executive Summary

Gravai is a macOS-native (with iOS companion) application that unifies professional multi-source **audio capture** with on-device AI meeting intelligence. It captures, mixes, and layers system and microphone audio **inside the app** for recording and transcription — without exposing a virtual microphone or feeding mixed audio to other apps — and pairs that with a privacy-first meeting recorder that transcribes, summarizes, and surfaces actionable insights without sending data to external servers or joining calls as a bot.

The core proposition: one app to capture, configure, and understand your audio, whether you're a podcaster, streamer archiving sessions, remote software engineer, or anyone who sits in back-to-back calls and wants their meetings to work *for* them.

***

## Problem Statement

Today, users who want both high-quality multi-source capture *and* AI-powered meeting notes are forced to run two separate tools (e.g., Dipper + Granola), configure them independently, and stitch outputs together manually. Meanwhile, most AI meeting recorders either:

- Send data to the cloud (privacy risk)
- Join calls as an intrusive bot (awkward, unreliable)
- Lack audio quality controls (flat, mixed recordings)
- Offer no per-source input control for podcasters, streamers, or DAW-heavy workflows

Gravai collapses this dual-tool workflow into a single macOS application with a **full primary UI** (windows, archive, editor, settings) — optionally complemented by menu bar shortcuts — that requires zero configuration for the common case and deep configuration for power users.

***

## Target Users

| Persona | Primary Need |
|---|---|
| **Remote SWE / Knowledge Worker** | Transcribe and summarize back-to-back calls, action items, zero bots |
| **Podcaster / Content Creator** | Multi-source recording, per-app volume control, preset switching |
| **Streamer** | Capture mic + game + chat apps for local archive and transcripts (no virtual mic into OBS) |
| **Interviewer / Researcher** | Speaker diarization, searchable archive, semantic recall |
| **Privacy-Conscious Professional** | 100% on-device AI, no cloud upload, no account required |

***

## Goals & Non-Goals

### Goals
- Capture and mix Mac system and mic audio inside Gravai with studio-quality fidelity (no audio output device for other apps)
- Transcribe meetings locally using on-device ML (Whisper / Apple Neural Engine)
- Auto-detect and record meetings without manual triggers
- Generate AI summaries and action items post-meeting
- Provide a searchable, semantic archive of all recordings and transcripts
- Export to Markdown, PDF, Obsidian, Notion, and Slack
- Expose **every** user-relevant knob in Settings (or equivalent) — no “magic” behavior without an discoverable toggle, default, or explanation; advanced options may be grouped or behind “Advanced” but remain accessible
- Support **named profiles** so users can switch whole configuration bundles (work vs. podcast vs. minimal) in one action
- Make **keyboard shortcuts** and **automations** first-class: easy to discover, edit, export/import, and fully remappable where technically possible on the platform
- Keep the core **extensible by design**: transcription, summarization, and other “engines” plug in behind stable interfaces so external services or alternate models can be added without rewriting the app

### Non-Goals
- Cloud-based transcription or storage (explicitly out of scope for v1)
- Windows support (post-roadmap)
- Video recording (audio-only focus for v1)
- Acting as a meeting bot that joins calls on behalf of the user
- Virtual microphone, loopback output device, or any feature that sends Gravai’s audio into other apps as their input

***

## Feature Requirements

### F1 — Audio Capture Engine

**Priority: P0 (Must Have)**

The audio capture engine is the foundation of Gravai. It taps into macOS CoreAudio to capture audio from any running app for **internal** recording and processing — without requiring end users to install a separate loopback driver for basic capture, and **without** presenting a virtual microphone to the rest of the system.

- Capture audio from any running application individually (browser tabs, Zoom, Spotify, DAWs, FaceTime, WhatsApp, Discord)
- Capture mic input simultaneously and independently from system audio
- Record in **48kHz, 24-bit stereo** (PCM baseline) with user-selectable export using **macOS-native codecs only** (Core Audio / AVFoundation — no bundled LAME or third-party encoders required):
  - **Lossless archival:** WAV, AIFF, **CAF** (Core Audio Format; flexible container for PCM or compressed streams)
  - **Conversation-friendly (sharing, email, Slack, playback on any device):** **AAC-LC in M4A** (default compact choice), optional **Apple Lossless (ALAC) in M4A** for smaller-than-WAV lossless archives
- Per-source volume and stereo panning controls (independent per app or device)
- Multi-track recording — save each audio source to its own isolated track for post-processing
- Automatic audio level metering per source (VU meter) visible in the main recording UI (menu bar shortcuts optional)
- Support simultaneous capture of up to 8 audio sources in parallel

**Acceptance Criteria:**
- Audio recorded from any app produces a clean, artifact-free **WAV** (or equivalent PCM in **AIFF / CAF**) at **48kHz / 24-bit** stereo
- **AAC (M4A)** export uses system encoders and yields listenable, artifact-free speech at sensible default bitrates (e.g., 128–256 kbps stereo for mixed calls; configurable), without requiring non-Apple codecs
- **ALAC (M4A)** export matches PCM quality at reduced file size vs. uncompressed formats
- Per-source volume adjustments are reflected in recorded output, not just monitoring
- Multi-track output produces one file per source, time-aligned and same duration

***

### F2 — Recording & Transcription Input Selection

**Priority: P0 (Must Have)**

Gravai does **not** install or expose a virtual microphone, loopback **output** device, or any mechanism that feeds Gravai’s mix into Zoom, OBS, Discord, or other apps as an input. All capture stays inside Gravai for recording, transcription, and AI features.

Users **filter, choose, and configure** what audio is used where:

- **Recording inputs:** which apps, devices, and system-audio scopes are included in the recorded session (multi-track and mixed master as defined in F1)
- **Transcription inputs:** which sources (or a defined submix) are sent to the on-device speech pipeline — may match the full recording mix or a narrower selection (e.g. meeting app + mic only) to improve accuracy or reduce noise
- Per-source include/exclude for each path where it matters; clear UI that shows “what is being recorded” vs “what is being transcribed”
- Monitoring/level checks via the app’s own meters and headphone monitoring of **captured** signals — not as a system-wide virtual output to third-party apps

**Acceptance Criteria:**
- No virtual input device from Gravai appears in System Settings for use by other applications
- User can configure recording and transcription inputs independently within supported source types
- Changing transcription input does not silently change what is written to disk unless the user explicitly links the two

***

### F3 — On-Device Transcription

**Priority: P0 (Must Have)**

For **v1**, transcription runs entirely on-device using Apple's Neural Engine and Whisper-based models — consistent with Non-Goals (no Gravai-mandated cloud transcription). The **implementation** follows the pluggable transcription provider model (F12 extensibility) so a future milestone can add user-opt-in external engines (e.g. self-hosted or BYO API) without rewriting the session pipeline.

- Whisper (large-v3 or distil-Whisper) on macOS Apple Silicon (M1 and later)
- Parakeet model on iOS for fast, battery-efficient on-device transcription
- Real-time transcription: text appears live as speech occurs, with a configurable latency target of <3 seconds
- Multi-language support: auto-detect language per session; support EN, PT, ES, FR, DE, PL, JA initially
- Punctuation and paragraph inference (not raw word dumps)
- Confidence scores per word; low-confidence words visually flagged in the editor

**Acceptance Criteria:**
- Transcription runs fully offline with no network access required
- WER (Word Error Rate) ≤ 8% on English clean speech using Whisper large-v3
- Transcription begins within 2 seconds of audio capture starting
- App remains fully functional (audio capture uninterrupted) during transcription processing

***

### F4 — Automatic Meeting Detection

**Priority: P1 (Should Have)**

Gravai should know when a meeting is happening and act accordingly — starting a recording session, attaching it to a calendar event, and offering a clear path to stop when the call likely ends (without silently cutting the recording short).

- Monitor active window titles and running process list for known meeting apps: Zoom, Google Meet, Microsoft Teams, Slack Huddles, FaceTime, WebEx, Discord Stage
- Detect microphone activation events as a fallback trigger for unknown meeting tools
- Calendar integration (Google Calendar, Apple Calendar, Outlook) to pre-name recording sessions with meeting title and participants
- Configurable lead time: start recording X seconds before detected meeting start
- When the meeting window closes or the mic goes idle for >30 seconds, **do not** auto-stop recording; alert the user (e.g. notification or in-app banner) and ask for explicit confirmation to stop. Default: keep recording until the user confirms stop or dismisses (continuing the session).
- Popup prompt: "Meeting detected — record?" with one-click confirm or permanent allow list per app

**Acceptance Criteria:**
- Meeting detection fires correctly for Zoom and Google Meet 100% of the time when the window is active
- Calendar-linked recordings are auto-named with the meeting title within 5 seconds of session start
- False positive rate for non-meeting mic activations < 5% in standard office use
- On meeting-window close or mic idle >30s while recording, the user receives a visible alert and must confirm before the session stops; recording does not end without that confirmation

***

### F5 — Speaker Diarization & Identification

**Priority: P1 (Should Have)**

Knowing *who* said *what* transforms a raw transcript into a searchable, attributed record.

- Detect distinct speaker voices and segment transcript by speaker (Speaker 1, Speaker 2, etc.)
- Allow user to assign names to speaker labels ("Speaker 1" → "Thiago", "Speaker 2" → "John")
- Persist voice fingerprints on-device (encrypted local store) and auto-recognize the same person in future meetings
- Diarization model runs locally (pyannote.audio-based or CoreML equivalent)
- Handle remote participants: attribute voice segments to participants even over compressed Zoom/Meet audio
- Support up to 10 distinct speakers per session

**Acceptance Criteria:**
- Speaker segmentation correctly attributes turns with ≥ 85% accuracy in a 2-speaker call
- Named speakers from a previous meeting are auto-recognized in a new meeting within 15 seconds of joining
- Voice fingerprints are stored encrypted at rest in the macOS Keychain or app sandbox

***

### F6 — AI Summaries & Action Items

**Priority: P1 (Should Have)**

Post-meeting intelligence that turns a 60-minute conversation into a 2-minute brief.

- Auto-generate a structured meeting summary after each session ends:
  - **TL;DR** (2–3 sentences)
  - **Key Decisions** (bulleted)
  - **Action Items** (per owner, if names are known)
  - **Open Questions** (unresolved topics)
- Summaries generated by a local LLM (default: Llama 3 via Ollama) or user-provided API key (OpenAI, Anthropic, Mistral — BYOK)
- User can edit, approve, or regenerate summaries inline
- Summary is embedded into the transcript view and included in all exports
- Post-meeting email draft: optionally compose a follow-up email with action items pre-filled, opened in Mail.app or Gmail

**Acceptance Criteria:**
- Summary is ready within 60 seconds of meeting end for a 1-hour session on M-series Mac
- Action items are correctly extracted in ≥ 80% of cases where they were explicitly stated in the meeting
- BYOK works with OpenAI gpt-4o-mini and Anthropic Claude Haiku as verified integrations

***

### F7 — Capture Presets & Boards

**Priority: P1 (Should Have)**

Inspired by Dipper's "boards" concept, Gravai lets users save and recall complete **capture** configuration states in one click. **Presets** are the audio/capture slice of the broader **Profile** model in F12 — a profile may reference a default capture preset among other settings.

- A **Preset** captures: which sources are active, per-source volume/panning, recording vs transcription input selection, recording format, and output folder
- Built-in preset templates: Podcast, Streaming, Meeting, Interview, Music Practice
- One-click preset activation from the main UI, optional menu bar shortcut, or global keyboard shortcut (shortcut bindings themselves are user-configurable per F12)
- Preset scheduler: automatically activate a preset based on time of day or detected calendar event type (scheduler rules live under F12 automations; preset is the payload)
- Import/export presets as JSON for sharing across machines or with teammates

**Acceptance Criteria:**
- Switching between presets takes < 500ms with no audio dropout
- Preset state is fully restored after app restart
- Scheduler successfully activates the correct preset for a calendar event in ≥ 95% of test cases

***

### F12 — User Configuration, Profiles, Remappable Shortcuts & Automations

**Priority: P0 (Must Have) for configuration discoverability; P1 for full automation builder depth**

Gravai treats **configuration as a product**: nothing important is hard-coded for power users without a path to change it. **Profiles** bundle settings so switching context (e.g. office vs. studio) is one action. **Shortcuts** and **automations** are editable in-app, not only in plist defaults or docs.

#### Configuration surface
- Single **Settings** (or equivalent) hierarchy: search, categories, and consistent patterns (toggle, slider, picker, path, API key fields)
- **Defaults documented in UI** where helpful (e.g. “Default: 3s”) and reset-to-default per control or per section
- **Import / export** of configuration: at minimum JSON (or platform-native bundle) for full settings, profiles, shortcuts map, and automation definitions — suitable for backup, team templates, and support
- No hidden feature flags for end users: if a behavior exists, it is reachable from Settings or a documented shortcut unless platform policy forbids it (e.g. certain system permissions)

#### Profiles
- A **Profile** is a named bundle that can include: active **capture preset** (F7), transcription engine and model parameters, diarization on/off, meeting-detection sensitivity, summary/LLM provider choice, export defaults, notification behavior, and **shortcut + automation sets** (or references to shared shortcut/automation libraries)
- **Switch profile** from main UI, menu bar, and assignable global shortcut
- Built-in starter profiles (e.g. Meeting-focused, Podcast-focused, Minimal) plus unlimited user profiles
- Optional **per-profile overrides** only where the product explicitly allows (e.g. “use system default mic” vs. fixed device); avoid duplicate conflicting UIs — profile wins are predictable and shown in UI when switching

#### Keyboard shortcuts
- **In-app** and **global** shortcuts listed in a dedicated **Shortcuts** editor
- Every bindable action exposed in the shortcuts UI; **fully remappable** where macOS allows (respect system reserved combinations; detect and warn on conflicts with the OS and other apps where feasible)
- Support **chords** or multi-step shortcuts only if the UI toolkit allows; otherwise document limitations
- Export/import shortcut maps (e.g. with profile or standalone) for portability

#### Automations
- **Automation** = trigger + condition(s) + action(s). Examples: calendar event starts → activate profile “Work”; meeting app foregrounded → show “Record?”; session ended → run export template
- Central **Automations** list: enable/disable, edit, duplicate, reorder priority where conflicts arise
- Templates for common flows; advanced users can compose from building blocks without scripting in v1 (scripting / AppleScript hooks remain an open question for later)
- Clear logging or last-run status for debugging (“why did this fire?”)

#### Acceptance Criteria
- A new user can find how to change transcription-related defaults without reading external docs (in-app navigation or search from Settings)
- Creating a new profile, assigning a capture preset and a different summary provider, and switching profiles completes in under 10 clicks from cold start
- Remapping a global shortcut persists across restart and is visible in the Shortcuts editor
- Disabling an automation prevents its actions; no orphaned background triggers without user visibility
- Exported configuration JSON round-trips on the same app version (forward compatibility best-effort across minor versions)

***

### Extensibility — Pluggable Engines (Cross-Cutting)

**Priority: P1 (architecture from Alpha); additional providers ship per milestone**

Implementation must not assume a single transcription or summarization backend.

- **Transcription provider interface**: local Whisper/CoreML, future **external transcription** (user-supplied endpoint, batch file upload, or licensed SDK) registered behind the same pipeline contract (audio in → timed segments + metadata out). Privacy and network policy are explicit per provider (on-device vs. BYO server).
- **Summarization / LLM provider interface**: already implied by F6 (Ollama, BYOK); formalize as pluggable modules with shared types (prompt templates, token limits, errors).
- **Optional engine slots** (e.g. VAD, diarization) use the same pattern: swap implementation without changing session UI contract
- **Configuration** exposes provider choice and provider-specific options (API URL, model name, timeouts) when a provider is selected — no compile-time-only engine selection for user-facing features
- **Versioning**: provider adapters are isolated crates or modules with minimal coupling to UI; new provider = new adapter + settings schema fragment + validation

This aligns with the technical stack section: the **AI Pipeline** consumes **interfaces**, not concrete Whisper-only types, from the first milestone that ships transcription.

***

### F8 — Semantic Search & Archive

**Priority: P1 (Should Have)**

Every meeting becomes a searchable, permanent asset.

- Full-text search across all transcripts (exact keyword)
- Semantic search: query by meaning, not keywords (e.g., "meeting where we discussed the AWS migration")
- Embeddings generated on-device using a compact sentence transformer (e.g., all-MiniLM-L6-v2 via CoreML)
- Filter by: date range, participants, meeting app, tags, duration
- Playback of original audio synchronized to transcript (click any sentence → jump to that moment in audio)
- Inline transcript editor: fix transcription errors; edits are version-tracked

**Acceptance Criteria:**
- Semantic search returns the correct meeting in top-3 results ≥ 80% of the time for natural language queries
- Audio/transcript sync is accurate to within ± 1 second
- Archive supports ≥ 10,000 meeting sessions without performance degradation

***

### F9 — "Ask Gravai" — Conversational Interface

**Priority: P2 (Nice to Have)**

Chat with any meeting or across your entire archive.

- Per-meeting chat: "What did John say about the Q2 budget?" or "List all action items assigned to me"
- Cross-archive chat: "Have we discussed GDPR compliance in any meeting this year?"
- Answers are grounded in transcripts and include a citation link (timestamp + meeting name)
- Context window management: summarize older transcript segments before sending to LLM to stay within token limits
- Works fully offline with local LLM; optional cloud LLM for longer context via BYOK

**Acceptance Criteria:**
- Per-meeting queries return factually accurate answers backed by visible transcript citations
- Cross-archive queries search across the last 12 months of meetings by default
- Response latency < 5 seconds for single-meeting queries on M-series Mac with local LLM

***

### F10 — Export & Integrations

**Priority: P1 (Should Have)**

Gravai output should flow into the tools people already use.

| Export Target | Format | Details |
|---|---|---|
| **Local File** | PDF, Markdown, TXT | Full transcript + summary + action items |
| **Obsidian** | Markdown + YAML frontmatter | Auto-push to configured vault folder |
| **Notion** | API | Push summary + action items to a configured database |

- Export triggered manually or automatically on meeting end (per-integration toggle)
- Template editor: customize the Markdown/PDF export layout
- Selective export: choose which sections to include (full transcript, summary only, action items only)

**Acceptance Criteria:**
- Obsidian export creates a correctly formatted note in < 5 seconds after meeting end
- Notion push works with standard API key; no OAuth flow required for v1
- PDF export is readable on mobile (A4 layout, ≥ 11pt font)

***


### F11 — Auto Silence Trimming

**Priority: P2 (Nice to Have)**

- Detect and optionally trim silent segments (> configurable threshold, default 3s) from exported audio
- Preview trim before applying; non-destructive (original always preserved)
- Useful for podcasters to clean up exported recordings before editing in a DAW

***

## Technical Architecture Overview

Gravai is implemented in **Rust** on macOS. The internal design should **reuse patterns and crates proven in ears** (`/Users/tpinto/code/ears-rust-api`, crate `ears-rust-api`): session lifecycle, shared `AppState`, event bus, config and storage layout, ScreenCaptureKit + device capture, Whisper (`whisper-rs`), ONNX/ORT for auxiliary models (e.g. VAD), structured logging, and model download/preflight flows. **Unlike ears**, Gravai does **not** ship a first-party REST or WebSocket API — all surfaces are in-process calls from the UI to the core. **Unlike ears** (menu bar icon + API only), Gravai exposes a **proper application UI**: primary windows for recording, transcripts, archive, presets, and settings; the menu bar may remain a convenience entry point but is not the sole interface.

**Configuration & profiles:** User settings, profiles, shortcut maps, and automation definitions live in a **single versioned configuration model** (e.g. structured on disk + migration layer), not scattered ad hoc. The UI reads/writes through one configuration service so import/export and profile switching stay consistent. **Shortcuts** are data (action id → key sequence), not compile-time constants only.

**Pluggable pipelines:** The audio graph and session orchestrator call **trait-based or enum-dispatched providers** for transcription, summarization, and optional stages (VAD, diarization). Concrete implementations (Whisper on-device, HTTP transcription adapter, Ollama, OpenAI BYOK) register at startup or when the user installs/enables them. Adding a new engine is **additive**: implement the trait, add settings schema + validation, and wire the provider into the picker — no fork of the session FSM.

```
┌─────────────────────────────────────────────────────────────┐
│                         Gravai.app                         │
│                    (Rust binary + UI)                       │
│                                                             │
│  ┌──────────────┐   ┌──────────────┐   ┌────────────────┐  │
│  │  Audio Engine │   │  AI Pipeline │   │  Storage Layer │  │
│  │  (cpal +      │──▶│  Whisper-rs, │──▶│  (SQLite +     │  │
│  │   ScreenCapture│   │  CoreML /    │   │   embeddings + │  │
│  │   Kit, etc.)  │   │  local LLM)  │   │   audio files) │  │
│  └──────────────┘   └──────────────┘   └────────────────┘  │
│          │                  │                   │            │
│          └──────────────────┴───────────────────┘            │
│                            │                                │
│                            ▼                                │
│  ┌──────────────────────────────────────────────────────┐   │
│  │     UI layer — full app (windows / panels)           │   │
│  │     Transcript editor · Archive · Presets · Export   │   │
│  │     Optional: menu bar / status item shortcuts       │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
         │                                      │
    iCloud / LAN sync                   External APIs (BYOK)
         │                                      │
    iOS Companion                      Notion / Slack / Jira
```

**Core Technology Stack (macOS):**
- **Language:** Rust (edition aligned with ears); UI toolkit **TBD** (native-feeling macOS windowing + views — evaluate against product goals and App Store constraints).
- **Reference codebase:** `ears-rust-api` for audio stack, session model, transcription pipeline shape, and operational patterns — **omit** Axum routers, WebSocket server, and “headless API + tray only” product shape.
- **Audio:** `cpal` for device input; ScreenCaptureKit (e.g. `screencapturekit` crate) for system/per-app capture where applicable; CoreAudio/AVFoundation via FFI or higher-level crates as needed for encoding/export — no Gravai-owned virtual mic or system-wide loopback **output** to other apps.
- **Transcription:** `whisper-rs` (and Apple Neural Engine / CoreML paths where conversion or native models are justified).
- **Speaker diarization & embeddings:** ONNX/CoreML or Apple frameworks as selected per milestone; same privacy constraints as the rest of the PRD.
- **Local LLM:** Ollama (bundled or user-installed) with Llama 3.2 3B as default, 8B optional — or in-process Rust bindings if the architecture converges there.
- **Storage:** SQLite (transcripts + metadata) + file system (audio files); patterns compatible with ears’ storage approach where applicable.
- **Concurrency:** `tokio` / async patterns consistent with ears for I/O-heavy and pipeline work.
- **Sync:** CloudKit or equivalent for iOS ↔ macOS (encrypted) — iOS companion may remain a separate native codebase; share models and export formats across platforms.

**Explicit non-stack for v1:** No bundled HTTP API for local consumers; integrations (BYOK, Notion, etc.) use outbound HTTP from the app, not a Gravai-hosted server.

***

## Competitive Positioning

| Feature | Gravai | Slipbox AI | Dipper | Granola | Otter.ai |
|---|---|---|---|---|---|
| Multi-source capture (no virtual mic out) | ✅ | ❌ | ❌ | ❌ | ❌ |
| Per-app volume & panning | ✅ | ❌ | ✅ | ❌ | ❌ |
| Capture from DAW / app audio (into Gravai) | ✅ | ❌ | ✅ | ❌ | ❌ |
| Multi-track recording | ✅ | ❌ | Partial | ❌ | ❌ |
| On-device transcription | ✅ | ✅ | ❌ | ✅ | ✅ (optional) |
| Speaker diarization | ✅ | ✅ | ❌ | Partial | ✅ |
| AI meeting summaries | ✅ | ✅ | ❌ | ✅ | ✅ |
| Semantic search archive | ✅ | ✅ | ❌ | ❌ | ✅ |
| No cloud / no bot | ✅ | ✅ | ✅ | ✅ | ❌ |
| Calendar auto-detection | ✅ | ✅ | ❌ | ✅ | ✅ |
| Capture / input presets | ✅ | ❌ | ✅ | ❌ | ❌ |
| Full settings + profiles + remappable shortcuts | ✅ | Partial | Partial | ❌ | ❌ |
| BYOK (Bring Your Own Key) | ✅ | ✅ | ❌ | ❌ | ❌ |
| Obsidian / Notion export | ✅ | ✅ | ❌ | ❌ | ❌ |
| iOS companion | ✅ | ✅ | ❌ | ❌ | ✅ |

***

## Milestones & Release Plan

| Milestone | Features | Target |
|---|---|---|
| **Alpha v0.1** | F1 (Audio Capture), F2 (Recording / transcription inputs), F12 subset (Settings shell, config persistence, import/export stub) | Month 2 |
| **Alpha v0.2** | F3 (Transcription), F4 (Meeting Detection), transcription **provider abstraction** (local first; external adapter stub optional) | Month 3 |
| **Beta v0.5** | F5 (Diarization), F6 (AI Summaries), F7 (Presets), F12 (profiles, shortcuts editor, automations v1) | Month 5 |
| **Beta v0.8** | F8 (Search), F9 (Ask Gravai), F10 (Export) | Month 7 |
| **v1.0 GA** | iOS companion, F11 (Silence Trim), polish, App Store | Month 9 |

***

## Open Questions

1. **Rust UI stack on macOS**: Pick a windowing and widget approach (native shell + web, immediate-mode, or retained GUI crates) that matches App Store / accessibility goals and team velocity.
2. **Capture APIs on macOS**: Validate ScreenCaptureKit / CoreAudio capture paths for per-app audio on Sonoma and Sequoia without relying on a Gravai virtual output device.
3. **Whisper model size trade-off**: Ship large-v3 (3GB, best accuracy) or distil-Whisper (600MB, 90% accuracy at 6x speed) as default? Possibly offer both with a quality/speed toggle.
4. **Ollama bundling**: Bundle Ollama runtime in the app (large binary, simpler UX) or require user to install separately (smaller app, friction)?
5. **External transcription**: Which first-class integrations (generic HTTPS + API key, SRT upload, vendor SDKs) justify P1 vs. community plugins post–v1?
6. **Automation depth**: Visual rule builder only for v1, or expose AppleScript / Shortcuts.app bridges for power users?
7. **Pricing model**: One-time purchase vs. subscription. Given the privacy-first, no-cloud positioning, a one-time purchase with optional "Pro AI features" IAP may fit better than recurring billing.
8. **App Store vs. direct distribution**: App Store provides trust and discoverability but adds sandboxing constraints that may limit CoreAudio access. Direct distribution via Gumroad or own site avoids these constraints.