// Model files on disk, shared by every picker in Settings and by Library.
import { writable } from "svelte/store";
import { invoke, listen } from "./tauri";
import { modelDownloading } from "./store";

export interface ModelEntry {
  id: string;
  description: string;
  approx_size: number;
  actual_size?: number;
  downloaded: boolean;
  corrupted?: boolean;
  note?: string | null;
  ram_gb?: number;
  speed?: string;
  catalog?: boolean;
  filename?: string;
}

export interface ModelsStatus {
  whisper_models: ModelEntry[];
  silero_vad: ModelEntry;
  ai_models: ModelEntry[];
  embedding_models: ModelEntry[];
  llm_models: ModelEntry[];
  models_dir: string;
}

export const models = writable<ModelsStatus | null>(null);

let listening = false;

export async function loadModels(): Promise<void> {
  try { models.set(await invoke("get_models_status")); } catch (_) {}
  if (!listening) {
    listening = true;
    listen("gravai:model-download", (e: any) => {
      const d = e.payload?.data || e.payload;
      if (d?.status === "complete" || d?.status === "error") setTimeout(loadModels, 1200);
    });
  }
}

/** Start a download. Progress arrives through `modelDownloading`. */
export async function downloadModel(id: string): Promise<void> {
  modelDownloading.update(cur => ({ ...cur, [id]: { progress: 0, status: "starting" } }));
  try {
    await invoke("download_model", { modelId: id });
  } catch (e) {
    modelDownloading.update(cur => { const { [id]: _, ...rest } = cur; return rest; });
    throw e;
  }
}

export async function deleteModelFile(entry: ModelEntry, kind: "llm" | "other"): Promise<void> {
  if (kind === "llm") await invoke("delete_llm_model", { modelId: entry.filename ?? entry.id });
  else await invoke("delete_model", { modelId: entry.id });
  await loadModels();
}

/** Friendly names for catalog ids. */
export function modelName(id: string): string {
  const names: Record<string, string> = {
    "tiny": "Tiny", "base": "Base", "small": "Small", "medium": "Medium",
    "large-v3-turbo": "Large v3 Turbo", "large-v3": "Large v3",
    "qwen3-0.6b": "Qwen3 0.6B", "qwen3-1.7b": "Qwen3 1.7B", "qwen3-4b": "Qwen3 4B", "qwen3-8b": "Qwen3 8B",
    "llama-3.2-3b": "Llama 3.2 3B", "phi3-mini-q4": "Phi-3 Mini", "mistral-7b-q4": "Mistral 7B",
    "all-minilm": "MiniLM", "gemma-embed": "Nomic Embed", "bge-m3": "BGE-M3", "bag-of-words": "Built-in keywords",
    "go-emotions": "Emotion model", "pyannote-segmentation": "Pyannote speakers", "silero_vad": "Silero voice detection",
  };
  return names[id] ?? id;
}
