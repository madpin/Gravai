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
