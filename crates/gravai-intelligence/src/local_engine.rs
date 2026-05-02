//! Local LLM engine — in-process inference via mistral.rs.
//!
//! Two loading paths:
//! - **GGUF**: Pre-quantized `.gguf` files (Qwen, Llama, Phi, Mistral).
//! - **HF-ISQ**: HuggingFace models quantized on-the-fly with UQFF caching (Gemma 4).
//!   Gemma 4 GGUF architecture is not supported by mistral.rs 0.8.x, but the
//!   MultimodalModelBuilder + ISQ path works. First load downloads from HF and
//!   quantizes (~minutes); subsequent loads use the cached UQFF file (~seconds).

use std::sync::Arc;

use mistralrs::{
    ChatCompletionResponse, GgufModelBuilder, IsqType, Model, MultimodalModelBuilder,
    RequestBuilder, TextMessageRole, TextMessages, UqffMultimodalModelBuilder,
};
use tokio::sync::RwLock;
use tracing::info;

/// How to load a model.
enum ModelSource {
    /// Pre-quantized GGUF file in ~/.gravai/models/llm/
    Gguf { filename: String },
    /// HuggingFace model with on-the-fly ISQ + local UQFF cache
    HfIsq { hf_repo: String },
}

/// Wrapper around a loaded mistral.rs model.
///
/// `Model` is `Send + Sync`; mistral.rs handles request scheduling internally.
pub struct LocalLlmEngine {
    model: Model,
    model_id: String,
}

impl LocalLlmEngine {
    async fn build_model(source: ModelSource, model_id: &str) -> Result<Self, String> {
        let mid = model_id.to_string();

        let load_fut = match source {
            ModelSource::Gguf { filename } => {
                let models_dir = gravai_config::models_dir()
                    .join("llm")
                    .to_string_lossy()
                    .to_string();
                info!("Loading GGUF LLM: {model_id} from {models_dir}/{filename}");

                tokio::task::spawn(async move {
                    // PagedAttention disabled: mistral.rs 0.8.1 Metal shaders have a
                    // function_constant index conflict that causes "channel closed" errors.
                    GgufModelBuilder::new(models_dir, vec![filename])
                        .with_max_num_seqs(1)
                        .with_prefix_cache_n(Some(16))
                        .build()
                        .await
                })
            }
            ModelSource::HfIsq { hf_repo } => {
                let uqff_dir = gravai_config::models_dir().join("llm").join("uqff");
                let _ = std::fs::create_dir_all(&uqff_dir);
                // mistral.rs writes UQFF as sharded files: `<base>-<idx>.uqff`.
                // The unsharded `<base>.uqff` path is what we pass to `write_uqff`,
                // but it never exists on disk afterwards — the cache check must
                // scan for the sharded files.
                let uqff_write_path = uqff_dir.join(format!("{mid}.uqff"));
                let cached_shards: Vec<std::path::PathBuf> = collect_uqff_shards(&uqff_dir, &mid);
                let mid_for_log = mid.clone();

                info!("Loading HF-ISQ LLM: {model_id} ({hf_repo})");

                tokio::task::spawn(async move {
                    if !cached_shards.is_empty() {
                        info!(
                            "Loading from cached UQFF: {} shard(s) for {mid_for_log}",
                            cached_shards.len()
                        );
                        UqffMultimodalModelBuilder::new(&hf_repo, cached_shards)
                            .into_inner()
                            .with_max_num_seqs(1)
                            .with_prefix_cache_n(Some(16))
                            .build()
                            .await
                    } else {
                        info!("ISQ from HuggingFace (first run — will cache as UQFF)");
                        MultimodalModelBuilder::new(&hf_repo)
                            .with_isq(IsqType::Q4K)
                            .write_uqff(uqff_write_path)
                            .with_max_num_seqs(1)
                            .with_prefix_cache_n(Some(16))
                            .build()
                            .await
                    }
                })
            }
        };

        // 10 min timeout for HF-ISQ first run, plenty for GGUF too.
        let model = match tokio::time::timeout(std::time::Duration::from_secs(600), load_fut).await
        {
            Ok(join_result) => join_result
                .map_err(|e| {
                    format!(
                        "LLM engine panicked loading {mid}: {e}. \
                         The model architecture may not be supported."
                    )
                })?
                .map_err(|e| format!("Failed to load LLM {mid}: {e}"))?,
            Err(_) => {
                return Err(format!(
                    "LLM engine timed out loading {model_id} after 600s."
                ));
            }
        };

        info!("Local LLM engine ready: {model_id}");
        Ok(Self {
            model,
            model_id: model_id.to_string(),
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Run chat completion with OpenAI-style messages.
    pub async fn chat(
        &self,
        messages: &[serde_json::Value],
        max_tokens: u32,
        temperature: f64,
    ) -> Result<String, String> {
        let mut text_messages = TextMessages::new();

        for msg in messages {
            let role_str = msg["role"].as_str().unwrap_or("user");
            let content = msg["content"].as_str().unwrap_or("");
            let role = match role_str {
                "system" => TextMessageRole::System,
                "assistant" => TextMessageRole::Assistant,
                "user" => TextMessageRole::User,
                other => TextMessageRole::Custom(other.to_string()),
            };
            text_messages = text_messages.add_message(role, content);
        }

        let request = RequestBuilder::from(text_messages)
            .set_sampler_temperature(temperature)
            .set_sampler_max_len(max_tokens as usize);

        let response: ChatCompletionResponse = self
            .model
            .send_chat_request(request)
            .await
            .map_err(|e| format!("Local LLM inference error: {e}"))?;

        info!(
            "LLM inference: {:.1} prompt tok/s, {:.1} completion tok/s",
            response.usage.avg_prompt_tok_per_sec, response.usage.avg_compl_tok_per_sec,
        );

        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.as_ref())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        if content.is_empty() {
            return Err("Local LLM returned empty response".into());
        }

        Ok(content)
    }
}

/// Find UQFF shard files for a given model id in the cache directory.
/// `mistral.rs` writes UQFF as `<mid>-<idx>.uqff` (one or more shards), even
/// when called with an unsharded target path. Sorted ascending by shard index
/// so loading order is deterministic.
fn collect_uqff_shards(uqff_dir: &std::path::Path, mid: &str) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(uqff_dir) else {
        return Vec::new();
    };
    let prefix = format!("{mid}-");
    let mut shards: Vec<(u32, std::path::PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            // Match `<mid>-<idx>.uqff`
            let rest = name.strip_prefix(&prefix)?;
            let idx_str = rest.strip_suffix(".uqff")?;
            let idx: u32 = idx_str.parse().ok()?;
            Some((idx, path))
        })
        .collect();
    shards.sort_by_key(|(idx, _)| *idx);
    shards.into_iter().map(|(_, p)| p).collect()
}

// ── Global singleton engine ──────────────────────────────────────────────────

static ENGINE: std::sync::OnceLock<RwLock<Option<Arc<LocalLlmEngine>>>> =
    std::sync::OnceLock::new();

fn engine_lock() -> &'static RwLock<Option<Arc<LocalLlmEngine>>> {
    ENGINE.get_or_init(|| RwLock::new(None))
}

// ── Event bus for status updates ─────────────────────────────────────────────
//
// We expose a global `OnceLock<EventBus>` so the engine can publish
// `LlmStatus` events without callers having to thread the bus through every
// `chat()` call. The Tauri app sets this once at startup. If unset, status
// emission is a no-op (handy for tests / CLI use).

static EVENT_BUS: std::sync::OnceLock<gravai_core::EventBus> = std::sync::OnceLock::new();

/// Register the global event bus once at app startup. Subsequent calls are
/// no-ops (the bus is fixed for the lifetime of the process).
pub fn set_event_bus(bus: gravai_core::EventBus) {
    let _ = EVENT_BUS.set(bus);
}

fn emit_status(state: &str, model_id: &str, message: Option<String>) {
    emit_status_full(state, model_id, message, None, None, None);
}

#[allow(clippy::too_many_arguments)]
fn emit_status_full(
    state: &str,
    model_id: &str,
    message: Option<String>,
    progress: Option<f32>,
    phase: Option<String>,
    eta_seconds: Option<u64>,
) {
    if let Some(bus) = EVENT_BUS.get() {
        bus.publish(gravai_core::GravaiEvent::LlmStatus {
            state: state.to_string(),
            model_id: model_id.to_string(),
            message,
            progress,
            phase,
            eta_seconds,
        });
    }
}

// ── Progress estimation ──────────────────────────────────────────────────────
//
// mistral.rs does not expose progress callbacks for either the HuggingFace
// download (~5–16 GB) or the in-situ quantization pass that follows. Rather
// than leave the user staring at a frozen UI for several minutes, we run a
// lightweight ticker that combines two signals:
//
// 1. **Real bytes downloaded** — we poll the HF blobs directory for the model
//    repo. While `mistralrs_core::pipeline::hf` is fetching weights, the
//    blob files grow on disk. We treat the download as done as soon as
//    *either* the cache hits ~95 % of the expected total *or* the size has
//    been stable for a few seconds (so the bar never gets stuck waiting for
//    a few-MB metadata file or a slightly-off size estimate).
// 2. **Time-based heuristic** — once the download is complete (or for cached
//    UQFF loads where there is no download), we advance a smooth time-based
//    bar through phase labels: "Loading weights" → "Quantizing" →
//    "Saving cache" → "Warming up". The post-download timer is anchored at
//    the moment the download phase ends, so a fully-cached repo doesn't sit
//    at 50 % during the entire quantize+save phase.
//
// The bar never exceeds 0.95 from the ticker — only the final `ready` event
// snaps to 1.0. This keeps us from lying to the user when an underlying
// step takes longer than the heuristic expected.

/// Approximate full HF-cache size (in bytes, decimal) for a model. Slightly
/// *under* the real total so off-by-a-few-MB metadata variations don't keep
/// us pinned in the download phase forever. Combined with the stable-size
/// watchdog below, exact values aren't critical.
fn expected_hf_cache_bytes(model_id: &str) -> Option<u64> {
    match model_id {
        "gemma-4-e2b" => Some(9_500_000_000),
        "gemma-4-e4b" => Some(15_500_000_000),
        _ => None,
    }
}

/// Total size of every regular file inside a directory tree (1 level deep is
/// enough for HF blobs/). Returns 0 if the directory does not exist.
fn dir_size_bytes(path: &std::path::Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(path) else {
        return 0;
    };
    rd.flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

fn hf_blobs_dir(model_id: &str) -> Option<std::path::PathBuf> {
    let repo = match model_id {
        "gemma-4-e2b" => "models--google--gemma-4-E2B-it",
        "gemma-4-e4b" => "models--google--gemma-4-E4B-it",
        _ => return None,
    };
    let home = std::env::var_os("HOME")?;
    Some(
        std::path::PathBuf::from(home)
            .join(".cache/huggingface/hub")
            .join(repo)
            .join("blobs"),
    )
}

fn typical_eta_seconds(is_first_run: bool, model_id: &str) -> u64 {
    if is_first_run {
        match model_id {
            "gemma-4-e4b" => 360,
            _ => 240,
        }
    } else {
        50
    }
}

/// Mutable state owned by the progress ticker between ticks.
#[derive(Debug)]
struct ProgressState {
    /// `Some(t)` once we've decided the HF download phase is finished.
    /// `None` while still downloading (or before first tick).
    download_done_at: Option<std::time::Instant>,
    /// Largest cache size observed so far.
    last_dl_bytes: u64,
    /// When `last_dl_bytes` last changed. If it stays stable for a few
    /// seconds we conclude the download has finished.
    last_dl_changed_at: std::time::Instant,
    /// `true` if we ever spent time visibly downloading. Determines whether
    /// the bar reserves the lower half for download (true) or uses the full
    /// 0–95 % range for the post-download work (false, e.g. fully-cached
    /// repo on first ISQ run).
    had_download_phase: bool,
}

impl ProgressState {
    fn new(now: std::time::Instant) -> Self {
        Self {
            download_done_at: None,
            last_dl_bytes: 0,
            last_dl_changed_at: now,
            had_download_phase: false,
        }
    }
}

/// Compute (phase, progress) for a single tick. Called every ~1 s by the
/// ticker task; mutates `state` to track the download watchdog across calls.
fn estimate_progress(
    state: &mut ProgressState,
    started: std::time::Instant,
    eta_total: u64,
    is_first_run: bool,
    model_id: &str,
) -> (String, f32) {
    let now = std::time::Instant::now();

    // Phase 1 — download. First-run only and only until we mark it done.
    if is_first_run && state.download_done_at.is_none() {
        if let Some(blobs) = hf_blobs_dir(model_id) {
            let cur = dir_size_bytes(&blobs);

            // Watchdog: track the largest size we've seen and when it grew.
            if cur > state.last_dl_bytes {
                state.last_dl_bytes = cur;
                state.last_dl_changed_at = now;
            }

            let by_size = match expected_hf_cache_bytes(model_id) {
                Some(exp) => cur as f64 / exp as f64 >= 0.95,
                None => false,
            };
            // Stable for 5+ seconds AND we have *some* data → download done.
            // We require `cur > 0` so we don't immediately bail before the
            // download has actually started.
            let stable = cur > 0
                && now.duration_since(state.last_dl_changed_at)
                    > std::time::Duration::from_secs(5);

            if by_size || stable {
                state.download_done_at = Some(now);
                // If this is the very first observation and the cache is
                // already full, we never actually saw a download phase from
                // the user's POV — keep `had_download_phase` false so the
                // post-download phase gets the full 0–95 % range.
                if now.duration_since(started) >= std::time::Duration::from_secs(2) {
                    state.had_download_phase = true;
                }
            } else {
                state.had_download_phase = true;
                let expected = expected_hf_cache_bytes(model_id).unwrap_or(0);
                let frac = if expected > 0 {
                    (cur as f32 / expected as f32).clamp(0.0, 0.50)
                } else {
                    0.0
                };
                let label = if expected > 0 {
                    format!(
                        "Downloading model weights ({:.1} / {:.1} GB)",
                        cur as f64 / 1.0e9,
                        expected as f64 / 1.0e9
                    )
                } else {
                    "Downloading model weights".into()
                };
                return (label, frac);
            }
        } else {
            // Unknown repo layout — skip the download phase entirely.
            state.download_done_at = Some(now);
        }
    }

    // Phase 2+ — post-download (first-run) or full load (cached). Time-based,
    // anchored at the moment we entered this phase so a fully-cached repo
    // doesn't waste the first 30 % of the eta.
    let phase_started = state.download_done_at.unwrap_or(started);
    let phase_elapsed = now.duration_since(phase_started).as_secs_f32();
    // Reserve ~60 s of the eta budget for the download phase on first run
    // *if* we actually had one; otherwise let the post-download work claim
    // the full eta budget so the bar doesn't crawl artificially.
    let phase_eta = if is_first_run && state.had_download_phase {
        eta_total.saturating_sub(60).max(60) as f32
    } else {
        eta_total.max(1) as f32
    };
    let phase_frac = (phase_elapsed / phase_eta).clamp(0.0, 1.0);

    // The bar's lower half is reserved for the download phase only when one
    // actually happened. A fully-cached repo gets the full 0–95 % range for
    // its quant + save + warmup phases, which is much less misleading than
    // sitting at 50 % for the entire 5-minute ISQ run.
    let (base, span) = if is_first_run && state.had_download_phase {
        (0.50, 0.45)
    } else {
        (0.0, 0.95)
    };
    let progress = base + span * phase_frac;

    let label = if is_first_run {
        if phase_frac < 0.40 {
            "Quantizing weights"
        } else if phase_frac < 0.80 {
            "Saving quantized cache"
        } else {
            "Warming up"
        }
    } else if phase_frac < 0.30 {
        "Loading model files"
    } else if phase_frac < 0.85 {
        "Loading weights to GPU"
    } else {
        "Warming up"
    };

    (label.into(), progress)
}

/// Get the currently loaded engine, or load it if not loaded / model changed.
///
/// Emits `LlmStatus` events around the load so the frontend can show progress:
/// - `loading` when a cached / fast load is starting (GGUF or cached UQFF)
/// - `first_run` when the slow ISQ path is needed (download + quantize, minutes)
/// - `ready` when the engine is usable
/// - `error` on failure (with the error string in `message`)
pub async fn get_or_load_engine(model_id: &str) -> Result<Arc<LocalLlmEngine>, String> {
    {
        let guard = engine_lock().read().await;
        if let Some(ref engine) = *guard {
            if engine.model_id() == model_id {
                return Ok(Arc::clone(engine));
            }
        }
    }

    let mut guard = engine_lock().write().await;

    if let Some(ref engine) = *guard {
        if engine.model_id() == model_id {
            return Ok(Arc::clone(engine));
        }
        info!(
            "Swapping local LLM from {} to {}",
            engine.model_id(),
            model_id
        );
    }

    // Drop old engine to free GPU memory
    *guard = None;

    let source = resolve_model_source(model_id)?;

    // Decide which status to emit based on whether we expect a fast or slow load.
    let is_first_run = matches!(&source, ModelSource::HfIsq { .. })
        && collect_uqff_shards(
            &gravai_config::models_dir().join("llm").join("uqff"),
            model_id,
        )
        .is_empty();

    let eta = typical_eta_seconds(is_first_run, model_id);

    // Compute the first progress sample synchronously so the very first event
    // shows real values (e.g. when the HF download is already complete, the
    // bar jumps straight to "Quantizing 50 %" instead of flashing 0 % first).
    let started = std::time::Instant::now();
    let mut initial_state = ProgressState::new(started);
    let (initial_phase, initial_progress) =
        estimate_progress(&mut initial_state, started, eta, is_first_run, model_id);

    let initial_message = if is_first_run {
        Some(
            "First use of this model — downloading and quantizing. \
             This takes ~2–5 minutes depending on the model size and is \
             cached afterwards."
                .into(),
        )
    } else {
        None
    };
    emit_status_full(
        if is_first_run { "first_run" } else { "loading" },
        model_id,
        initial_message,
        Some(initial_progress),
        Some(initial_phase),
        Some(eta),
    );

    // Spawn a progress ticker. It cancels itself when `cancel` flips, which
    // happens immediately after `build_model` returns (success or failure).
    // The ticker re-uses the state we already initialized so the watchdog and
    // download-done flag persist across ticks.
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ticker = {
        let cancel = Arc::clone(&cancel);
        let mid = model_id.to_string();
        tokio::spawn(async move {
            let mut state = initial_state;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let (phase, progress) =
                    estimate_progress(&mut state, started, eta, is_first_run, &mid);
                emit_status_full(
                    "progress",
                    &mid,
                    None,
                    Some(progress),
                    Some(phase),
                    Some(eta),
                );
            }
        })
    };

    let result = LocalLlmEngine::build_model(source, model_id).await;
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    // The ticker exits on its next wake-up; we don't need to await it.
    drop(ticker);

    match result {
        Ok(engine) => {
            let engine = Arc::new(engine);
            *guard = Some(Arc::clone(&engine));
            emit_status_full(
                "ready",
                model_id,
                None,
                Some(1.0),
                Some("Ready".into()),
                None,
            );
            Ok(engine)
        }
        Err(e) => {
            *guard = None;
            emit_status_full("error", model_id, Some(e.clone()), None, None, None);
            Err(e)
        }
    }
}

/// Unload the engine to free RAM/VRAM.
pub async fn unload_engine() {
    let mut guard = engine_lock().write().await;
    if let Some(ref engine) = *guard {
        let mid = engine.model_id().to_string();
        info!("Unloading local LLM engine: {mid}");
        emit_status("unloaded", &mid, None);
    }
    *guard = None;
}

/// Check if an engine is currently loaded.
pub async fn engine_status() -> Option<String> {
    let guard = engine_lock().read().await;
    guard.as_ref().map(|e| e.model_id().to_string())
}

/// Validate that a model ID is recognized (either GGUF on disk or HF-ISQ).
pub fn validate_model(model_id: &str) -> Result<(), String> {
    resolve_model_source(model_id).map(|_| ())
}

/// Resolve how to load a given model.
fn resolve_model_source(model_id: &str) -> Result<ModelSource, String> {
    // ── HF-ISQ models (Gemma 4) ──────────────────────────────────────────
    // Gemma GGUF architecture is not supported by mistral.rs 0.8.x, but
    // MultimodalModelBuilder + ISQ works. First load downloads + quantizes.
    let hf_repo = match model_id {
        "gemma-4-e2b" => Some("google/gemma-4-E2B-it"),
        "gemma-4-e4b" => Some("google/gemma-4-E4B-it"),
        _ => None,
    };
    if let Some(repo) = hf_repo {
        return Ok(ModelSource::HfIsq {
            hf_repo: repo.to_string(),
        });
    }

    // ── GGUF catalog models ──────────────────────────────────────────────
    let gguf_filename = match model_id {
        "qwen3-0.6b" => Some("Qwen3-0.6B-Q4_K_M.gguf"),
        "qwen3-1.7b" => Some("Qwen3-1.7B-Q4_K_M.gguf"),
        "qwen3-4b" => Some("Qwen3-4B-Q4_K_M.gguf"),
        "qwen3-8b" => Some("Qwen3-8B-Q4_K_M.gguf"),
        "llama-3.2-3b" => Some("Llama-3.2-3B-Instruct-Q4_K_M.gguf"),
        "phi3-mini-q4" => Some("Phi-3-mini-4k-instruct-Q4_K_M.gguf"),
        "mistral-7b-q4" => Some("Mistral-7B-Instruct-v0.3-Q4_K_M.gguf"),
        _ => None,
    };
    if let Some(fname) = gguf_filename {
        let path = gravai_config::models_dir().join("llm").join(fname);
        if !path.exists() {
            return Err(format!(
                "LLM model file not found: {}. Download it first via Settings → Models.",
                path.display()
            ));
        }
        return Ok(ModelSource::Gguf {
            filename: fname.to_string(),
        });
    }

    // ── Custom GGUF file ─────────────────────────────────────────────────
    let gguf_name = if model_id.ends_with(".gguf") {
        model_id.to_string()
    } else {
        format!("{model_id}.gguf")
    };
    let path = gravai_config::models_dir().join("llm").join(&gguf_name);
    if path.exists() {
        return Ok(ModelSource::Gguf {
            filename: gguf_name,
        });
    }

    Err(format!(
        "Unknown LLM model: {model_id}. Download it first via Settings → Models."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn cached_load_progress_uses_full_range() {
        // Non-first-run (cached UQFF) should not reserve the lower half.
        let started = Instant::now();
        let mut state = ProgressState::new(started);
        let (_, p0) = estimate_progress(&mut state, started, 50, false, "gemma-4-e2b");
        assert!(p0 < 0.05, "expected near-zero start, got {p0}");

        // Pretend 25 s have passed by rebuilding state with an earlier start.
        let earlier = started - Duration::from_secs(25);
        let mut state = ProgressState::new(earlier);
        let (label, p) = estimate_progress(&mut state, earlier, 50, false, "gemma-4-e2b");
        assert!(p > 0.4 && p < 0.6, "halfway through 50 s eta: {p}");
        assert!(label.contains("Loading") || label.contains("Warming"));
    }

    #[test]
    fn first_run_with_no_observed_download_uses_full_range() {
        // Simulate: cache is already full (so the very first tick sees a
        // huge `cur` value and immediately marks download done). The bar
        // should NOT jump to 50 % — it should start near 0 and use the full
        // 0–95 % range across the eta.
        //
        // We can't easily fake the file system here, so instead assert the
        // logical invariant directly via ProgressState.
        let started = Instant::now();
        let mut state = ProgressState::new(started);
        // Pretend the very first observation flagged download done, with no
        // visible download phase.
        state.download_done_at = Some(started);
        state.had_download_phase = false;

        let (_, p0) = estimate_progress(&mut state, started, 360, true, "gemma-4-e4b");
        // Hopefully not stuck at 50 %.
        assert!(p0 < 0.05, "expected near-zero start, got {p0}");

        // Simulate ~150 s elapsed (~half of post-download eta of 360 s).
        let earlier = started - Duration::from_secs(150);
        let mut state = ProgressState::new(earlier);
        state.download_done_at = Some(earlier);
        state.had_download_phase = false;
        let (_, p) = estimate_progress(&mut state, earlier, 360, true, "gemma-4-e4b");
        assert!(p > 0.30 && p < 0.55, "midway through 360 s eta: {p}");
    }

    #[test]
    fn first_run_with_visible_download_reserves_lower_half() {
        // When a real download phase happened, the post-download work is
        // shown in the upper half of the bar.
        let started = Instant::now() - Duration::from_secs(120);
        let mut state = ProgressState::new(started);
        state.download_done_at = Some(Instant::now() - Duration::from_secs(60));
        state.had_download_phase = true;

        let (_, p) = estimate_progress(&mut state, started, 360, true, "gemma-4-e4b");
        assert!(p >= 0.50, "should be at least 50 % after download: {p}");
        assert!(p < 0.95, "should not yet be at the cap: {p}");
    }
}
