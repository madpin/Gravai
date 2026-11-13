// Plain-language costs shown next to settings. Estimates, not measurements.

export function fmtBytes(b: number): string {
  if (!b) return "0 MB";
  if (b < 1048576) return `${(b / 1024).toFixed(0)} KB`;
  if (b < 1073741824) return `${(b / 1048576).toFixed(0)} MB`;
  return `${(b / 1073741824).toFixed(1)} GB`;
}

/** Bytes per hour while recording, and what is kept once the recording stops. */
export function diskPerHour(cfg: any): { recording: number; kept: number } {
  const rec = cfg?.audio?.recording ?? {};
  if (rec.enabled === false) return { recording: 0, kept: 0 };
  const tracks = (cfg?.audio?.microphone?.enabled ? 1 : 0) + (cfg?.audio?.system_audio?.enabled ? 1 : 0);
  const depth = rec.bit_depth === 24 ? 3 : 2;
  const raw = (rec.sample_rate ?? 48000) * depth * (rec.channels ?? 2) * 3600 * tracks;
  if (!rec.auto_compress) return { recording: raw, kept: raw };
  const fmt = rec.export_format ?? "m4a-aac";
  // Compression writes one merged playback file.
  const aac = ((rec.aac_bitrate_kbps ?? 192) * 1000 / 8) * 3600;
  const kept = fmt === "m4a-alac" ? raw * 0.55 : fmt.startsWith("m4a") ? aac : raw;
  return { recording: raw, kept };
}

export function diskLine(cfg: any): string {
  const { recording, kept } = diskPerHour(cfg);
  if (!recording) return "No audio is saved. Transcripts only, no playback.";
  if (kept === recording) return `≈ ${fmtBytes(recording)} per hour of recording.`;
  return `≈ ${fmtBytes(recording)} per hour while recording, ≈ ${fmtBytes(kept)} kept after it stops.`;
}

export function apiHost(url: string): string {
  try { return new URL(url).host || "the API provider"; } catch { return url || "the API provider"; }
}
