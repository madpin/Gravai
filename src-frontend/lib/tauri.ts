// Tauri API wrappers
const tauriCore = (window as any).__TAURI__?.core;
const tauriEvent = (window as any).__TAURI__?.event;

export async function invoke<T = any>(cmd: string, args?: Record<string, any>): Promise<T> {
  return tauriCore.invoke(cmd, args);
}

export async function listen(event: string, handler: (e: any) => void): Promise<() => void> {
  return tauriEvent.listen(event, handler);
}

export function convertFileSrc(path: string): string {
  // Tauri v2: use __TAURI_INTERNALS__ (canonical) or __TAURI__.core (global alias)
  const internals = (window as any).__TAURI_INTERNALS__;
  if (internals?.convertFileSrc) {
    return internals.convertFileSrc(path, "asset");
  }
  if (tauriCore?.convertFileSrc) {
    return tauriCore.convertFileSrc(path);
  }
  return path;
}

export function sourceIconName(source: string): string {
  if (source === "microphone" || source === "mic") return "microphone";
  if (source === "system_audio" || source === "system" || source === "sys") return "monitor";
  return "speaker";
}

export function fmtDuration(s: number): string {
  if (s < 60) return `${s.toFixed(0)}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${Math.floor(s % 60)}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

export function fmtTimer(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60).toString().padStart(2, "0");
  const sec = (s % 60).toString().padStart(2, "0");
  return h > 0 ? `${h}:${m}:${sec}` : `${m}:${sec}`;
}
