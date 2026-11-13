// The live configuration: what the next recording uses.
// Every settings control reads this store and writes through patchConfig.
import { writable, get } from "svelte/store";
import { invoke, listen } from "./tauri";

export const config = writable<any>(null);

let listening = false;

/** Load the config once and keep it fresh when anything else writes it. */
export async function loadConfig(): Promise<any> {
  const cfg = await invoke("get_config");
  config.set(cfg);
  if (!listening) {
    listening = true;
    listen("gravai:config-changed", async () => config.set(await invoke("get_config")));
  }
  return cfg;
}

/** Deep-merge `patch` into the live config and save it. */
export async function patchConfig(patch: any): Promise<any> {
  const cfg = await invoke("update_config", { patch });
  config.set(cfg);
  return cfg;
}

/** Read a dotted path, e.g. get("features.diarization.enabled"). */
export function pick(cfg: any, path: string): any {
  return path.split(".").reduce((o, k) => (o == null ? o : o[k]), cfg);
}

/** Build a nested patch object from a dotted path. */
export function patchFor(path: string, value: any): any {
  return path.split(".").reduceRight((acc, k) => ({ [k]: acc }), value);
}

export function setValue(path: string, value: any): Promise<any> {
  return patchConfig(patchFor(path, value));
}

export const LANGUAGES: Array<[string, string]> = [
  ["auto", "Detect automatically"],
  ["en", "English"], ["pt", "Portuguese"], ["es", "Spanish"], ["fr", "French"],
  ["de", "German"], ["it", "Italian"], ["nl", "Dutch"], ["pl", "Polish"],
  ["ja", "Japanese"], ["zh", "Chinese"],
];

export function languageName(code: string): string {
  if (code === "auto") return "Auto language";
  return LANGUAGES.find(([c]) => c === code)?.[1] ?? code;
}

/** One line describing what the next recording runs. Mirrors Snapshot::summary in Rust. */
export function runSummary(cfg: any): string {
  if (!cfg) return "";
  const t = cfg.transcription ?? {};
  const llm = cfg.llm ?? {};
  const parts = [
    `Whisper ${t.model ?? "medium"}`,
    languageName(t.language ?? "en"),
    llm.provider === "api" ? `${llm.model || "API model"} (API)` : llm.local_model || "qwen3-4b",
  ];
  const mic = cfg.audio?.microphone?.enabled ?? true;
  const sys = cfg.audio?.system_audio?.enabled ?? true;
  if (!mic || !sys) parts.push(mic ? "Mic only" : sys ? "System audio only" : "No audio sources");
  if (cfg.features?.diarization?.enabled) parts.push("Speakers");
  if (cfg.features?.sentiment?.enabled) parts.push("Emotion");
  if (cfg.correction?.enabled) parts.push("Correction");
  return parts.join(" · ");
}

export function currentConfig(): any {
  return get(config);
}
