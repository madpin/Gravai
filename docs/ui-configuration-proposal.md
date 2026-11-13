# Settings interface — proposal for evaluation

**Status:** Implemented (all steps in section 11).  
**Date:** 9 November 2026, revised 12 November 2026  
**Scope:** Where configuration lives in the app. Archive, Ask, and the recording pipeline are unchanged.

Read the decisions in section 4 and mark each one. Section 7 has the review answers to the open questions. Section 8 is how each control states what it costs.

---

## 1. What this is trying to fix

The sidebar splits work (Recording, Archive, Ask Gravai) from eight configuration pages: Presets, Profiles, Knowledge, Models, Shortcuts, Automations, Storage, and System.

Presets, Profiles, and Models all answer "what will the next recording do?", and System repeats several of the same switches. The value on screen is not always the value that runs.

There is one live configuration, `~/.gravai/config.json`. Two other files are copied on top of it inside `start_session`:

- The active **profile** (`profiles.json`) overwrites transcription, language, voice detection, diarization, echo, correction, emotion, meeting detection, and the language model, for any field the profile has set.
- The active **preset** (`presets.json`) overwrites mic on/off, system audio on/off, sample rate, bit depth, channels, export format, and output folder.

Activating a preset only stores its id. It does not write `config.json`. The overwrite happens when recording starts, after the Record screen has already saved the mic device and the system-audio app into `config.json`. Those two device choices survive. The on/off flags from the preset replace whatever the Record screen just set.

A profile can store a `preset_id`. Built-in Meeting, Podcast, and Minimal profiles do. Nothing reads that field when a recording starts. Capture changes only if that preset is also the active preset on its own. The profile editor saves `preset_id` as empty, so editing a built-in profile drops the link.

A profile can store a `shortcut_set_id`. Nothing reads that either.

## 2. Controls that look real and are not

These are the corrections to the first pass. They change the proposal: the new Capture section has to *create* a working gain control, and migration must not pretend a profile already selects a preset.

| What the user sees | What happens |
|---|---|
| Preset mic gain and system gain | Saved on the preset. No recording code reads them. |
| Record screen mic and system volume sliders | Local UI state. Never sent to the backend. |
| Profile export toggles, folders, format, and real-time save | Sent on save, then ignored. The profile struct has no such fields, and JSON deserialization drops unknown keys. Reload shows the defaults. |
| Profile echo threshold and API URL | Same. Dropped on save. Echo threshold and API URL exist on the live configuration only, and only the raw JSON editor can set them today. |
| Models page, "set active LLM" | Writes `config.json`. The next recording start replaces it when the active profile has `llm_local_model` set. |
| Models page, active Whisper model | Display only. The page sends you to Profiles to change it. |
| System → meeting detection and emotion | Writes `config.json`, then writes the same values back onto the active profile so the profile will not undo them at the next start. |
| System → transcript correction | Writes `config.json` only. A profile that has `correction_enabled` set replaces the toggle at the next start. The rest of the correction settings (batch, debounce, prompt) stay, because the profile cannot store them. |
| Automation "switch preset" | Emits an event that nothing in the window listens to. The preset is never activated. |
| Automation "switch profile" | Stores the active id only. Unlike the Profiles page, it does not patch `config.json`, so Settings shows the old values until the next start overwrites them. |
| Shortcuts "next preset", "previous preset", "switch profile" | Listed and rebindable. No code handles these actions. |
| Record → emotion column | Follows the active profile's `sentiment_enabled`, not the setting the pipeline uses. |

Fields that exist on the live configuration and have no working control, only the raw JSON block under System:

- Save audio at all (`audio.recording.enabled`)
- AAC bitrate
- Hallucination blocklists
- Silero threshold, WebRTC aggressiveness
- Echo similarity threshold
- API key, max tokens, API base URL
- Diarization engine and max speakers
- Meeting lead time, idle timeout, allowed apps
- Correction timeout
- Export destinations (the profile form pretends to edit these)

The proposal gives each of these a home. Most of them stay behind More.

## 3. Rules

1. **The live configuration is what the next recording uses.** Changing a control writes that value. No second file is copied over it at record start.
2. **One setting, one home.** A screen can deep-link to that home. It does not host a second copy of the control.
3. **A setup is a bookmark**, not an editor. Saving and applying are explicit. Editing a control never rewrites a saved setup.
4. **Applying is one-shot.** A setup writes the live configuration once. Nothing stays "active", so there is never a question of which one wins.
5. **A model is a file a setting points at.** Download and delete sit on that setting. A library page can show the files. It does not choose the active model by itself.
6. **Previous use is recallable without a name.** Each recording start stores the parts that affected it. The Record screen can put that combination back.

## 4. Decisions to mark

Mark each row. The recommendation is the one this document is written around. D5, D7, and D9 changed in review: the first draft added parts, drift tracking, and a gain feature, which is more to explain than what it replaces.

| # | Decision | Recommendation | If you reject it |
|---|---|---|---|
| D1 | Retire Presets and Profiles as places. Replace both with setups. | Accept | Keep two objects only if their names tell the user which fields each one owns, and a profile stops containing capture, speech, and the model at once. |
| D2 | Stop reapplying a bundle at `start_session`. | Accept | The bug in section 1 stays, in whatever the new object is called. |
| D3 | Sidebar is Record, Archive, Ask, and one Settings entry. | Accept | Configuration stays a list of pages, and the section list in section 5 becomes that list. The ownership rules still apply. |
| D4 | Models stops being a destination. Each picker shows download state. Library lists files. | Accept | Keep a Models page as the file manager only, with no "active" badge. |
| D5 | A setup is a full snapshot of Capture, Speech, Assistant, and Sessions. Applying it replaces all four. | Accept | Setups store a chosen subset of parts. Applying two partial setups in a row then has no honest one-line description ("Using Podcast" is wrong after Meeting changed Speech). Revisit only if users ask. |
| D6 | Record shows the last eight recording starts, named or not, and can restore them. | Accept | Only named setups. Unnamed work is kept only because the live configuration itself persists. |
| D7 | Applying a setup is one-shot. It writes the live configuration and the link is forgotten. Nothing is "active" afterwards. | Accept | Track the applied setup, mark it "with changes" on any edit, and offer Update, Save as new, and Revert. That is a second state machine to explain for a feature used a few times a week. |
| D8 | Archive and Ask stay as they are. | Accept | Out of scope for this pass either way. |
| D9 | Remove the Record sliders and the preset gain fields. Gain comes back only as a pipeline feature with clipping protection and metering. | Accept | Wire gain now. On 24-bit capture that needs a limiter and a meter that shows the result, which is a feature, not a simplification. |
| D10 | Built-in Meeting, Podcast, and Light ship as templates you apply, not as a mode you remain inside. | Accept | Ship no built-ins. The migrated user data is the only list. |

## 5. The interface

### Sidebar

```
Record
Archive
Ask

Settings
```

Settings is one page with its own section list. The current eight configuration pages become sections, not peers of Record.

### Record

Session controls stay on Record, and they edit the same Capture values Settings does: mic device, mic on, system audio on, which app. Level meters stay. There is no gain slider (D9).

Above the button, one line describes the rest of what will run, for example `Whisper medium · English · Qwen3 4B`. That line opens Speech and Assistant. It does not open a second editor.

```
MacBook Pro mic  +  Zoom                 ▮▮▮▯▯  ▮▮▯▯▯
Whisper medium · English · Qwen3 4B

[ ● Record ]
                                         [ Recent ▾ ]
```

**Recent** contains:

- **Saved.** User setups, plus the built-in templates from D10.
- **Previous recordings.** Up to eight starts. Each row is the session title and time, and it restores the Capture, Speech, Assistant, and Sessions values stored for that start.

Applying one writes the live configuration and shows a short notice with Undo, which puts back the values from just before. After that, the summary line describes the live values and names no setup.

**Save current as…** stores the live Capture, Speech, Assistant, and Sessions values under a name. It asks for a name and nothing else.

Automations apply a setup, or set one field. The separate "switch profile" and "switch preset" actions become that.

### Settings sections

Each section shows the controls people change. Everything else from the inventory in section 6 is on that same page behind More.

**Capture.** Mic device, mic on, system audio on, which app, sample rate, bit depth, channels, format, bitrate, output folder, compress when a recording stops, and whether audio is saved at all.

**Speech.** Language, transcription engine, Whisper model with its download state on the row, speakers (on, engine, max), echo suppression, emotion, transcript correction (on, batch, debounce, timeout, prompt, optional other model). More: voice-detection engine and its model download, pause, min and max utterance, Silero threshold, WebRTC aggressiveness, hallucination lists.

**Assistant.** On-device or API. On-device: model, with download and a custom GGUF URL on the row. API: base URL, model name, key, max tokens.

**Search.** Embedding model with its download state, and auto-index. Its own group, because "Assistant" reads as chat, not indexing.

**Sessions.** Meeting detection, allowed apps, lead time, idle timeout. Calendar naming and the calendar permission status. Automations. Export: auto transcript, auto audio, folders, transcript format, real-time save.

**Library.** Knowledge entries. Disk usage for sessions. Installed model files, with size and delete. Each file names the section that uses it (Speech, Assistant, or Search). Choosing the active file happens in that section.

**App.** Shortcuts. Updates. Health and the setup wizard. Import and export of the live configuration. Raw JSON under More.

Knowledge, shortcuts, automations, and storage keep their current behavior. They move under these sections so the sidebar stops listing them beside Record.

### Onboarding

The last step lands on Record with the live defaults already set. It does not send the user to choose a profile.

## 6. Where each current setting goes

"Live" means it is in `config.json` today and a working control writes it. "Dropped" means the profile form shows it and save discards it. "JSON" means only the raw editor can set it. "Dead" means a control exists and nothing in the pipeline reads it.

| Setting | Today | New home |
|---|---|---|
| Mic device, mic on, system on, captured app | Live, from Record. Preset then overwrites the two on/off flags at start | Capture, also on Record |
| Mic gain, system gain | Dead, on the preset and on Record | Removed (D9) |
| Sample rate, bit depth, channels, format, output folder | Preset, applied at start | Capture |
| AAC bitrate, save-audio toggle | JSON | Capture → More |
| Compress on stop | Live, from System | Capture |
| Whisper engine, model, language | Profile, applied at start. Models only displays the model | Speech |
| Voice detection engine, pause, min, max | Profile, applied at start | Speech → More |
| Silero threshold, WebRTC aggressiveness | JSON | Speech → More |
| Hallucination lists | JSON | Speech → More |
| Echo on/off | Profile, applied at start | Speech |
| Echo threshold | Dropped on the profile form. JSON on the live config | Speech → More |
| Speakers on/off | Profile, applied at start | Speech |
| Speaker engine, max speakers | JSON | Speech → More |
| Emotion | Profile and System, both written so they match | Speech |
| Correction on/off | Profile, applied at start, and System | Speech |
| Correction batch, debounce, prompt | Live, from System | Speech |
| Correction timeout, correction model override | Timeout is JSON. Override is live, from System | Speech → More |
| LLM provider and model | Profile, applied at start. Models also writes the local model | Assistant |
| API URL | Dropped on the profile form | Assistant |
| API key, max tokens | JSON | Assistant → More |
| Embedding model, auto-index | Live. Model from Models, auto-index from System | Search |
| Meeting detection | Profile and System | Sessions |
| Calendar naming | Live, from System | Sessions |
| Lead time, idle timeout, allowed apps | JSON | Sessions → More |
| Calendar permission | System | Sessions |
| Automations | Own page. Actions target a profile or a preset | Sessions |
| Export toggles, folders, format, real-time save | Dropped on the profile form. Record displays the live flags | Sessions |
| Knowledge | Own page | Library |
| Session disk usage | Storage page | Library |
| Model files, custom GGUF download | Models page | On the setting that uses the file. Library lists files and can delete them |
| Shortcuts | Own page. Includes "next preset" and "switch profile", which nothing handles | App. Those actions are removed |
| Updates, health, wizard, import/export, raw JSON | System | App |
| `preset_id`, `shortcut_set_id` | Stored, unused at record start | Removed. A setup stores the parts it includes |

## 7. Open questions, with review answers

1. **Name.** "Setup". The Record menu needs no noun at all: its groups are **Saved** and **Recent**.
2. **During a recording.** Device and source on/off take effect now, which already works. Everything else applies at the next start, and the control says so with a "next recording" tag while a recording runs. Applying a setup mid-recording follows the same split.
3. **How a previous recording is stored.** A full snapshot of the four parts at start. A diff from defaults is smaller and worse to explain.
4. **Search.** Its own group in Settings (section 5).
5. **Correction.** Under Speech, because it changes the transcript. The row says which model it calls: "uses the Assistant model" unless overridden.
6. **Built-ins.** Meeting, Podcast, and Light, as templates (D10).
7. **Shortcuts.** Remove `preset.next`, `preset.prev`, and `profile.switch`. No code handles them today. Add "apply next setup" only if a setup list gets an order people would cycle through.
8. **Continuing a stopped recording.** It uses the snapshot stored for that session, not the live configuration, so a continued recording does not switch Whisper model or language halfway through. Capture devices are the exception and come from the live values, because the old device may be gone.

## 8. Consequences on the control

Ownership fixes "the value on screen is not the value that runs." It does not tell the user what a choice costs. Each control below carries one line, next to the value, in plain terms. No separate help page.

| Control | What the line says |
|---|---|
| Whisper model | Download size, memory while recording, and whether it keeps up with live speech on this Mac ("keeps up", "falls behind on long calls"). |
| On-device assistant model | Download size, memory, and rough speed for a summary. |
| Sample rate, bit depth, channels, format, save audio | Disk per hour of recording, computed from the current values. "Save audio off" says transcripts only, no playback. |
| Speakers, emotion, correction, Silero voice detection | Extra CPU or battery while recording. Correction also says it calls the assistant model during the recording. |
| Assistant provider = API | "Audio stays on this Mac. Transcript text is sent to {host}." Shown on the row, and once as a confirmation when switching to API. |
| Meeting detection, calendar naming | Background scan interval. Calendar naming can take up to a minute on large calendars. |
| Any control that does not apply mid-recording | "Next recording" tag while a recording runs (section 7, item 2). |

Sizes come from the model catalog that already drives downloads. Speed labels come from a short benchmark the first time a model loads, not from a static table.

## 9. Migration

The first draft said the live configuration already holds the last applied result. It does not. `start_session` copied the profile and preset onto an in-memory copy for that one recording and never wrote it back. Removing the overwrite without migrating would silently change the Whisper model, language, and assistant for anyone with an active profile.

**Step 1 (ships with removing the overwrite):**

- At launch, before anything else reads the configuration, the active profile and then the active preset are applied to `config.json`, in the same order the start path used. Both active ids are cleared. This runs once: with no active id there is nothing to do.
- Applying a preset or profile from its page, or from an automation, writes the live configuration directly. Nothing is remembered as active.

**Step 3 (when setups exist):**

- One recent entry is written from the live configuration, named "Before update".
- Each preset becomes a setup. Each profile becomes a setup. Fields the old object did not store are filled from the live configuration at migration time, so every setup is a full snapshot (D5). `preset_id` is not followed, because the app never applied it. If a preset and a profile share a built-in id (`meeting`, `podcast`, `minimal`), they are merged into one setup.
- Gain fields are dropped (D9).
- Automations that switch a profile or a preset are pointed at the migrated setup.

Nothing in Archive, the transcript, or the model files moves.

## 10. Out of scope

- Visual restyle of Record, Archive, or Ask beyond the summary line, the Recent menu, and the single settings entry.
- New recording features, including bookmarks and retention from `docs/NEXT_PHASE_LIVE_ACCESSIBILITY.md`.
- Changing which models exist, or the on-device and API providers.
- Gain as a pipeline feature (D9).
- iOS.

## 11. Implementation order

All steps are done.

1. **Done.** Remove the `start_session` overwrite and fold the active profile and preset into `config.json` at launch (section 9, step 1). Delete the controls that do nothing.
2. **Done.** One Settings entry with seven sections (Capture, Speech, Assistant, Search, Sessions, Library, App). Every JSON-only field from section 2 has a control, most behind More. Model pickers show size, memory, speed tier, and download state on the row. Library lists installed model files with "Used by".
3. **Done.** Setups (`setups.json`) are full snapshots with one-shot apply and Undo. Record has a summary line and a Setups menu with Saved, Previous recordings (last eight starts), and Save current as. Each recording stores its snapshot in `sessions.config_snapshot`; continuing a recording uses it. Presets and profiles migrate into setups on first launch and the old files are renamed to `*.migrated.json`.
4. **Done.** Consequence lines from section 8, from static catalog hints (no benchmark).
5. **Done.** Shortcuts are registered for real (global via the OS, window-only in the app) with a shortcut-store migration; preset/profile actions removed. `AppState.active_profile`/`active_preset`, `PresetActivated`, `ProfileSwitched`, `set_source_gain`, and the preset/profile commands are gone.

### Decisions made during implementation

- **Snapshots exclude** the mic device, the captured app, the API key, search, and updates. Devices belong to the moment; the key must never be copied into a bookmark.
- **The meeting start automation only starts recording.** It no longer applies anything, so settings never change behind the user's back. User-made rules that applied a profile or preset now apply the setup with the same id.
- **Global shortcut defaults** moved to ⌘⌥R / ⌘⌥S / ⌘⌥P. The old ⌘⇧S default would have taken Save As from every app. Bookmark stays ⌘⇧B, window-only.
- **Not exposed, because nothing reads them:** the HTTP transcription engine (a stub), transcript export format, auto-export audio, and the audio export folder. They stay in the config file until they work.
- The auto-export automation and the "Save the transcript" setting are shown as one control in Sessions.
