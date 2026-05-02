# Plan: Replace Ollama with mistral.rs + Managed Model Download

## Goal

Remove the hard dependency on an external Ollama process and replace it with
**mistral.rs** running in-process. Gravai will own the full LLM lifecycle:
download, load, run, and unload models — with no user-installed daemon required.

Two and only two backends will be supported:
- **`local`** — in-process GGUF inference via mistral.rs (Apple Silicon / Metal)
- **`api`** — any OpenAI-compatible external endpoint (OpenAI, Anthropic, custom)

The `"ollama"` provider is removed entirely (no compatibility shim, no deprecation
path — it is just deleted).

---

## Current State (what we are replacing)

- **HTTP client**: raw `reqwest` POST to `{base_url}/chat/completions` (OpenAI-compatible).
- **Default endpoint**: `http://localhost:11434/v1` (Ollama).
- **Default model**: `gemma3:4b` (pulled by the user via `ollama pull`).
- **Three call-sites** using `LlmClient`:
  1. `gravai-intelligence/src/summarization/mod.rs` — session summaries
  2. `gravai-intelligence/src/chat.rs` — RAG "Ask Gravai"
  3. `gravai-intelligence/src/correction/mod.rs` — transcript correction
- **Config**: `LlmConfig { provider, base_url, model, api_key, max_tokens }` in
  `gravai-config/src/lib.rs`.
- **Profile overrides**: `llm_provider`, `llm_model` in `gravai-config/src/profiles.rs`.
- **Model download infra**: already exists in `src-tauri/src/commands/models.rs`
  for Whisper, embeddings, sentiment, and diarization — GGUF LLMs slot into the
  same system.

---

## Target State

| Aspect | Before | After |
|--------|--------|-------|
| Runtime dependency | External `ollama serve` | None (in-process mistral.rs) |
| Providers | `"ollama"` / `"openai"` / `"anthropic"` | `"local"` / `"api"` |
| Local model format | Ollama model names (`gemma3:4b`) | GGUF files in `~/.gravai/models/llm/` |
| Local model acquisition | `ollama pull` by user | Gravai download manager |
| Local inference | HTTP to Ollama process | Direct Rust calls into mistral.rs engine |
| External API | Hard-coded Ollama base URL | User-supplied `base_url` + `api_key` |

---

## Integration Strategy

Add `mistralrs` as a workspace dependency. Call the engine directly from async
Rust — no subprocess, no sockets, no HTTP round-trips for local inference.
Models are loaded lazily on first use and held in `AppState` until the user
switches models or unloads them.

```toml
# workspace Cargo.toml
[dependencies]
mistralrs = { version = "0.4", default-features = false, features = ["metal"] }
# metal = Apple Silicon GPU; adjust if/when Linux support is added
```

Verify the correct feature flag for the pinned version before implementing.

---

## Implementation Plan

### Phase 1 — LLM model catalog + download

**1.1 `gravai-models` crate — new `llm_models.rs`**

```rust
pub struct LlmModelInfo {
    pub id: &'static str,           // "gemma3-4b-q4"
    pub display_name: &'static str,
    pub filename: &'static str,     // "gemma-3-4b-it-q4_k_m.gguf"
    pub hf_repo: &'static str,      // "google/gemma-3-4b-it-gguf"
    pub size_mb: u64,
    pub ram_required_gb: f32,
    pub context_length: u32,        // tokens, for truncation decisions
}

pub const LLM_MODELS: &[LlmModelInfo] = &[
    LlmModelInfo {
        id: "gemma3-4b-q4",
        display_name: "Gemma 3 4B (Q4, ~3 GB RAM)",
        filename: "gemma-3-4b-it-q4_k_m.gguf",
        hf_repo: "google/gemma-3-4b-it-gguf",
        size_mb: 2_490,
        ram_required_gb: 3.0,
        context_length: 8192,
    },
    LlmModelInfo {
        id: "phi3-mini-q4",
        display_name: "Phi-3 Mini (Q4, ~2.4 GB RAM)",
        filename: "Phi-3-mini-4k-instruct-q4.gguf",
        hf_repo: "microsoft/Phi-3-mini-4k-instruct-gguf",
        size_mb: 2_300,
        ram_required_gb: 2.5,
        context_length: 4096,
    },
    LlmModelInfo {
        id: "mistral-7b-q4",
        display_name: "Mistral 7B (Q4, ~4.5 GB RAM)",
        filename: "mistral-7b-instruct-v0.3.Q4_K_M.gguf",
        hf_repo: "mistralai/Mistral-7B-Instruct-v0.3-GGUF",
        size_mb: 4_370,
        ram_required_gb: 4.5,
        context_length: 32768,
    },
];
```

**1.2 `src-tauri/src/commands/models.rs` — extend download command**

Add a new arm to `download_model()` for `model_type == "llm"`:
- Download GGUF to `~/.gravai/models/llm/{filename}`.
- Reuse existing progress-event infrastructure (`gravai:model-download`).
- Add `list_llm_models()` Tauri command returning catalog + per-model
  `{ downloaded: bool, path: Option<String> }`.
- Add `delete_llm_model(id)` Tauri command.

---

### Phase 2 — `MistralRsClient` in `gravai-intelligence`

**2.1 New file: `crates/gravai-intelligence/src/llm_local.rs`**

```rust
use mistralrs::{
    GGUFLoaderBuilder, GGUFSpecificConfig, MistralRs, MistralRsBuilder,
    NormalRequest, Request, RequestMessage, ResponseOk, SamplingParams,
};

pub struct LocalLlmClient {
    engine: Arc<MistralRs>,
}

impl LocalLlmClient {
    pub async fn new(model_path: &Path) -> Result<Self, String> {
        // Build GGUF loader from model_path
        // Select Metal device for Apple Silicon
        // Wrap in MistralRsBuilder, call build()
    }

    pub async fn chat(
        &self,
        messages: &[serde_json::Value],
        max_tokens: u32,
        temperature: f64,
    ) -> Result<String, String> {
        // Convert messages JSON → Vec<RequestMessage>
        // Build NormalRequest with SamplingParams
        // Submit to engine, await ResponseOk
        // Return content string
    }
}
```

**2.2 Refactor `LlmClient` in `crates/gravai-intelligence/src/llm_client.rs`**

Replace the current single-path struct with a backend enum:

```rust
pub enum LlmClient {
    Local(LocalLlmClient),
    Api(ApiLlmClient),   // renamed from current LlmClient — reqwest HTTP
}

impl LlmClient {
    pub async fn from_config(config: &LlmConfig, models_dir: &Path) -> Result<Self, String> {
        match config.provider.as_str() {
            "local" => {
                let path = models_dir.join("llm").join(&config.model_filename()?);
                Ok(LlmClient::Local(LocalLlmClient::new(&path).await?))
            }
            "api" => Ok(LlmClient::Api(ApiLlmClient::new(config))),
            other => Err(format!("Unknown LLM provider: {other}")),
        }
    }

    pub async fn chat(
        &self,
        messages: &[serde_json::Value],
        max_tokens: u32,
        temperature: f64,
    ) -> Result<String, String> {
        match self {
            LlmClient::Local(c) => c.chat(messages, max_tokens, temperature).await,
            LlmClient::Api(c)   => c.chat(messages, max_tokens, temperature).await,
        }
    }
}
```

The three existing call-sites remain **unchanged** — they call `client.chat(...)`.

---

### Phase 3 — Config changes

**3.1 `gravai-config/src/lib.rs` — replace `LlmConfig`**

```rust
pub struct LlmConfig {
    /// "local" or "api"
    pub provider: String,
    // --- local fields (provider == "local") ---
    /// Model id from LLM_MODELS catalog, e.g. "gemma3-4b-q4"
    pub local_model_id: String,
    // --- api fields (provider == "api") ---
    /// Full base URL, e.g. "https://api.openai.com/v1"
    pub api_base_url: String,
    /// Model name as the API expects, e.g. "gpt-4o-mini"
    pub api_model: String,
    pub api_key: Option<String>,
    // --- shared ---
    pub max_tokens: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "local".into(),
            local_model_id: "gemma3-4b-q4".into(),
            api_base_url: "https://api.openai.com/v1".into(),
            api_model: "gpt-4o-mini".into(),
            api_key: None,
            max_tokens: 2048,
        }
    }
}
```

**3.2 Config migration** (bump `AppConfig.version`):

```rust
// version N-1 → N
if old.llm.provider == "ollama" || old.llm.provider.is_empty() {
    new.llm.provider = "local".into();
    // best-effort name remap
    new.llm.local_model_id = match old.llm.model.as_str() {
        m if m.contains("gemma") => "gemma3-4b-q4",
        m if m.contains("phi")   => "phi3-mini-q4",
        m if m.contains("mistral") => "mistral-7b-q4",
        _ => "gemma3-4b-q4",
    }.into();
}
if old.llm.provider == "openai" || old.llm.provider == "anthropic" {
    new.llm.provider = "api".into();
    new.llm.api_base_url = old.llm.base_url.clone();
    new.llm.api_model    = old.llm.model.clone();
    new.llm.api_key      = old.llm.api_key.clone();
}
```

**3.3 Profile overrides** (`gravai-config/src/profiles.rs`):

Replace `llm_provider` + `llm_model` fields with:
```rust
pub llm_provider: Option<String>,       // "local" or "api"
pub llm_local_model_id: Option<String>,
pub llm_api_model: Option<String>,
```

---

### Phase 4 — Engine lifecycle in `AppState`

The mistral.rs engine takes ~2–4 s to load on Apple Silicon. Strategy:

- Store `Option<Arc<LlmClient>>` in `AppState`.
- Load **lazily** on first call to summarization / chat / correction; hold for the
  app lifetime.
- Reload when `local_model_id` changes (user selects different model in Settings).
- Add Tauri command `unload_llm_model()` to free VRAM/RAM before a recording session
  on memory-constrained hardware.
- Emit `gravai:llm-status` events (`loading | first_run | ready | unloaded | error`)
  so the frontend can show a spinner or warning.
  - **Implemented**: `gravai_core::GravaiEvent::LlmStatus { state, model_id, message }`,
    bridged to the frontend in `src-tauri/src/lib.rs` and rendered by
    `LlmStatusBanner.svelte` (Recording + Chat) and the `StatusBar` pill.
    The `first_run` state is emitted when no UQFF cache shards are found,
    explaining the multi-minute wait the first time a model is used.

---

### Phase 5 — Frontend

**5.1 Settings — LLM section**

Replace the current Ollama URL + model-name text fields with two tabs/modes:

**Local tab:**
- Table of GGUF models from `list_llm_models()`: name, size, RAM, download status.
- Download / Delete buttons (same UX as Whisper models).
- Radio to select active model.
- Engine status indicator (loading / ready / unloaded).
- "Unload model" button.

**API tab:**
- Base URL input (default `https://api.openai.com/v1`).
- Model name input.
- API key input (masked).
- Test connection button.

**5.2 First-run / no-model state**

If `provider == "local"` and no model is downloaded, show an inline banner
in the AI features area: "No local model downloaded — go to Settings → LLM to
download one, or switch to an external API."

**5.3 Profile UI**

`llm_local_model_id` shows a dropdown of downloaded GGUF models.
`llm_api_model` remains a free-text field (API model names vary).

---

## Files Changed Summary

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add `mistralrs` dependency |
| `crates/gravai-models/src/llm_models.rs` | New: LLM model catalog |
| `crates/gravai-models/src/lib.rs` | Export `llm_models` |
| `crates/gravai-intelligence/src/llm_local.rs` | New: LocalLlmClient (mistral.rs) |
| `crates/gravai-intelligence/src/llm_client.rs` | Rewrite: Backend enum, remove Ollama |
| `crates/gravai-intelligence/src/lib.rs` | Export new module |
| `crates/gravai-config/src/lib.rs` | Rewrite LlmConfig, bump version, migration |
| `crates/gravai-config/src/profiles.rs` | Update profile LLM fields |
| `src-tauri/src/commands/models.rs` | Add LLM download / list / delete commands |
| `src-tauri/src/lib.rs` | Register commands, engine lifecycle in AppState |
| `src-frontend/src/routes/settings/` | New LLM settings UI (local + api tabs) |
| `src-frontend/src/routes/recording/` | Replace Ollama warning with LLM-status banner |

---

## Open Questions Before Starting

1. **mistral.rs crate API**: Pin to a specific version. The library API (not the
   CLI) changed significantly between 0.3 and 0.4. Audit `GGUFLoaderBuilder` and
   `MistralRsBuilder` signatures against the chosen version before writing code.

2. **Metal feature flag**: May be `metal`, `accelerate`, or `candle-metal` — check
   the pinned version's `Cargo.toml` features list.

3. **Context window & transcript truncation**: Correction prompts include raw
   transcript text that can easily exceed 4k tokens. Add a truncation helper that
   trims to `model.context_length - system_prompt_tokens - response_budget` before
   sending. Phi-3 Mini (4k context) may be too small for correction; prefer Gemma
   or Mistral as default.

4. **Concurrent requests**: Summarization, correction, and chat can fire
   simultaneously. mistral.rs has an internal request queue — verify it handles
   concurrent `chat()` calls safely without deadlock.

5. **First-run download size**: Smallest usable model is ~2.3 GB. Consider
   whether to gate AI features entirely or show degraded placeholders until a
   model is ready.

---

## Suggested Implementation Order

1. **Phase 1** — Model catalog + download commands (isolated, no inference code)
2. **Phase 3.1 + 3.2** — Config rewrite + migration (keeps app runnable with API fallback)
3. **Phase 2** — LocalLlmClient + Backend enum (local inference end-to-end)
4. **Phase 4** — Engine lifecycle + Tauri commands
5. **Phase 5** — Frontend UI
6. **Phase 3.3** — Profile field updates + profile UI
