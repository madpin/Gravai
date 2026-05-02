// Shared reactive state using Svelte writable stores.
// These persist across page navigation (component mount/unmount).
import { writable } from "svelte/store";

export const currentPage = writable("recording");
export const isRecording = writable(false);
export const isPaused = writable(false);
export const currentSessionId = writable<string | null>(null);
export const sessionStartTime = writable<number | null>(null);
export const autoScrollEnabled = writable(true);
export const healthStatus = writable<string>("ok");

// Recording page state that must survive tab switches
export const liveUtterances = writable<any[]>([]);
export const lastSessionId = writable<string | null>(null);
export const activityLogs = writable<string[]>([]);
export const liveSummary = writable<any>(null);
export const bookmarkCount = writable(0);

// Meeting banner dismiss state (persists across tab switches)
export const dismissedMeetingApps = writable<Set<string>>(new Set());

// ======================================================================
// Unified alert system — one place for all user-facing notifications.
// Replaces: meeting banner, error toasts, transcription warnings.
// ======================================================================
export interface AppAlert {
  id: string;
  level: "error" | "warning" | "info" | "meeting";
  message: string;
  actions?: Array<{ label: string; handler: () => void }>;
  dismissable: boolean;
}
export const alerts = writable<AppAlert[]>([]);

export function addAlert(alert: Omit<AppAlert, "id">): string {
  const id = Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
  alerts.update((a) => {
    // Don't duplicate identical messages
    if (a.some((x) => x.message === alert.message)) return a;
    return [...a, { ...alert, id }];
  });
  return id;
}

export function dismissAlert(id: string) {
  alerts.update((a) => a.filter((x) => x.id !== id));
}

export function dismissAlertsByLevel(level: string) {
  alerts.update((a) => a.filter((x) => x.level !== level));
}

export function clearAlerts() {
  alerts.set([]);
}

// Model download state — persists across tab navigation
export const modelDownloading = writable<Record<string, { progress: number; status: string }>>({});

// Local LLM engine status — persists across tab navigation so any page
// (Recording, Archive, Chat, Settings) can show a shared "preparing model"
// indicator instead of looking hung during the (possibly multi-minute) ISQ
// first-run. Updated by the global `gravai:llm-status` listener in App.svelte.
//
// `progress` is a 0.0–1.0 estimate; `phase` is a short human label
// ("Quantizing weights", "Saving cache", …); `eta_seconds` is the typical
// total duration the backend expects, used to drive smooth animation
// between (~1 Hz) server-side progress ticks.
export type LlmStatusState = "idle" | "loading" | "first_run" | "progress" | "ready" | "unloaded" | "error";
export const llmStatus = writable<{
  state: LlmStatusState;
  model_id: string | null;
  message: string | null;
  progress: number | null;
  phase: string | null;
  eta_seconds: number | null;
  /** Wall-clock when the load started, used to render an elapsed timer. */
  started_at: number | null;
}>({
  state: "idle",
  model_id: null,
  message: null,
  progress: null,
  phase: null,
  eta_seconds: null,
  started_at: null,
});

// Cross-page navigation: clicking a citation in Chat navigates to Archive with this session pre-selected
export const pendingArchiveSessionId = writable<string | null>(null);
