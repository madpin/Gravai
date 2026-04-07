// Shared reactive state using Svelte 5 runes-compatible writable stores
import { writable } from "svelte/store";

export const currentPage = writable("recording");
export const isRecording = writable(false);
export const isPaused = writable(false);
export const currentSessionId = writable<string | null>(null);
export const sessionStartTime = writable<number | null>(null);
export const autoScrollEnabled = writable(true);
export const healthStatus = writable<string>("ok");
