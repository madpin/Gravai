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
