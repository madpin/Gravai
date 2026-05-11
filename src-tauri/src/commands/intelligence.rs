//! AI intelligence commands: summarize, diarize.

use gravai_core::AppState;
use gravai_intelligence::SummarizationProvider;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;
use tracing::info;

/// Maximum wall-clock time we'll wait for the LLM engine to be ready
/// before starting inference. First-run HF-ISQ load + quantize is bounded
/// internally to 600 s already; we add a small margin here so a stuck
/// engine load doesn't pin a `summarize_session` call indefinitely.
const ENGINE_LOAD_BUDGET_SECS: u64 = 660;

/// Maximum wall-clock time we'll wait for the actual chat-completion call
/// once the engine is ready. On Apple Silicon, a 22 K-char transcript
/// summary completes in 30–120 s on the recommended models, so 8 minutes
/// is a generous ceiling that still surfaces real hangs.
const INFERENCE_BUDGET_SECS: u64 = 480;

/// How often the heartbeat emits an `LlmStatus` event during inference so
/// the frontend can show a live "Summarizing…" progress bar with elapsed
/// time and a smooth interpolation against the eta hint.
const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// Phase label used for the inference heartbeat. Matches the labels the
/// engine itself emits during model load so the existing `LlmStatusBanner`
/// renders them consistently.
const SUMMARIZING_PHASE: &str = "Summarizing transcript";

/// Generate a summary for a session's transcript.
///
/// Persists the result in `session_summaries` so subsequent requests
/// (Archive, reopen, refresh) can retrieve it cheaply via
/// `get_session_summary` instead of re-running the LLM.
#[tauri::command]
pub async fn summarize_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<serde_json::Value, String> {
    let config = state.config.read().await;

    // Load transcript from DB
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let utterances = db.get_utterances(&session_id).map_err(|e| e.to_string())?;

    if utterances.is_empty() {
        return Err("No transcript to summarize".into());
    }

    // Build transcript text — prefer corrected_text when present AND non-empty
    // (a previous failed correction can leave an empty string in the column,
    // which would otherwise blank out the utterance for summarization).
    let transcript: String = utterances
        .iter()
        .map(|u| {
            let corrected = u.corrected_text.as_deref().filter(|s| !s.trim().is_empty());
            let text = corrected.unwrap_or(&u.text);
            format!(
                "[{}] {}: {}",
                u.timestamp,
                u.speaker.as_deref().unwrap_or(&u.source),
                text
            )
        })
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if transcript.trim().is_empty() {
        return Err("Transcript is empty — nothing to summarize.".into());
    }

    let transcript_len = transcript.len();
    info!(
        "Summarizing session {session_id} ({} utterances, {transcript_len} chars)",
        utterances.len(),
    );

    let llm_config = config.llm.clone();
    let provider_label = match llm_config.provider.as_str() {
        "local" => format!("local/{}", llm_config.local_model),
        _ => format!("api/{}", llm_config.model),
    };
    drop(config); // release the read lock before any await

    // ── Phase 1: ensure the engine is ready ──────────────────────────────
    //
    // For local models we eagerly resolve the engine *before* starting the
    // inference timer. This way:
    //   - First-run model loads (download + quantize, can take minutes)
    //     get their own dedicated budget and their existing rich progress
    //     events from `local_engine`, instead of being silently rolled
    //     into the summarization timeout.
    //   - When this returns OK, the user sees an immediate "Summarizing…"
    //     status bar instead of a generic spinner that might silently fail
    //     30 minutes later.
    if llm_config.provider == "local" {
        let model_id = llm_config.local_model.clone();
        let load_result = tokio::time::timeout(
            std::time::Duration::from_secs(ENGINE_LOAD_BUDGET_SECS),
            gravai_intelligence::local_engine::get_or_load_engine(&model_id),
        )
        .await;
        match load_result {
            Err(_) => {
                gravai_intelligence::local_engine::emit_status_external(
                    "error",
                    &model_id,
                    Some("Local LLM took too long to load.".into()),
                    None,
                    None,
                    None,
                );
                return Err(format!(
                    "Local LLM '{model_id}' did not finish loading in {ENGINE_LOAD_BUDGET_SECS}s. \
                     Try a smaller model in Settings (e.g. qwen3-1.7b or gemma-4-e2b), \
                     or check the logs for download/quantization errors."
                ));
            }
            Ok(Err(e)) => {
                return Err(format!(
                    "Failed to prepare local LLM '{model_id}': {e}. \
                     Open Settings → Models to verify the model is available."
                ));
            }
            Ok(Ok(_)) => {} // engine ready, fall through
        }
    }

    // ── Phase 2: inference with heartbeat ────────────────────────────────
    //
    // Spawn a ticker that publishes "summarizing" `LlmStatus` events every
    // second. The frontend `LlmStatusBanner` shows a smooth elapsed/ETA
    // bar driven by these events, so multi-minute summarizations on big
    // local models feel responsive instead of frozen.
    let heartbeat_eta = inference_eta_seconds(&llm_config, transcript_len);
    let heartbeat_model = match llm_config.provider.as_str() {
        "local" => llm_config.local_model.clone(),
        _ => llm_config.model.clone(),
    };
    gravai_intelligence::local_engine::emit_status_external(
        "summarizing",
        &heartbeat_model,
        Some(format!(
            "Summarizing {} chars of transcript",
            transcript_len.min(99_999)
        )),
        Some(0.0),
        Some(SUMMARIZING_PHASE.into()),
        Some(heartbeat_eta),
    );

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let heartbeat_handle = spawn_summary_heartbeat(
        Arc::clone(&cancel_flag),
        heartbeat_model.clone(),
        heartbeat_eta,
    );

    let inference_result = tokio::time::timeout(
        std::time::Duration::from_secs(INFERENCE_BUDGET_SECS),
        async {
            let provider =
                gravai_intelligence::summarization::LlmSummarizationProvider::new(&llm_config)
                    .await
                    .map_err(|e| e.to_string())?;
            provider
                .summarize(&transcript, None)
                .await
                .map_err(|e| e.to_string())
        },
    )
    .await;

    cancel_flag.store(true, Ordering::Relaxed);
    heartbeat_handle.abort();

    let summary = match inference_result {
        Ok(Ok(summary)) => {
            gravai_intelligence::local_engine::emit_status_external(
                "ready",
                &heartbeat_model,
                None,
                Some(1.0),
                Some("Summary ready".into()),
                None,
            );
            summary
        }
        Ok(Err(e)) => {
            gravai_intelligence::local_engine::emit_status_external(
                "error",
                &heartbeat_model,
                Some(e.clone()),
                None,
                None,
                None,
            );
            return Err(format!(
                "Summary generation failed: {e}. \
                 Try regenerating; if the issue persists, switch to a smaller model in Settings."
            ));
        }
        Err(_) => {
            gravai_intelligence::local_engine::emit_status_external(
                "error",
                &heartbeat_model,
                Some(format!(
                    "Inference exceeded {INFERENCE_BUDGET_SECS}s budget."
                )),
                None,
                None,
                None,
            );
            let mins = INFERENCE_BUDGET_SECS / 60;
            return Err(format!(
                "Summary generation timed out after {mins} minutes on a {transcript_len}-char transcript. \
                 The transcript may still be too long for the selected model — try switching to \
                 a smaller, faster model (qwen3-1.7b or gemma-4-e2b) in Settings → AI, \
                 or use an API provider for very long meetings."
            ));
        }
    };

    // Persist to DB so it survives navigation/refresh
    {
        let key_decisions_json = serde_json::to_string(&summary.key_decisions).ok();
        let action_items_json = serde_json::to_string(&summary.action_items).ok();
        let open_questions_json = serde_json::to_string(&summary.open_questions).ok();
        if let Err(e) = db.upsert_session_summary(
            &session_id,
            Some(&summary.tldr),
            key_decisions_json.as_deref(),
            action_items_json.as_deref(),
            open_questions_json.as_deref(),
            Some(&provider_label),
        ) {
            tracing::warn!("Failed to persist summary for {session_id}: {e}");
        }
    }

    let summary_json = serde_json::to_value(&summary).map_err(|e| e.to_string())?;
    info!("Summary generated for session {session_id}");
    Ok(summary_json)
}

/// Heuristic estimate (in seconds) for how long inference should take on
/// the configured provider, given the transcript size. Drives the smooth
/// progress bar; not used for any timeout logic.
fn inference_eta_seconds(config: &gravai_config::LlmConfig, transcript_len: usize) -> u64 {
    // Roughly tuned against M-series Apple Silicon throughput for the
    // bundled GGUF/ISQ models. Hosted APIs are much faster but also have
    // network round-trips; pick a small constant for them.
    let chars_k = (transcript_len as f32 / 1000.0).max(1.0);
    let base = if config.provider == "local" {
        match config.local_model.as_str() {
            id if id.starts_with("qwen3-0.6b") => 10.0 + chars_k * 1.0,
            id if id.starts_with("qwen3-1.7b") || id.contains("phi3") => 15.0 + chars_k * 1.5,
            id if id.starts_with("qwen3-4b") || id.starts_with("llama-3.2-3b") => {
                25.0 + chars_k * 2.5
            }
            id if id.contains("gemma-4-e2b") => 25.0 + chars_k * 2.0,
            id if id.contains("gemma-4-e4b") => 40.0 + chars_k * 3.5,
            id if id.starts_with("qwen3-8b") || id.starts_with("mistral-7b") => {
                50.0 + chars_k * 4.5
            }
            _ => 30.0 + chars_k * 3.0,
        }
    } else {
        // Hosted API — small fixed overhead + a tiny per-K factor.
        8.0 + chars_k * 0.6
    };
    base.clamp(8.0, INFERENCE_BUDGET_SECS as f32 - 30.0) as u64
}

/// Spawn a background task that periodically publishes a "summarizing"
/// `LlmStatus` event so the frontend's banner stays alive with a moving
/// progress bar throughout inference. Aborted by the caller on completion.
fn spawn_summary_heartbeat(
    cancel: Arc<AtomicBool>,
    model_id: String,
    eta_seconds: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let eta = eta_seconds.max(1) as f32;
        loop {
            tokio::time::sleep(HEARTBEAT_INTERVAL).await;
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let elapsed = started.elapsed().as_secs_f32();
            // Cap at 0.95 — only the final "ready" event snaps to 1.0,
            // matching the engine-load progress contract.
            let progress = (elapsed / eta).clamp(0.0, 0.95);
            gravai_intelligence::local_engine::emit_status_external(
                "summarizing",
                &model_id,
                None,
                Some(progress),
                Some(SUMMARIZING_PHASE.into()),
                Some(eta_seconds),
            );
        }
    })
}

/// Retrieve the persisted summary for a session, if one has been generated.
/// Returns `null` if no summary exists yet (frontend should fall back to
/// running `summarize_session`).
#[tauri::command]
pub async fn get_session_summary(session_id: String) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let record = db
        .get_session_summary(&session_id)
        .map_err(|e| e.to_string())?;
    let Some(rec) = record else {
        return Ok(serde_json::Value::Null);
    };
    let parse_arr = |s: Option<&str>| -> serde_json::Value {
        s.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .unwrap_or_else(|| serde_json::json!([]))
    };
    Ok(serde_json::json!({
        "tldr": rec.tldr.unwrap_or_default(),
        "key_decisions": parse_arr(rec.key_decisions.as_deref()),
        "action_items": parse_arr(rec.action_items.as_deref()),
        "open_questions": parse_arr(rec.open_questions.as_deref()),
        "created_at": rec.created_at,
        "provider": rec.provider,
    }))
}

/// List available export formats for the current platform.
#[tauri::command]
pub async fn get_export_formats() -> Result<serde_json::Value, String> {
    let formats: Vec<serde_json::Value> = gravai_audio::encoder::available_formats()
        .iter()
        .map(|(id, label)| serde_json::json!({"id": id, "label": label}))
        .collect();
    Ok(serde_json::json!(formats))
}

/// Export a session's audio to a different format.
/// Merges all tracks (mic + system) into one file automatically.
#[tauri::command]
pub async fn export_session_audio(
    session_id: String,
    format: String,
    source_track: Option<String>,
) -> Result<String, String> {
    let session_dir = gravai_config::sessions_dir().join(&session_id);
    let fmt = gravai_audio::encoder::ExportFormat::parse(&format);

    if let Some(track) = source_track {
        // Export a specific track only
        let source = session_dir.join(format!("{track}.wav"));
        if !source.exists() {
            return Err(format!("Track not found: {track}.wav"));
        }
        let output = session_dir.join(format!("{track}.{}", fmt.extension()));
        gravai_audio::encoder::export_audio(&source, &output, fmt, 192)?;
        Ok(output.display().to_string())
    } else {
        // Default: merge all tracks into one export
        let output = session_dir.join(format!("export.{}", fmt.extension()));
        gravai_audio::encoder::merge_and_export(&session_dir, &output, fmt, 192)?;
        Ok(output.display().to_string())
    }
}

/// Retrieve per-speaker sentiment summary for a session.
/// Returns a list of speakers with their dominant emotion and top emotion counts.
#[tauri::command]
pub async fn get_session_sentiment(session_id: String) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let utterances = db
        .get_session_sentiment(&session_id)
        .map_err(|e| e.to_string())?;

    // Group by speaker, accumulate emotion counts
    let mut speakers: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for u in &utterances {
        let speaker = u.speaker.clone().unwrap_or_else(|| "Remote".into());
        let label = u
            .sentiment_label
            .clone()
            .unwrap_or_else(|| "neutral".into());
        let score = u.sentiment_score.unwrap_or(0.0);
        speakers
            .entry(speaker)
            .or_default()
            .push(serde_json::json!({ "label": label, "score": score }));
    }

    let summary: Vec<serde_json::Value> = speakers
        .into_iter()
        .map(|(speaker, emotions)| {
            // Count each label
            let mut counts: HashMap<String, u32> = HashMap::new();
            for e in &emotions {
                let label = e["label"].as_str().unwrap_or("neutral").to_string();
                *counts.entry(label).or_default() += 1;
            }
            let dominant = counts
                .iter()
                .max_by_key(|(_, &v)| v)
                .map(|(k, _)| k.clone())
                .unwrap_or_else(|| "neutral".into());
            serde_json::json!({
                "speaker": speaker,
                "dominant_emotion": dominant,
                "utterance_count": emotions.len(),
                "emotion_counts": counts,
            })
        })
        .collect();

    Ok(serde_json::json!({ "speakers": summary }))
}
