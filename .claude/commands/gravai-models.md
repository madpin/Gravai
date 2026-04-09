---
description: Guide to gravai-models and gravai-transcription crates — model download, Whisper engine, VAD models
allowed-tools: Read, Glob, Grep
---

You are helping with the Gravai models system. This covers `crates/gravai-models/` (download management) and `crates/gravai-transcription/` (Whisper transcription engine).

## Model Storage Location
All models stored at `~/.gravai/models/` (or `~/.gravai-dev/models/` in debug):
```
~/.gravai/models/
├── ggml-tiny.bin          # 39MB Whisper
├── ggml-base.bin          # 74MB Whisper
├── ggml-small.bin         # 244MB Whisper
├── ggml-medium.bin        # 769MB Whisper
├── ggml-large-v3-turbo.bin # ~800MB Whisper (recommended)
├── ggml-large-v3.bin      # 1.5GB Whisper (highest quality)
├── silero_vad.onnx        # ~2MB VAD
├── distilbert_sentiment.onnx  # ~67MB (if sentiment enabled)
├── pyannote_diarization.onnx  # (if pyannote diarization enabled)
└── all-minilm-l6-v2.onnx     # ~22MB embeddings (or larger models)
```

## `gravai-models` Crate
### Key Functions
```rust
// Called at startup — downloads missing essential models
ensure_models(config: &AppConfig, event_bus: &EventBus) -> Result<()>

// Internal helper — skips if file exists, downloads atomically
download_if_missing(url: &str, dest: &Path, model_id: &str, event_bus: &EventBus) -> Result<()>
```
- **Atomic download**: writes to `.tmp` file, renames on success (no corrupt partial files)
- **Progress events**: publishes `GravaiEvent::DownloadProgress { model_id, downloaded, total }` during download
- Sources: HuggingFace (Whisper GGML models), GitHub releases (Silero VAD ONNX)
- Startup behavior: spawned as background thread, non-blocking

### Model IDs (used in Tauri commands)
| ID | File | Size | Source |
|----|------|------|--------|
| `"whisper-tiny"` | `ggml-tiny.bin` | 39MB | HuggingFace |
| `"whisper-base"` | `ggml-base.bin` | 74MB | HuggingFace |
| `"whisper-small"` | `ggml-small.bin` | 244MB | HuggingFace |
| `"whisper-medium"` | `ggml-medium.bin` | 769MB | HuggingFace |
| `"whisper-large-v3-turbo"` | `ggml-large-v3-turbo.bin` | ~800MB | HuggingFace |
| `"whisper-large-v3"` | `ggml-large-v3.bin` | 1.5GB | HuggingFace |
| `"silero-vad"` | `silero_vad.onnx` | ~2MB | GitHub |
| `"sentiment"` | `distilbert_sentiment.onnx` | ~67MB | HuggingFace |
| `"all-minilm"` | `all-minilm-l6-v2.onnx` | ~22MB | HuggingFace |
| `"gemma-embed"` | `gemma-embed.onnx` | ~274MB | HuggingFace |
| `"bge-m3"` | `bge-m3.onnx` | ~572MB | HuggingFace |

## `gravai-transcription` Crate

### `TranscriptionProvider` Trait
```rust
pub trait TranscriptionProvider: Send {
    async fn transcribe(&self, samples: &[f32]) -> Result<Vec<TranscriptionSegment>>;
}

pub struct TranscriptionSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub confidence: f32,
    pub language: Option<String>,
}
```

### `WhisperEngine` (`whisper.rs`)
```rust
pub struct WhisperEngine {
    model_path: PathBuf,  // ~/.gravai/models/ggml-{model}.bin
    config: TranscriptionConfig,
}
```
- Wraps `whisper-rs` crate (Rust bindings to whisper.cpp ONNX)
- Input: 16kHz mono `f32` samples (from resampler)
- Output: `Vec<TranscriptionSegment>` with word-level timestamps
- Model loaded lazily on first transcription call
- Hallucination filtering: strips phrases matching `config.hallucination_blocklist`

### `HttpStubProvider` (`http_stub.rs`)
- Fallback for testing — POSTs audio to an HTTP endpoint
- Selected when `config.transcription.engine = "http-stub"`

### Factory
```rust
pub fn create_provider(config: &TranscriptionConfig) -> Box<dyn TranscriptionProvider>
```
Routes to `WhisperEngine` (default) or `HttpStubProvider` based on `config.engine`.

## Tauri Commands (Models Page)
```typescript
// Get status of all models
const status = await invoke<ModelsStatus>("get_models_status");
// Returns: whisper models list, silero status, AI models, embedding models
// Each: { id, name, downloaded: bool, size_bytes, path }

// Trigger download (emits gravai:model-download progress events)
await invoke("download_model", { modelId: "whisper-base" });
```

## Frontend Model Download Flow
1. User clicks Download on Models page
2. `invoke("download_model", { modelId })` → starts background download
3. Backend publishes `DownloadProgress` events → bridge emits `"gravai:model-download"`
4. Frontend `modelDownloading` store tracks progress, shows progress bar
5. On completion, `DownloadProgress { downloaded == total }` → mark complete
6. `get_models_status()` re-queried to refresh UI

---

Now answer the user's question about Gravai models and transcription: $ARGUMENTS
