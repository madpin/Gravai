// Recordings that did not finish cleanly. Scanned once at launch; the dialog
// can also be opened from Settings → Library.
import { writable, get } from "svelte/store";
import { invoke } from "./tauri";

export interface IncompleteSession {
  session_id: string;
  title: string | null;
  started_at: string | null;
  last_write: string | null;
  meeting_app: string | null;
  db_state: string | null;
  in_library: boolean;
  reasons: string[];
  seconds: number;
  dir: string;
  segments: number;
  tracks: Array<{ name: string; seconds: number; bytes: number; header_ok: boolean; sample_rate: number; channels: number }>;
  raw_bytes: number;
  has_playback: boolean;
  convert_bytes: number;
  utterances: number;
  preview: Array<{ speaker: string; text: string; start_ms: number | null }>;
  has_summary: boolean;
  bookmarks: number;
  settings: string | null;
}

export const incomplete = writable<IncompleteSession[]>([]);
export const recoveryOpen = writable(false);
let scanning: Promise<IncompleteSession[]> | null = null;

export async function scanIncomplete(): Promise<IncompleteSession[]> {
  scanning ??= invoke<IncompleteSession[]>("scan_incomplete_sessions")
    .then((list) => { incomplete.set(list); return list; })
    .catch(() => get(incomplete))
    .finally(() => { scanning = null; });
  return scanning;
}

export type RecoveryAction = "convert" | "keep" | "delete";

export async function resolveIncomplete(id: string, action: RecoveryAction): Promise<string> {
  const message = await invoke<string>("resolve_incomplete_session", { sessionId: id, action });
  incomplete.update((list) => list.filter((s) => s.session_id !== id));
  return message;
}
