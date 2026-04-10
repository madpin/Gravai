# Next phase: live bookmarks, transcript playback sync, accessibility surfaces, retention

**Status:** Draft (planning)  
**Scope:** Live capture affordances (bookmarks, captions), archive playback sync, and **per-profile storage retention** with optional **compressed** (not necessarily lossless) audio archival.

**Principles**

- **Opt-in and configurable:** Overlays and mini-transcripts are off by default; the user explicitly enables them and controls position, opacity, and behavior. **Retention policies** are per profile and should default to non-destructive until the user opts in.
- **Privacy unchanged:** No new cloud surfaces; bookmarks and sync metadata stay in local SQLite with the session.
- **Shortcuts-first:** Every primary action should be reachable from the keyboard; UI buttons are discoverable alternatives, not the only path.

---

## 1. Bookmark / flag moments during recording

### User value

Users can mark “this mattered” during a long recording without stopping audio—useful for decisions, quotes, follow-ups, or “come back here” navigation in the archive.

### Behavior

1. **While recording (or paused, if product allows bookmarks in paused state):** Create a **bookmark** anchored to **session timeline** (recommended: offset in milliseconds from session start, or monotonic clock aligned with WAV/session metadata—same basis used elsewhere for utterances).
2. **Optional short note:** Free text, short (e.g. hard cap 500 characters; exact limit TBD). Empty note is allowed (pure flag).
3. **Inputs:**
   - **Global hotkey** (user-configurable; default TBD, must not conflict with macOS or existing Gravai shortcuts). Single action = bookmark; optional chord or double-tap pattern could open a small “add note” prompt—prefer one hotkey + optional inline note modal.
   - **In-app button** on the Recording screen (and optionally tray/menu context): “Add bookmark” with same semantics as the hotkey.
4. **Feedback:** Subtle confirmation (toast, tray notification, or inline counter) so the user knows the bookmark was stored without interrupting the call.

### Data model (proposed)

- New table, e.g. `session_bookmarks` (name TBD), columns at minimum:
  - `id`, `session_id`, `offset_ms` (or `t_offset_ms`), `created_at`, `note` (nullable text), optional `label` / `kind` enum later.
- Bookmarks are **independent of utterances** (an utterance may not exist yet at flag time); optional future link `utterance_id` nullable if the engine can resolve “nearest utterance after insert.”

### Implementation touchpoints

| Layer | Work |
| --- | --- |
| `gravai-storage` | Migration + CRUD + query by session ordered by offset |
| `gravai-core` / session | Resolve “now” → session offset from active recording pipeline |
| Tauri | Commands: `add_bookmark`, `list_bookmarks`, optional `delete_bookmark`; emit `GravaiEvent` / `gravai:*` for UI refresh |
| `gravai-config` | Shortcut entry + optional “show note dialog on bookmark” toggle |
| Frontend | Recording page: button + bind shortcut via existing shortcuts machinery; optional minimal modal for note |

### Acceptance criteria

- [ ] User can add a bookmark via **hotkey** and via **button** during an active session; timestamp matches replay position within agreed tolerance (e.g. ±100 ms once audio seek exists).
- [ ] Optional note persists and appears in archive/session detail (surface TBD: sidebar list or markers on a future timeline strip).
- [ ] Bookmarks survive app restart and appear for the same `session_id`.
- [ ] Shortcut is remappable and documented in Shortcuts settings.

### Open questions

- Should bookmarks be exportable (Markdown front matter, Obsidian block) in this phase or a fast follow?
- Confirm canonical time base: session wall clock vs audio sample position for multi-track drift.

---

## 2. Click transcript → seek audio (and vice versa) with a clear playhead

### User value

Reviewing a meeting means **text and audio stay locked together**: clicking a line jumps playback; as audio plays, the visible transcript shows **which utterance is current** (playhead).

### Behavior

1. **Playback context:** Applies to **archive / session review** (and any screen where recorded audio + transcript are shown together). Live recording may only support “scroll to latest” today; this phase targets **synchronized review** once a session has audio + stored utterances.
2. **Single timeline for playback:** Sessions with **two sources** (e.g. microphone + system) are recorded as **separate tracks**. For transcript seek and playhead sync, playback must use **one merged stream**—both tracks **mixed into a single file** (or an on-the-fly mix with one shared clock) so `currentTime` matches the **same timeline** utterances were transcribed on. **Do not** require the user to pick a track or run two playheads in parallel for v1.
3. **Click utterance (or line/time range) → seek** audio to the start of that utterance (or to `start_ms` stored per utterance—verify schema), against that **merged** timeline.
4. **Audio time update → transcript:** While playing, highlight the **active utterance** (playhead). Scrolling policy: optional auto-scroll to active line (toggle), default off or “smart” to avoid fighting manual scroll.
5. **Visual design:** Distinct playhead (background strip, left border, or caret) consistent with app tokens; keyboard accessibility: focusable rows, Enter to seek.

### Data requirements

- Utterances must carry **time offsets** on the **merged session timeline** (same origin as the mixed-down playback asset). If transcription today uses a single mixed feed internally, the stored merge for archive playback must **match that alignment** (levels, padding, and start skew between tracks must be identical or compensated so seeks land correctly).
- **Merge artifact:** Either generate a **dedicated mixed file** when the session stops (or lazily on first play), and store its path for replay, or define a deterministic real-time mix in the player with one clock—either way, one seekable timeline is the contract.

### Implementation touchpoints

| Layer | Work |
| --- | --- |
| Frontend | `TranscriptView` (or archive variant): click handlers, `currentTime` subscription from HTML5 audio or Tauri-backed player, `aria-current` / visual playhead |
| Tauri / Rust | **Play session audio** API: resolve **one** replay URL/path; if two WAVs exist, ensure mixdown exists or trigger **merge** (see `gravai-audio`) before returning |
| `gravai-audio` | **Merge two tracks → one** seekable asset for replay (same gain/pan rules as capture pipeline where applicable); cache path in storage or session metadata |
| Storage | Confirm utterance fields: `start_ms` / `end_ms` (or seconds); optional `replay_mix_path` (or reuse existing session audio fields); migrate if missing |

### Acceptance criteria

- [ ] Clicking an utterance seeks playback to that utterance’s start (within one frame / buffer tolerance).
- [ ] During playback, exactly one utterance is marked as **current**; updates smoothly as time advances (no flicker on boundaries—define debounce if needed).
- [ ] Clicking **progress bar / time** (if present) moves playhead in transcript to the containing utterance.
- [ ] Works with corrected transcript display mode if timestamps refer to original alignment (document behavior when text is edited).
- [ ] **Dual-track sessions:** playback is **one merged stream**; seek and playhead stay aligned with transcript (no per-track player in v1).

### Open questions

- **When to materialize the mix:** at **stop**, on **first open** in archive, or **background** after stop (CPU vs disk tradeoff).
- Very long utterances: sub-utterance word timing not in scope unless Whisper word timestamps are already stored.

---

## 3. System caption-style overlay or menu bar mini-transcript

### User value

**Hearing accessibility** and **glanceable capture** without bringing the main window forward: live text appears in a dedicated, user-controlled surface.

### Modes (user-configurable, all opt-in)

1. **Caption overlay:** A separate, always-on-top **floating window** showing the latest line or rolling few lines of the live transcript (or last finalized utterance). User configures: font size, max lines, opacity, position (preset corners), width, show/hide shadow, **always on top** on/off.
2. **Menu bar mini-transcript:** Tray/menu bar extra showing truncated latest text (or icon + tooltip with last line). Must respect menu bar space limits; optional click-to-expand popover with more lines.

**Activation:** Disabled by default. User turns on via Settings (and optionally a hotkey to **toggle** overlay visibility). No automatic activation on first launch.

### Platform notes (macOS + Tauri)

- **Overlay window:** Typically a second Webview window with `transparent: true`, appropriate `alwaysOnTop`, and click-through optional (may be phase 2). Must respect **screen recording / accessibility** expectations—overlay shows **derived text only**, not screen capture.
- **Menu bar:** Use Tauri **tray** / menu bar APIs; dynamic title length limits may require truncation with full text in popover.
- **Performance:** Subscribe to existing `gravai:transcript` (or equivalent) stream; throttle UI updates to avoid excessive redraws.

### Configuration (proposed `gravai-config` fields)

Nested object, e.g. `accessibility_live_captions` (name TBD):

- `enabled: bool` (master; default `false`)
- `mode: "overlay" | "menu_bar" | "both"`
- `overlay`: position, opacity, font_pt, max_lines, width_px, always_on_top
- `menu_bar`: max_chars, popover_enabled
- `toggle_overlay_hotkey`: optional shortcut

### Implementation touchpoints

| Layer | Work |
| --- | --- |
| Tauri | Register second window or tray popover; permissions in `tauri.conf.json` as needed |
| Frontend | Small Svelte bundle for overlay (minimal CSS, high contrast option) |
| `gravai-config` | Schema + migration + Settings UI section |
| Core | No change if events already carry transcript text; ensure session-boundary cleanup closes overlay state |

### Acceptance criteria

- [ ] Feature is **off** by default; enabling it requires explicit user action in Settings.
- [ ] User can choose **overlay**, **menu bar**, or **both**; settings persist across restarts.
- [ ] Overlay is configurable (at minimum: position preset, opacity, font size, line count).
- [ ] Toggle hotkey (if set) shows/hides overlay without quitting recording.
- [ ] When recording stops or session changes, overlay clears or shows inactive state—no stale text from previous session without user context.

### Open questions

- macOS **Spaces / full-screen** behavior: should overlay appear on active space only?
- Localize “Recording…” placeholder when transcript is empty?

---

## 4. Scheduled retention (per profile): WAV lifecycle, keep transcript + summary, optional compressed archive

### User value

Long-running users accumulate large **multi-track WAV** trees under `~/.gravai/sessions/`. Per-**profile** retention lets each bundle (e.g. work vs podcast) define how long raw captures stay in full quality, while **transcripts, summaries, embeddings, and SQLite rows** remain the durable source of truth. Optionally **transcode to a compressed format** so a smaller audio file remains for replay and seek-sync without keeping lossless WAV forever. **Codec and container are implementation-defined** (see below); the requirement is **meaningful size reduction** with **seekable** playback, not a specific algorithm.

### Behavior

1. **Profile-scoped policy** (stored on the profile in `gravai-config`, not global-only):
   - **Retention enabled** (default off or conservative default TBD).
   - **Age threshold:** e.g. “apply policy to session audio older than **N** days” (from `session.end` or file mtime—pick one rule and document it).
   - **Action on raw WAV:** one of:
     - **Delete raw only:** remove `.wav` (or defined raw extensions); keep DB + derived artifacts; playback for that session is transcript-only unless a compressed file exists.
     - **Transcode then delete raw:** produce **one** compressed replay file from the **merged timeline** (see §2: if two tracks exist, merge first, then encode); update session storage metadata to point replay at that file, then delete per-track raw files (and any intermediate mix if not retained).
     - **Transcode only (optional variant):** keep WAV until transcode succeeds and validates, then delete WAV (same as above; failed transcode should not delete source).
2. **Always retain** (unless user explicitly opts into a stricter policy elsewhere): **utterances, corrections, summaries, chat history pointers, exports metadata**—anything needed for archive search and AI features. This phase should **not** delete SQLite transcript content as part of the default retention path.
3. **Execution model:** background pass on app launch, daily timer while app is open, or explicit “Run retention now” in Settings—must be **idempotent** and safe if interrupted (crash mid-transcode leaves WAV intact or uses a temp file + atomic rename).

### Audio: compressed archival

- **Goal:** Smaller on-disk footprint while keeping **single-timeline** audio suitable for **playback + transcript seek** (section 2).
- **Format:** Any **widely supported, seekable** compressed representation is acceptable (e.g. lossy codecs in common containers). The app may expose a **small set of presets** (`compact` / `balanced` / `better_quality`) mapping to codec + bitrate, or a single internal default—exact codec (AAC, Opus, Vorbis, etc.) is an engineering choice, not a product lock-in.
- **Parameters:** Configurable **bitrate** and/or **quality preset** where the encoder allows; document defaults for mono vs stereo mixdown.
- **Source mapping:** For sessions with **two tracks**, retention transcode must target the **same single merged timeline** as archive playback in **§2** (one mixdown), not separate compressed files per track—otherwise seek/playhead and storage would diverge.
- **Timestamp alignment:** Utterance offsets must remain valid against the **file used for playback** after transcode (the merged timeline).

### Implementation touchpoints

| Layer | Work |
| --- | --- |
| `gravai-config` / profiles | Retention fields: `enabled`, `min_age_days`, `raw_wav_action: delete \| transcode_compressed`, `compressed_preset` or `codec` + `bitrate_kbps`, optional `run_on_launch` |
| `gravai-storage` | Session rows or sidecar: `audio_replay_path`, `archival_state`, `raw_deleted_at`; store encoded **mime/extension or format id** for the player path |
| `gravai-audio` or new module | Transcode pipeline (e.g. FFmpeg subprocess vs in-crate encoder—decide; FFmpeg supports many compressed outputs) |
| `gravai-core` / Tauri | Scheduler job, progress/logging, `GravaiEvent` for “session audio compacted” |
| Frontend | Profile or Storage settings: explain tradeoffs (quality, disk, replay) |

### Acceptance criteria

- [ ] Each profile can set **independent** retention rules; inactive profiles do not surprise-delete when switching.
- [ ] Sessions past **N** days trigger the configured action; younger sessions are untouched.
- [ ] **Transcode path:** after success, replay uses the compressed file (or documented mixdown); **transcript + summary** still load.
- [ ] **Delete path:** WAV removed; DB intact; UI clearly indicates “audio no longer available” vs “compressed copy available.”
- [ ] Failed transcode **does not** delete source WAV; user-visible error in logs and optional Settings banner.

### Open questions

- Legal / org policy: require **confirmation** before first enable per profile?
- Should **export** regenerate a WAV for users who need lossless after the fact (out of scope vs follow-up)?

---

## Suggested implementation order

1. **Storage + core time base** for bookmarks (unblocks Recording UI + future timeline).
2. **Transcript ↔ audio seek + playhead** in archive (depends on stable utterance timestamps + **merged** playback path when two tracks exist).
3. **Overlay / menu bar** (consumes same live transcript events; can parallelize UI once events are stable).
4. **Retention + compressed archival** (can start after playback path exists so “canonical replay file” and utterance alignment are defined; transcode should use the **§2 mixdown** as the transcode source when applicable).

---

## Traceability

| Feature | Primary crates / dirs |
| --- | --- |
| Bookmarks | `gravai-storage`, `gravai-core`, `src-tauri`, `gravai-config`, `src-frontend/pages/Recording.svelte` |
| Seek / playhead | `src-frontend/components/TranscriptView.svelte`, archive pages, `src-tauri` playback commands, `gravai-audio` (**two-track → one** replay mix) |
| Overlay / menu bar | `src-tauri` (window + tray), `src-frontend`, `gravai-config` |
| Retention / compressed audio | `gravai-config` (profiles), `gravai-storage`, `gravai-audio` (or encoder helper), `src-tauri`, Storage/Profiles UI |

This document is planning-only; it does not change product scope in `PRD.md` until reviewed.
