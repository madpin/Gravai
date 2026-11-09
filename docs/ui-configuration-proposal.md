# Settings interface — proposal for evaluation

**Status:** Draft, for review  
**Date:** 9 November 2026  
**Scope:** Where configuration lives in the app. Archive, Ask, and the recording pipeline are unchanged.

Read the decisions in section 4 and mark each one. Section 7 is the list of questions that block an implementation.

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
4. **A setup names which parts it includes.** Applying it changes those parts and leaves the others alone.
5. **A model is a file a setting points at.** Download and delete sit on that setting. A library page can show the files. It does not choose the active model by itself.
6. **Previous use is recallable without a name.** Each recording start stores the parts that affected it. The Record screen can put that combination back.

## 4. Decisions to mark

Mark each row. The recommendation is the one this document is written around.

| # | Decision | Recommendation | If you reject it |
|---|---|---|---|
| D1 | Retire Presets and Profiles as places. Replace both with setups. | Accept | Keep two objects only if their names tell the user which fields each one owns, and a profile stops containing capture, speech, and the model at once. |
| D2 | Stop reapplying a bundle at `start_session`. | Accept | The bug in section 1 stays, in whatever the new object is called. |
| D3 | Sidebar is Record, Archive, Ask, and one Settings entry. | Accept | Configuration stays a list of pages, and the section list in section 5 becomes that list. The ownership rules still apply. |
| D4 | Models stops being a destination. Each picker shows download state. Library lists files. | Accept | Keep a Models page as the file manager only, with no "active" badge. |
| D5 | Setups are optional bookmarks of Capture, Speech, Assistant, Sessions, in any combination. | Accept | A setup always copies all four. Independent configuration is then "change it after you apply." |
| D6 | Record shows the last eight recording starts, named or not, and can restore them. | Accept | Only named setups. Unnamed work is kept only because the live configuration itself persists. |
| D7 | Changing a control while a setup is in use marks it "with changes" and offers Update, Save as new, and Revert. | Accept | A setup is a one-shot template. After apply, the link is forgotten. |
| D8 | Archive and Ask stay as they are. | Accept | Out of scope for this pass either way. |
| D9 | Gain becomes one live Capture setting and is actually applied to the recorded audio. The two current sliders go away. | Accept | Remove the sliders and the preset gain fields until gain exists in the pipeline. Do not keep a control that does nothing. |
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

Session controls stay on Record, and they edit the same Capture values Settings does: mic device, mic on, system audio on, which app. Levels on this screen are the same gain values as Capture.

Above the button, one line describes the rest of what will run, for example `Whisper medium · English · Qwen3 4B`. That line opens Speech and Assistant. It does not open a second editor.

```
MacBook Pro mic  +  Zoom                 gain ━━━  ━━━
Whisper medium · English · Qwen3 4B

[ ● Record ]

Using Podcast, unchanged                 [ Recent ▾ ]
```

**Recent** contains:

- **Saved.** User setups, plus the built-in templates from D10.
- **Previous recordings.** Up to eight starts. Each row is the session title and time, and it restores the Capture, Speech, Assistant, and Sessions values stored for that start.

After a restore, the line reads `Using Podcast, unchanged`. Change the language and it reads `Using Podcast, with changes`, with Update, Save as new, and Revert. Revert restores the setup that was applied. Update rewrites that setup. A control change on its own does not.

Saving asks which parts to include. All four start checked. Uncheck Assistant and applying "Podcast" later leaves the language model alone.

Automations apply a setup, or set one field. The separate "switch profile" and "switch preset" actions become that.

### Settings sections

Each section shows the controls people change. Everything else from the inventory in section 6 is on that same page behind More.

**Capture.** Mic device, mic on, mic gain, system audio on, which app, system gain, sample rate, bit depth, channels, format, bitrate, output folder, compress when a recording stops, and whether audio is saved at all.

**Speech.** Language, transcription engine, Whisper model with its download state on the row, speakers (on, engine, max), echo suppression, emotion, transcript correction (on, batch, debounce, timeout, prompt, optional other model). More: voice-detection engine and its model download, pause, min and max utterance, Silero threshold, WebRTC aggressiveness, hallucination lists.

**Assistant.** On-device or API. On-device: model, with download and a custom GGUF URL on the row. API: base URL, model name, key, max tokens. At the bottom, the search embedding model and auto-index, because that is the other model choice.

**Sessions.** Meeting detection, allowed apps, lead time, idle timeout. Calendar naming and the calendar permission status. Automations. Export: auto transcript, auto audio, folders, transcript format, real-time save.

**Library.** Knowledge entries. Disk usage for sessions. Installed model files, with size and delete. Each file names the section that uses it (Speech, Assistant, or the search row). Choosing the active file happens in that section.

**App.** Shortcuts. Updates. Health and the setup wizard. Import and export of the live configuration. Raw JSON under More.

Knowledge, shortcuts, automations, and storage keep their current behavior. They move under these sections so the sidebar stops listing them beside Record.

### Onboarding

The last step lands on Record with the live defaults already set. It does not send the user to choose a profile.

## 6. Where each current setting goes

"Live" means it is in `config.json` today and a working control writes it. "Dropped" means the profile form shows it and save discards it. "JSON" means only the raw editor can set it. "Dead" means a control exists and nothing in the pipeline reads it.

| Setting | Today | New home |
|---|---|---|
| Mic device, mic on, system on, captured app | Live, from Record. Preset then overwrites the two on/off flags at start | Capture, also on Record |
| Mic gain, system gain | Dead, on the preset and on Record | Capture, wired to the pipeline (D9) |
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
| Embedding model, auto-index | Live. Model from Models, auto-index from System | Assistant, search group |
| Meeting detection | Profile and System | Sessions |
| Calendar naming | Live, from System | Sessions |
| Lead time, idle timeout, allowed apps | JSON | Sessions → More |
| Calendar permission | System | Sessions |
| Automations | Own page. Actions target a profile or a preset | Sessions |
| Export toggles, folders, format, real-time save | Dropped on the profile form. Record displays the live flags | Sessions |
| Knowledge | Own page | Library |
| Session disk usage | Storage page | Library |
| Model files, custom GGUF download | Models page | On the setting that uses the file. Library lists files and can delete them |
| Shortcuts | Own page. Includes "next preset" and "switch profile" | App. Those two actions become "apply next setup" or go away |
| Updates, health, wizard, import/export, raw JSON | System | App |
| `preset_id`, `shortcut_set_id` | Stored, unused at record start | Removed. A setup stores the parts it includes |

## 7. Questions that block implementation

1. **Name.** Setup, template, or something else? The word has to cover "Meeting" and "what I did Tuesday" without sounding like a user account. Profile is a poor fit because it already means the bundle we are retiring.
2. **During a recording.** Applying a setup or editing Capture while audio is running: take effect now, or on the next start? Device switching already works mid-recording. Model and voice-detection changes are heavier.
3. **How a previous recording is stored.** A snapshot of the four parts at start is enough to restore. Storing a diff from the defaults is smaller and worse to explain. Recommendation: store the four parts.
4. **Search.** Embedding sits under Assistant in this draft because it is a model. It can be its own Search group inside Settings if that reads more clearly.
5. **Correction.** It sits under Speech because it changes the transcript. The model it calls is the Assistant model unless overridden. Say if you want the whole correction block under Assistant instead.
6. **Built-ins.** D10 keeps Meeting, Podcast, and Light as templates. Light is today's Minimal: small Whisper model, no diarization, no emotion, no correction, mic only. Confirm the three, or name the set you want.
7. **Shortcuts.** `preset.next` and `profile.switch` need a replacement, or they are removed. Recommendation: remove them until a setup list has an order people would cycle through.

## 8. Migration

On first launch after the change:

- The current live configuration is left as it is, and one recent entry is written from it, named "Before update".
- Each preset becomes a setup that includes only Capture. Gain is copied into that setup and stays inert until D9 is implemented.
- Each profile becomes a setup that includes Speech and Assistant for the fields it actually stored, and Sessions for meeting detection only. Emotion and correction on/off go to Speech. `preset_id` is not followed, because the app never applied it. If a preset and a profile share a built-in id (`meeting`, `podcast`, `minimal`), they are merged into one setup.
- `active_profile_id` and `active_preset_id` are dropped. The live configuration already holds the last applied result for everything the start path copied. "Before update" covers the combination that was actually running.
- Automations that switch a profile or a preset are pointed at the migrated setup.

Nothing in Archive, the transcript, or the model files moves.

## 9. Out of scope

- Visual restyle of Record, Archive, or Ask beyond the setup line and the single settings entry.
- New recording features, including bookmarks and retention from `docs/NEXT_PHASE_LIVE_ACCESSIBILITY.md`.
- Changing which models exist, or the on-device and API providers.
- iOS.

## 10. Implementation order, after the decisions are marked

1. Stop the `start_session` overwrite, and make every working control write the live configuration only. This removes the conflict even before the new pages exist.
2. Move the controls into the six sections. Delete the profile form fields that do not persist, by giving them real fields in Sessions, Speech, and Assistant.
3. Add setups and recents on Record, then migrate the old files.
4. Attach download state to the Speech, Assistant, and voice-detection pickers. Reduce Models to the Library file list.
5. Wire gain, or remove it, per D9.
