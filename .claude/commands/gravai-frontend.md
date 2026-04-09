---
description: Guide to Gravai frontend — Svelte 5 pages, stores, Tauri invoke/listen, component structure
allowed-tools: Read, Glob, Grep
---

You are helping with the Gravai frontend at `src-frontend/src/`. Provide accurate, concise information about the requested topic.

## Stack
- **Framework**: Svelte 5 + TypeScript
- **Build**: Vite
- **Communication**: Tauri `invoke`/`listen` only — no direct Rust access

## Directory Structure
```
src-frontend/src/
├── App.svelte          # Root: sidebar nav, page router, alert bar, status bar
├── lib/
│   ├── tauri.ts        # Tauri invoke/listen wrappers + helpers
│   └── store.ts        # Svelte writable stores (global state)
├── pages/
│   ├── Recording.svelte    # Live recording + transcript
│   ├── Archive.svelte      # Past sessions, search, export
│   ├── Chat.svelte         # Ask Gravai RAG
│   ├── Presets.svelte      # Capture preset management
│   ├── Profiles.svelte     # Transcription profile management
│   ├── Knowledge.svelte    # Knowledge base entries
│   ├── Models.svelte       # Model download status
│   ├── Shortcuts.svelte    # Keyboard shortcuts
│   ├── Automations.svelte  # Automation rules
│   ├── Storage.svelte      # Disk usage + cleanup
│   └── Settings.svelte     # Full settings panel
└── components/
    ├── TranscriptView.svelte  # Renders utterances (speaker-colored, timestamps)
    ├── StatusBar.svelte       # Bottom bar: health, version, config status
    ├── AlertBar.svelte        # Top stack of dismissible alerts
    ├── Icon.svelte            # lucide icon set renderer
    ├── SessionPicker.svelte   # Session dropdown selector
    ├── AppPicker.svelte       # ScreenCaptureKit app selector (bundle IDs)
    ├── PerfMonitor.svelte     # Debug CPU/memory panel
    └── Onboarding.svelte     # First-time setup + permission checks
```

## `lib/tauri.ts` — API Layer
```typescript
// Thin wrapper around Tauri's invoke
export function invoke<T>(cmd: string, args?: Record<string, any>): Promise<T>

// Event listener returning unlisten function
export function listen(event: string, handler: (e: TauriEvent) => void): Promise<() => void>

// Helpers
export function sourceIconName(source: string): string  // "mic" or "system" → icon name
export function fmtDuration(seconds: number): string     // 3661 → "1:01:01"
export function fmtTimer(seconds: number): string        // live timer format
```

## `lib/store.ts` — Global State
```typescript
// Navigation
export const currentPage = writable<string>("recording");

// Session state (mirrors backend FSM)
export const isRecording = writable<boolean>(false);
export const isPaused = writable<boolean>(false);
export const currentSessionId = writable<string | null>(null);
export const sessionStartTime = writable<number | null>(null);

// Live data
export const liveUtterances = writable<UtteranceRecord[]>([]);
export const lastSessionId = writable<string | null>(null);  // persists across tab switches
export const liveSummary = writable<MeetingSummary | null>(null);

// UI state
export const autoScrollEnabled = writable<boolean>(true);
export const healthStatus = writable<HealthStatus | null>(null);
export const activityLogs = writable<string[]>([]);  // last 100 messages

// Alerts (unified notification system)
export const alerts = writable<Alert[]>([]);

// Meeting detection
export const dismissedMeetingApps = writable<Set<string>>(new Set());

// Model downloads
export const modelDownloading = writable<ModelDownloadProgress | null>(null);
```

## Tauri Events (Frontend Listens)

| Event | Payload | Handler |
|-------|---------|---------|
| `"gravai:transcript"` | `UtteranceRecord` | Append to `liveUtterances` |
| `"gravai:volume"` | `{ source: string, level: number }` | Update VU meters |
| `"gravai:session"` | `{ session_id, state }` | Update `isRecording`, `isPaused` |
| `"gravai:meeting"` | `{ app_name, title? }` | Show meeting detection alert |
| `"gravai:meeting-ended"` | `{ app_name }` | Dismiss meeting alert |
| `"gravai:model-download"` | `{ model_id, downloaded, total }` | Update download progress |
| `"gravai:navigate"` | `{ page: string }` | Programmatic page navigation |
| `"gravai:update-available"` | version info | Show update alert |
| `"gravai:automation-start"` | session info | Handle auto-start from tray/automation |

## Key Pages

### `Recording.svelte`
- Session controls: start/stop/pause, mic/system toggles, device selectors, gain sliders
- VU meters: live volume bars per source
- Real-time transcript: auto-scrolls, speaker-color-coded via `TranscriptView`
- Active preset/profile display
- Editable session title
- Post-recording: summary generation button, sentiment display
- Polls: timer every 250ms, transcript every 2s, meeting detection every 2s (when idle)

### `Archive.svelte`
- Session list: date filter, meeting app filter, search
- Session detail: full transcript view, export buttons (Markdown/PDF/Obsidian/Notion)
- Session actions: rename, delete audio, delete all, re-export

### `Chat.svelte`
- Multi-turn RAG conversation
- Session scope selector (or global search)
- Clickable citations → navigate to Archive session + utterance
- Conversation history from DB

## Tauri Invoke Pattern
```typescript
// Type-safe invoke
const session = await invoke<SessionInfo>("start_session", {
    micEnabled: true,
    systemEnabled: false,
});

// Error handling
try {
    await invoke("stop_session");
} catch (e) {
    // e is a string (GravaiError serialized)
    alerts.update(a => [...a, { type: "error", message: e }]);
}
```

## `App.svelte` Events
- Listens for `"gravai:navigate"` → switch `currentPage`
- Listens for `"gravai:update-available"` → push update alert
- Listens for `"gravai:model-download"` → track in `modelDownloading` store
- Sidebar highlights active page, collapses settings section

---

Now answer the user's question about the Gravai frontend: $ARGUMENTS
