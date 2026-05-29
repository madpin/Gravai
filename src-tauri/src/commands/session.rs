//! Session lifecycle commands: start, pause, resume, stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gravai_audio::capture::{AudioCaptureManager, AudioChunk, VolumeCallback};
use gravai_audio::echo::EchoSuppressor;
use gravai_audio::pipeline;
use gravai_audio::recorder::MultiTrackRecorder;
use gravai_core::{AppState, GravaiEvent, Session, SessionState};
use tauri::State;
use tokio::sync::mpsc;

// Shared stop flag for the capture thread — set to true on session stop
// to release microphone and ScreenCaptureKit resources.
static CAPTURE_STOP: std::sync::OnceLock<std::sync::Mutex<Option<Arc<AtomicBool>>>> =
    std::sync::OnceLock::new();

fn get_capture_stop() -> &'static std::sync::Mutex<Option<Arc<AtomicBool>>> {
    CAPTURE_STOP.get_or_init(|| std::sync::Mutex::new(None))
}

// Process-wide shared database handle. Opening SQLite is expensive
// (file lock, PRAGMA setup, migration check), and previously every
// utterance insert / sentiment update / correction update opened a
// fresh connection inside the audio pipeline's async task. We now
// open once and share an Arc across all hot paths.
static DB: std::sync::OnceLock<Arc<gravai_storage::Database>> = std::sync::OnceLock::new();

fn shared_db() -> Result<Arc<gravai_storage::Database>, String> {
    if let Some(db) = DB.get() {
        return Ok(db.clone());
    }
    let path = gravai_config::data_dir().join("gravai.db");
    let opened = gravai_storage::Database::open(&path).map_err(|e| e.to_string())?;
    let arc = Arc::new(opened);
    // If another thread initialized first, prefer the existing one.
    Ok(DB.get_or_init(|| arc).clone())
}

/// Atomic guard that ensures only one `start_session` runs at a time.
/// Uses compare-exchange so concurrent callers fail immediately rather than
/// queuing up and starting duplicate sessions.
static SESSION_STARTING: AtomicBool = AtomicBool::new(false);

struct SessionStartGuard;
impl SessionStartGuard {
    /// Try to acquire the guard. Returns `None` if already taken.
    fn try_acquire() -> Option<Self> {
        SESSION_STARTING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| SessionStartGuard)
    }
}
impl Drop for SessionStartGuard {
    fn drop(&mut self) {
        SESSION_STARTING.store(false, Ordering::SeqCst);
    }
}
use tracing::{error, info, warn};

/// Start a new recording session.
#[tauri::command]
pub async fn start_session(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    // Acquire the start guard — only one start_session may run at a time.
    // This prevents duplicate sessions from concurrent calls (tray + automation,
    // double-click, etc.). The guard auto-resets via Drop on any return path.
    let _start_guard = SessionStartGuard::try_acquire().ok_or("A session is already starting")?;

    // Also reject if a session is actively recording/paused
    {
        let current = state.session.read().await;
        if let Some(ref s) = *current {
            if s.is_active() {
                return Err("A session is already active".into());
            }
        }
    }

    let mut config = state.config.read().await.clone();

    // Apply active profile overrides to config before starting
    let profile_store = gravai_config::profiles::ProfileStore::load();
    if let Some(ref pid) = profile_store.active_profile_id {
        if let Some(profile) = profile_store.profiles.get(pid) {
            if let Some(ref engine) = profile.transcription_engine {
                config.transcription.engine = engine.clone();
            }
            if let Some(ref model) = profile.transcription_model {
                config.transcription.model = model.clone();
            }
            if let Some(ref lang) = profile.transcription_language {
                config.transcription.language = lang.clone();
            }
            if let Some(ref provider) = profile.llm_provider {
                config.llm.provider = provider.clone();
            }
            if let Some(ref model) = profile.llm_model {
                config.llm.model = model.clone();
            }
            if let Some(ref local_model) = profile.llm_local_model {
                config.llm.local_model = local_model.clone();
            }
            // Re-run migration in case the profile has legacy provider values
            config.llm.migrate();
            if let Some(enabled) = profile.diarization_enabled {
                config.features.diarization.enabled = enabled;
            }
            if let Some(enabled) = profile.echo_suppression_enabled {
                config.features.echo_suppression.enabled = enabled;
            }
            if let Some(enabled) = profile.correction_enabled {
                config.correction.enabled = enabled;
            }
            info!(
                "Applied profile '{}' overrides (model: {})",
                profile.name, config.transcription.model
            );
        }
    }

    // Also apply active preset overrides
    let preset_store = gravai_config::presets::PresetStore::load();
    if let Some(ref pid) = preset_store.active_preset_id {
        if let Some(preset) = preset_store.presets.get(pid) {
            config.audio.microphone.enabled = preset.mic_enabled;
            config.audio.system_audio.enabled = preset.sys_enabled;
            config.audio.recording.sample_rate = preset.sample_rate;
            config.audio.recording.bit_depth = preset.bit_depth;
            config.audio.recording.channels = preset.channels;
            config.audio.recording.export_format = preset.export_format.clone();
            if let Some(ref folder) = preset.output_folder {
                config.audio.recording.output_folder = Some(folder.clone());
            }
            info!("Applied preset '{}' overrides", preset.name);
        }
    }

    // Helper to publish startup progress so the frontend activity log stays updated
    let progress_bus = state.event_bus.clone();
    let emit_progress = move |msg: &str| {
        info!("{msg}");
        progress_bus.publish(GravaiEvent::SessionStartProgress {
            message: msg.to_string(),
        });
    };

    emit_progress("Creating session...");
    let session_id = gravai_core::session::generate_session_id();
    let session = Arc::new(Session::new(session_id.clone(), config.clone()));
    session.set_state(SessionState::Recording);

    // Check for running meeting apps (fast — just process list scan)
    let meeting_app = {
        let meetings = gravai_meeting::detector::detect_meeting_apps();
        meetings.first().map(|m| m.app_name.clone())
    };

    // Store session in DB early — pipelines may insert utterances before this block was reached
    {
        if let Ok(db) = shared_db() {
            let record = gravai_storage::SessionRecord {
                id: session_id.clone(),
                started_at: session.started_at.to_rfc3339(),
                ended_at: None,
                duration_seconds: None,
                title: None,
                meeting_app: meeting_app.clone(),
                state: "recording".into(),
            };
            if let Err(e) = db.create_session(&record) {
                error!("Failed to create session record: {e}");
            }
        }
        // Register session in AppState early so stop_session works during startup
        let mut session_lock = state.session.write().await;
        *session_lock = Some(session.clone());
    }

    // Auto-name session from calendar in background (can take 10-15s with large calendars).
    // Updates the DB and emits an event when done — does not block recording start.
    {
        let cal_sid = session_id.clone();
        let cal_lead = config.features.meeting_detection.lead_time_seconds;
        let cal_meeting_app = meeting_app.clone();
        let cal_bus = state.event_bus.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            info!("Calendar lookup starting (background)...");
            let title = gravai_meeting::calendar::find_meeting_title(cal_lead).or_else(|| {
                if cal_meeting_app.as_deref() == Some("Zoom") {
                    gravai_meeting::detector::get_zoom_window_title()
                } else {
                    None
                }
            });
            let elapsed = start.elapsed();
            match &title {
                Some(t) => {
                    info!("Calendar found: \"{t}\" ({:.1}s)", elapsed.as_secs_f64());
                    if let Ok(db) = shared_db() {
                        let _ = db.rename_session(&cal_sid, t);
                    }
                    cal_bus.publish(GravaiEvent::SessionStartProgress {
                        message: format!("Meeting detected: {t}"),
                    });
                    // Re-emit session state so the frontend picks up the new title
                    cal_bus.publish(GravaiEvent::SessionStateChanged {
                        state: "recording".into(),
                        session_id: Some(cal_sid),
                    });
                }
                None => {
                    let msg = if elapsed.as_secs() >= 55 {
                        format!(
                            "Calendar lookup timed out ({:.0}s) — session unnamed",
                            elapsed.as_secs_f64()
                        )
                    } else {
                        format!("No calendar event found ({:.1}s)", elapsed.as_secs_f64())
                    };
                    info!("{msg}");
                    cal_bus.publish(GravaiEvent::SessionStartProgress { message: msg });
                }
            }
        });
    }
    let session_title: Option<String> = None;

    // Determine output directory (custom folder or default)
    let output_dir = config
        .audio
        .recording
        .output_folder
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| session.session_dir.clone());

    // Set up multi-track recorder (if recording is enabled)
    let recording_enabled = config.audio.recording.enabled;
    let recorder = if recording_enabled {
        let mut rec = MultiTrackRecorder::new(&output_dir).map_err(|e| format!("Recorder: {e}"))?;

        if config.audio.microphone.enabled {
            rec.add_track("mic")
                .map_err(|e| format!("Add mic track: {e}"))?;
        }
        if config.audio.system_audio.enabled {
            rec.add_track("system")
                .map_err(|e| format!("Add system track: {e}"))?;
        }
        // No master track — each source writes its own file with correct format.
        // Mixing sources with different sample rates into one file causes corruption.
        // Users can combine tracks in a DAW if needed.
        Some(Arc::new(std::sync::Mutex::new(rec)))
    } else {
        info!("Recording to disk is disabled — transcription only mode");
        None
    };

    emit_progress("Starting audio capture...");
    // Audio capture runs on a dedicated OS thread (cpal::Stream is not Send).
    let (mic_hq_tx, mic_hq_rx) = std::sync::mpsc::channel::<Option<mpsc::Receiver<AudioChunk>>>();
    let (sys_hq_tx, sys_hq_rx) = std::sync::mpsc::channel::<Option<mpsc::Receiver<AudioChunk>>>();
    let (mic_lq_tx_ch, mic_lq_rx_ch) =
        std::sync::mpsc::channel::<Option<mpsc::Receiver<Vec<f32>>>>();
    let (sys_lq_tx_ch, sys_lq_rx_ch) =
        std::sync::mpsc::channel::<Option<mpsc::Receiver<Vec<f32>>>>();
    let (result_tx, result_rx) = std::sync::mpsc::channel::<Result<(), String>>();

    let capture_config = config.clone();
    let bus = state.event_bus.clone();

    // Shared flag to signal the capture thread to stop and release resources
    let capture_stop = Arc::new(AtomicBool::new(false));
    let capture_stop_clone = capture_stop.clone();

    let _capture_thread = std::thread::spawn(move || {
        let mut capture = AudioCaptureManager::new(capture_config);

        // Throttle VolumeLevel publishes to 4 Hz per source. The underlying
        // capture callback fires at 10 Hz per source; emitting all of those
        // floods the Tauri IPC bridge (~48K events over a 40-minute session)
        // and forces the frontend VU meter to re-layout 20x/sec. The silence
        // detector in lib.rs uses these as a liveness signal with 10s/60s
        // thresholds, so 4 Hz is still well above what it needs.
        let last_publish: Arc<Mutex<std::collections::HashMap<String, std::time::Instant>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        let volume_cb: VolumeCallback = Arc::new(move |source: &str, db: f64| {
            const PUBLISH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
            let now = std::time::Instant::now();
            let mut map = last_publish.lock().unwrap();
            if let Some(prev) = map.get(source) {
                if now.duration_since(*prev) < PUBLISH_INTERVAL {
                    return;
                }
            }
            map.insert(source.to_string(), now);
            drop(map);
            bus.publish(GravaiEvent::VolumeLevel {
                source: source.to_string(),
                db,
            });
        });
        capture.set_volume_callback(volume_cb);

        if let Err(e) = capture.start() {
            let _ = result_tx.send(Err(e.to_string()));
            return;
        }

        let _ = mic_hq_tx.send(capture.mic_hq_rx.take());
        let _ = sys_hq_tx.send(capture.sys_hq_rx.take());
        let _ = mic_lq_tx_ch.send(capture.mic_lq_rx.take());
        let _ = sys_lq_tx_ch.send(capture.sys_lq_rx.take());
        let _ = result_tx.send(Ok(()));

        // Wait for stop signal, then drop capture to release mic + SCK
        while !capture_stop_clone.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        capture.stop();
        info!("Audio capture thread exiting — mic and SCK released");
        // capture drops here, releasing all OS audio resources
    });

    // Store the stop flag so stop_session can signal it
    *get_capture_stop().lock().unwrap() = Some(capture_stop);

    let start_result = result_rx
        .recv()
        .map_err(|_| "Capture thread died".to_string())?;
    start_result?;

    let mic_hq = mic_hq_rx.recv().ok().flatten();
    let sys_hq = sys_hq_rx.recv().ok().flatten();
    let mic_lq = mic_lq_rx_ch.recv().ok().flatten();
    let sys_lq = sys_lq_rx_ch.recv().ok().flatten();

    // Spawn recording tasks for HQ streams (only if recording enabled)
    if let (Some(rx), Some(ref rec)) = (mic_hq, &recorder) {
        let rec = rec.clone();
        let handle = tokio::spawn(async move {
            record_track(rx, rec, "mic").await;
        });
        session.add_task(handle).await;
    }

    if let (Some(rx), Some(ref rec)) = (sys_hq, &recorder) {
        let rec = rec.clone();
        let handle = tokio::spawn(async move {
            record_track(rx, rec, "system").await;
        });
        session.add_task(handle).await;
    }

    emit_progress("Loading AI models (transcription, diarization, sentiment)...");
    // Load all AI models in parallel (transcription, diarization, sentiment).
    // These are the main startup bottleneck — loading them concurrently instead
    // of sequentially cuts startup time significantly.
    let trans_config = config.transcription.clone();
    let diar_enabled = config.features.diarization.enabled;
    let diar_config = config.features.diarization.clone();

    // Warn about missing pyannote model before we start loading
    if diar_enabled && diar_config.model == "pyannote" {
        let model_path = gravai_config::models_dir()
            .join("diarization")
            .join("segmentation.onnx");
        if !model_path.exists() {
            state.event_bus.publish(GravaiEvent::Error {
                message: "Diarization is set to 'pyannote' but the model is not \
                          downloaded — using energy-based fallback instead. \
                          Download 'pyannote-segmentation' from the Models page \
                          to enable accurate multi-speaker labels."
                    .into(),
            });
            warn!(
                "Pyannote model missing at {}; falling back to energy diarizer",
                model_path.display()
            );
        }
    }

    let trans_future =
        tokio::task::spawn_blocking(move || gravai_transcription::create_provider(&trans_config));

    let diar_future = tokio::task::spawn_blocking(move || {
        if diar_enabled {
            Some(Arc::from(gravai_intelligence::diarization::create_diarizer(
                &diar_config,
            ))
                as Arc<dyn gravai_intelligence::DiarizationProvider>)
        } else {
            None
        }
    });

    let sentiment_future = tokio::task::spawn_blocking(|| {
        gravai_intelligence::OnnxSentimentEngine::try_load()
            .map(|e| Arc::new(e) as Arc<dyn gravai_intelligence::SentimentEngine>)
    });

    // Await all three in parallel
    let (trans_result, diar_result, sentiment_result) =
        tokio::join!(trans_future, diar_future, sentiment_future);

    emit_progress("AI models loaded, configuring pipelines...");

    let trans_bus = state.event_bus.clone();
    let transcriber: Option<Arc<dyn gravai_transcription::TranscriptionProvider>> =
        match trans_result {
            Ok(Ok(provider)) => {
                info!("Transcription ready ({})", config.transcription.model);
                Some(Arc::from(provider))
            }
            Ok(Err(e)) => {
                warn!("Transcription not available: {e}");
                trans_bus.publish(GravaiEvent::Error {
                    message: format!(
                        "Transcription unavailable: {}. Go to Settings → Models to download the '{}' model.",
                        e, config.transcription.model
                    ),
                });
                None
            }
            Err(e) => {
                warn!("Transcription load panicked: {e}");
                None
            }
        };

    let echo_suppressor = Arc::new(tokio::sync::Mutex::new(EchoSuppressor::new(
        config.features.echo_suppression.similarity_threshold,
    )));
    let pipeline_active = Arc::new(AtomicBool::new(true));

    let diarizer: Option<Arc<dyn gravai_intelligence::DiarizationProvider>> =
        diar_result.ok().flatten();

    let sentiment_engine: Option<Arc<dyn gravai_intelligence::SentimentEngine>> =
        sentiment_result.ok().flatten();

    // Set up LLM correction state (debounced batch, enabled only if configured).
    // Failure to init correction should not prevent the session from starting.
    let correction_provider: Option<Arc<gravai_intelligence::TranscriptCorrectionProvider>> =
        if config.correction.enabled {
            match gravai_intelligence::TranscriptCorrectionProvider::new(
                &config.llm,
                config.correction.model.as_deref(),
                config.correction.custom_prompt.as_deref(),
            )
            .await
            {
                Ok(provider) => Some(Arc::new(provider)),
                Err(e) => {
                    warn!("Correction provider unavailable, continuing without it: {e}");
                    None
                }
            }
        } else {
            None
        };
    // Spawn the correction actor (one task per session) that owns the pending
    // batch and the debounce timer. The writer task (below) sends utterance
    // ids into this channel — no per-utterance task spawn.
    let correction_tx: Option<tokio::sync::mpsc::UnboundedSender<i64>> =
        if let Some(provider) = correction_provider.as_ref() {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<i64>();
            let provider = provider.clone();
            let event_bus_actor = state.event_bus.clone();
            let sid_actor = session_id.clone();
            let batch_size = config.correction.batch_size;
            let debounce = config.correction.debounce_seconds;
            let timeout = config.correction.timeout_seconds;
            let handle = tokio::spawn(run_correction_actor(
                rx,
                provider,
                event_bus_actor,
                sid_actor,
                batch_size,
                debounce,
                timeout,
            ));
            session.add_task(handle).await;
            Some(tx)
        } else {
            None
        };

    // Single per-session writer task: drains `utt_rx` serially and does the
    // DB insert + sentiment + correction send. The on_utterance closure just
    // calls `utt_tx.send(...)` and returns — so the pipeline's async worker
    // is freed immediately AND insert order is deterministic (equal to send
    // order, which equals the pipeline's emit order). Without this, two
    // concurrent `tokio::spawn`s would race for the DB mutex and a later
    // utterance could win a lower row id, displaying out of order on the
    // frontend (which iterates by id).
    let (utt_tx, mut utt_rx) =
        tokio::sync::mpsc::unbounded_channel::<gravai_audio::pipeline::Utterance>();
    let writer_event_bus = state.event_bus.clone();
    let writer_sid = session_id.clone();
    let writer_sentiment = sentiment_engine.clone();
    let writer_correction_tx = correction_tx.clone();
    let writer_handle = tokio::spawn(async move {
        while let Some(utterance) = utt_rx.recv().await {
            let timestamp = utterance.timestamp.to_rfc3339();
            let record = gravai_storage::UtteranceRecord {
                id: 0,
                session_id: writer_sid.clone(),
                timestamp: timestamp.clone(),
                source: utterance.source.clone(),
                speaker: utterance.speaker.clone(),
                text: utterance.text.clone(),
                confidence: None,
                start_ms: Some(utterance.start_ms),
                end_ms: Some(utterance.end_ms),
                sentiment_label: None,
                sentiment_score: None,
                emotions_json: None,
                corrected_text: None,
                correction_status: None,
                correction_provider: None,
                corrected_at: None,
            };

            // SQLite calls are blocking syscalls — run on the blocking pool
            // so this async worker doesn't hold a worker thread.
            let insert_result = tokio::task::spawn_blocking(move || {
                let db = shared_db().map_err(|e| format!("shared_db: {e}"))?;
                db.insert_utterance(&record)
                    .map_err(|e| format!("insert_utterance: {e}"))
            })
            .await;

            let id = match insert_result {
                Ok(Ok(id)) => id,
                Ok(Err(e)) => {
                    error!("Insert utterance: {e}");
                    continue;
                }
                Err(join_err) => {
                    error!("Insert utterance task panicked: {join_err}");
                    continue;
                }
            };

            writer_event_bus.publish(GravaiEvent::TranscriptUpdated {
                session_id: writer_sid.clone(),
                utterance_id: id,
                source: utterance.source.clone(),
                speaker: utterance.speaker.clone(),
                text: utterance.text.clone(),
                timestamp,
            });

            // Sentiment on system audio only — fire-and-forget, uses shared DB.
            if let Some(engine) = writer_sentiment
                .as_ref()
                .filter(|_| utterance.source == "system_audio" || utterance.source == "system")
            {
                let engine = engine.clone();
                let text = utterance.text.clone();
                tokio::spawn(async move {
                    let result = tokio::task::spawn_blocking(move || engine.analyze(&text)).await;
                    if let Ok(sentiment) = result {
                        let emotions_json = sentiment
                            .emotions
                            .as_ref()
                            .and_then(|e| serde_json::to_string(e).ok());
                        let label = sentiment.label.clone();
                        let score = sentiment.score;
                        let _ = tokio::task::spawn_blocking(move || {
                            if let Ok(db) = shared_db() {
                                let _ = db.update_utterance_sentiment(
                                    id,
                                    &label,
                                    score,
                                    emotions_json.as_deref(),
                                );
                            }
                        })
                        .await;
                    }
                });
            }

            // Hand the id to the correction actor; debouncing + batching
            // happens there, so we never spawn per-utterance debounce tasks.
            if let Some(tx) = &writer_correction_tx {
                let _ = tx.send(id);
            }
        }
    });
    session.add_task(writer_handle).await;

    // The pipeline calls this synchronously from a tokio worker (see
    // crates/gravai-audio/src/pipeline.rs). All it does now is enqueue
    // the utterance for the writer task — the pipeline worker returns
    // immediately and never touches blocking I/O.
    let on_utterance: pipeline::OnUtterance = Arc::new(move |utterance| {
        let _ = utt_tx.send(utterance);
    });

    emit_progress("Starting transcription pipelines...");
    // Spawn transcription pipelines for each LQ source
    if let Some(rx) = mic_lq {
        let vad = gravai_audio::vad::create_vad(&config.vad).map_err(|e| format!("VAD: {e}"))?;
        let input = pipeline::PipelineInput {
            rx,
            source: "microphone".into(),
            vad,
            transcriber: transcriber.clone(),
            echo_suppressor: echo_suppressor.clone(),
            diarizer: None, // mic is always "You"
            config: pipeline::PipelineConfig::from_app_config(&config),
            on_utterance: on_utterance.clone(),
            active: pipeline_active.clone(),
        };
        let handle = tokio::spawn(async move { pipeline::run_pipeline(input).await });
        session.add_task(handle).await;
    }

    if let Some(rx) = sys_lq {
        let vad = gravai_audio::vad::create_vad(&config.vad).map_err(|e| format!("VAD: {e}"))?;
        let input = pipeline::PipelineInput {
            rx,
            source: "system_audio".into(),
            vad,
            transcriber: transcriber.clone(),
            echo_suppressor: echo_suppressor.clone(),
            diarizer: diarizer.clone(),
            config: pipeline::PipelineConfig::from_app_config(&config),
            on_utterance: on_utterance.clone(),
            active: pipeline_active.clone(),
        };
        let handle = tokio::spawn(async move { pipeline::run_pipeline(input).await });
        session.add_task(handle).await;
    }

    // Real-time auto-save task (crash-safe transcript export every 30s)
    if config.export.realtime_save {
        let save_sid = session_id.clone();
        let save_session = session.clone();
        let save_config = config.export.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                if !save_session.is_active() {
                    break;
                }

                let export_dir = save_config
                    .transcript_folder
                    .as_ref()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| gravai_config::data_dir().join("exports"));
                let _ = std::fs::create_dir_all(&export_dir);

                if let Ok(db) = shared_db() {
                    if let Ok(utterances) = db.get_utterances(&save_sid) {
                        if !utterances.is_empty() {
                            let bookmarks = db.list_bookmarks(&save_sid).unwrap_or_default();
                            let data = gravai_export::ExportData {
                                session_id: save_sid.clone(),
                                title: None,
                                started_at: utterances[0].timestamp.clone(),
                                ended_at: None,
                                duration_seconds: None,
                                meeting_app: None,
                                utterances: utterances
                                    .iter()
                                    .map(|u| gravai_export::ExportUtterance {
                                        timestamp: u.timestamp.clone(),
                                        source: u.source.clone(),
                                        speaker: u.speaker.clone(),
                                        text: u.text.clone(),
                                    })
                                    .collect(),
                                bookmarks: bookmarks
                                    .iter()
                                    .map(|b| gravai_export::ExportBookmark {
                                        offset_ms: b.offset_ms,
                                        note: b.note.clone(),
                                    })
                                    .collect(),
                                summary: None,
                            };
                            let md = gravai_export::markdown::export_markdown(
                                &data,
                                &gravai_export::ExportOptions::default(),
                            );
                            let path = export_dir.join(format!("{save_sid}.md"));
                            let _ = std::fs::write(&path, &md);
                        }
                    }
                }
            }
        });
        session.add_task(handle).await;
    }

    state.event_bus.publish(GravaiEvent::SessionStateChanged {
        state: "recording".into(),
        session_id: Some(session_id.clone()),
    });

    // Spawn meeting-close monitor: if a meeting app was detected at start,
    // watch for it to disappear and alert the user (do NOT auto-stop).
    if let Some(ref app_name) = meeting_app {
        let app = app_name.clone();
        let bus = state.event_bus.clone();
        let session_ref = session.clone();
        // Run meeting monitor on a dedicated OS thread (SCK is not Send)
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(10));
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
                if !session_ref.is_active() {
                    break;
                }
                let current = gravai_meeting::detector::detect_meeting_apps();
                let still_running = current.iter().any(|m| m.app_name == app);
                if !still_running {
                    info!("Meeting app '{app}' closed while recording — notifying user");
                    bus.publish(GravaiEvent::Error {
                        message: format!(
                            "{app} closed. Recording continues — stop manually when ready."
                        ),
                    });
                    break;
                }
            }
        });
    }

    info!("Session {session_id} started");

    Ok(serde_json::json!({
        "id": session_id,
        "state": "recording",
        "title": session_title,
        "meeting_app": meeting_app,
    }))
}

/// Pause the active recording session.
#[tauri::command]
pub async fn pause_session(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let session_lock = state.session.read().await;
    let session = session_lock.as_ref().ok_or("No active session")?;

    if session.state() != SessionState::Recording {
        return Err("Session is not recording".into());
    }

    session.set_state(SessionState::Paused);
    state.event_bus.publish(GravaiEvent::SessionStateChanged {
        state: "paused".into(),
        session_id: Some(session.id.clone()),
    });

    info!("Session {} paused", session.id);
    Ok(serde_json::json!({"state": "paused"}))
}

/// Resume a paused session.
#[tauri::command]
pub async fn resume_session(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let session_lock = state.session.read().await;
    let session = session_lock.as_ref().ok_or("No active session")?;

    if session.state() != SessionState::Paused {
        return Err("Session is not paused".into());
    }

    session.set_state(SessionState::Recording);
    state.event_bus.publish(GravaiEvent::SessionStateChanged {
        state: "recording".into(),
        session_id: Some(session.id.clone()),
    });

    info!("Session {} resumed", session.id);
    Ok(serde_json::json!({"state": "recording"}))
}

/// Stop the active recording session.
#[tauri::command]
pub async fn stop_session(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let session = {
        let session_lock = state.session.read().await;
        session_lock.as_ref().ok_or("No active session")?.clone()
    };

    if !session.is_active() {
        return Err("Session is not active".into());
    }

    let duration = session.duration_seconds();
    session.set_state(SessionState::Stopped);
    session.abort_tasks().await;

    // Signal the capture thread to stop and release mic + SCK resources
    if let Some(stop_flag) = get_capture_stop().lock().unwrap().take() {
        stop_flag.store(true, Ordering::SeqCst);
        info!("Signaled capture thread to release audio resources");
    }

    // Update session record in DB
    if let Ok(db) = shared_db() {
        if let Err(e) = db.update_session_state(
            &session.id,
            "stopped",
            Some(&chrono::Utc::now().to_rfc3339()),
            Some(duration),
        ) {
            error!("Failed to update session: {e}");
        }
    }

    state.event_bus.publish(GravaiEvent::SessionStateChanged {
        state: "stopped".into(),
        session_id: Some(session.id.clone()),
    });

    info!(
        "Session {} stopped (duration: {:.1}s)",
        session.id, duration
    );

    Ok(serde_json::json!({
        "id": session.id,
        "state": "stopped",
        "duration_seconds": duration,
    }))
}

/// Get transcript for a session.
#[tauri::command]
pub async fn get_transcript(session_id: String) -> Result<serde_json::Value, String> {
    let db = shared_db()?;
    let utterances = db.get_utterances(&session_id).map_err(|e| e.to_string())?;
    serde_json::to_value(&utterances).map_err(|e| e.to_string())
}

/// Get transcript utterances added after a given id (exclusive). Used for incremental live-poll.
#[tauri::command]
pub async fn get_transcript_since(
    session_id: String,
    after_id: i64,
) -> Result<serde_json::Value, String> {
    let db = shared_db()?;
    let utterances = db
        .get_utterances_since(&session_id, after_id)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&utterances).map_err(|e| e.to_string())
}

/// Search utterances across all sessions.
#[tauri::command]
pub async fn search_utterances(query: String) -> Result<serde_json::Value, String> {
    let db = shared_db()?;
    let results = db.search_utterances(&query).map_err(|e| e.to_string())?;
    serde_json::to_value(&results).map_err(|e| e.to_string())
}

/// Rename a speaker within a session — updates every utterance whose speaker
/// exactly matches `old_speaker`.
#[tauri::command]
pub async fn rename_speaker_in_session(
    session_id: String,
    old_speaker: String,
    new_speaker: String,
) -> Result<serde_json::Value, String> {
    let new_speaker = new_speaker.trim().to_string();
    if new_speaker.is_empty() {
        return Err("Speaker name cannot be empty".into());
    }
    let db = shared_db()?;
    let count = db
        .rename_speaker_in_session(&session_id, &old_speaker, &new_speaker)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "updated": count }))
}

/// Return distinct speaker names from all sessions for autocomplete suggestions.
#[tauri::command]
pub async fn get_speaker_suggestions() -> Result<serde_json::Value, String> {
    let db = shared_db()?;
    let speakers = db.get_distinct_speakers().map_err(|e| e.to_string())?;
    serde_json::to_value(&speakers).map_err(|e| e.to_string())
}

/// List all past sessions.
#[tauri::command]
pub async fn list_sessions() -> Result<serde_json::Value, String> {
    let db = shared_db()?;
    let sessions = db.list_sessions().map_err(|e| e.to_string())?;
    serde_json::to_value(&sessions).map_err(|e| e.to_string())
}

/// Detect running meeting apps.
#[tauri::command]
pub async fn detect_meetings() -> Result<serde_json::Value, String> {
    let meetings = gravai_meeting::detector::detect_meeting_apps();
    serde_json::to_value(&meetings).map_err(|e| e.to_string())
}

/// Rename a session — updates the `title` column in the sessions table.
#[tauri::command]
pub async fn rename_session(session_id: String, title: String) -> Result<(), String> {
    let title = title.trim().to_string();
    let db = shared_db()?;
    db.rename_session(&session_id, &title)
        .map_err(|e| e.to_string())
}

/// One-per-session actor that owns the correction batch + debounce timer.
/// Receives utterance ids on an unbounded channel; fires a correction task
/// when the batch fills OR when `debounce_secs` elapse with no new ids.
/// Replaces the previous per-utterance `tokio::spawn(debounce)` storm.
async fn run_correction_actor(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<i64>,
    provider: Arc<gravai_intelligence::TranscriptCorrectionProvider>,
    event_bus: gravai_core::EventBus,
    session_id: String,
    batch_size: usize,
    debounce_secs: u64,
    timeout_secs: u64,
) {
    let mut pending: Vec<i64> = Vec::new();
    let debounce = std::time::Duration::from_secs(debounce_secs);

    loop {
        if pending.is_empty() {
            // Idle — block on the next id (no timer running).
            match rx.recv().await {
                Some(id) => pending.push(id),
                None => return,
            }
            if pending.len() >= batch_size {
                let batch = std::mem::take(&mut pending);
                spawn_correction(
                    batch,
                    provider.clone(),
                    event_bus.clone(),
                    session_id.clone(),
                    timeout_secs,
                );
            }
        } else {
            // Have pending ids — race the next id against the debounce timer.
            let sleep = tokio::time::sleep(debounce);
            tokio::select! {
                maybe_id = rx.recv() => {
                    match maybe_id {
                        Some(id) => {
                            pending.push(id);
                            if pending.len() >= batch_size {
                                let batch = std::mem::take(&mut pending);
                                spawn_correction(
                                    batch,
                                    provider.clone(),
                                    event_bus.clone(),
                                    session_id.clone(),
                                    timeout_secs,
                                );
                            }
                        }
                        None => {
                            // Sender dropped — flush and exit.
                            let batch = std::mem::take(&mut pending);
                            spawn_correction(
                                batch,
                                provider.clone(),
                                event_bus.clone(),
                                session_id.clone(),
                                timeout_secs,
                            );
                            return;
                        }
                    }
                }
                _ = sleep => {
                    let batch = std::mem::take(&mut pending);
                    spawn_correction(
                        batch,
                        provider.clone(),
                        event_bus.clone(),
                        session_id.clone(),
                        timeout_secs,
                    );
                }
            }
        }
    }
}

fn spawn_correction(
    batch: Vec<i64>,
    provider: Arc<gravai_intelligence::TranscriptCorrectionProvider>,
    event_bus: gravai_core::EventBus,
    session_id: String,
    timeout_secs: u64,
) {
    if batch.is_empty() {
        return;
    }
    tokio::spawn(async move {
        run_correction_task(batch, provider, event_bus, session_id, timeout_secs).await;
    });
}

/// Async task that corrects a batch of utterances via LLM and updates the DB.
/// Wraps `provider.correct(...)` in a timeout so a hung LLM never pins tasks.
async fn run_correction_task(
    utterance_ids: Vec<i64>,
    provider: Arc<gravai_intelligence::TranscriptCorrectionProvider>,
    event_bus: gravai_core::EventBus,
    session_id: String,
    timeout_secs: u64,
) {
    use tracing::{info, warn};

    let db = match shared_db() {
        Ok(db) => db,
        Err(e) => {
            warn!("Correction: shared_db unavailable: {e}");
            return;
        }
    };

    // Mark utterances as pending.
    let _ = db.mark_utterances_correction_pending(&utterance_ids);

    // Load knowledge entries and utterance records.
    let knowledge = db.list_knowledge_entries(true).unwrap_or_default();
    let utterances = db.get_utterances_by_ids(&utterance_ids).unwrap_or_default();

    if utterances.is_empty() {
        return;
    }

    info!(
        "Running correction on {} utterances (session {})",
        utterances.len(),
        session_id
    );

    let correct_fut = provider.correct(&utterances, &knowledge);
    let timeout = std::time::Duration::from_secs(timeout_secs.max(1));
    let result = tokio::time::timeout(timeout, correct_fut).await;

    match result {
        Ok(Ok(corrections)) => {
            // Resolve every utterance in the batch. Ids the LLM echoed back get
            // their corrected text; ids it omitted (very common with chatty
            // models like Gemma) are marked `done` with the original text so
            // they don't stay `pending` forever.
            let mut corrected_ids = Vec::new();
            for utt in &utterances {
                let corrected_text = corrections
                    .get(&utt.id)
                    .cloned()
                    .unwrap_or_else(|| utt.text.clone());
                let _ = db.update_utterance_correction(
                    utt.id,
                    &corrected_text,
                    &provider.provider_name,
                    "done",
                );
                corrected_ids.push(utt.id);
            }
            if !corrected_ids.is_empty() {
                event_bus.publish(gravai_core::GravaiEvent::TranscriptCorrected {
                    session_id,
                    utterance_ids: corrected_ids,
                });
            }
        }
        Ok(Err(e)) => {
            warn!("Correction task failed: {e}");
            for id in &utterance_ids {
                let _ = db.mark_utterance_correction_error(*id, &provider.provider_name);
            }
        }
        Err(_elapsed) => {
            warn!(
                "Correction task timed out after {}s on batch of {} utterances",
                timeout.as_secs(),
                utterance_ids.len()
            );
            for id in &utterance_ids {
                let _ = db.mark_utterance_correction_error(*id, &provider.provider_name);
            }
        }
    }
}

/// Record audio chunks from a receiver to a named track file.
async fn record_track(
    mut rx: mpsc::Receiver<AudioChunk>,
    recorder: Arc<std::sync::Mutex<MultiTrackRecorder>>,
    track_name: &str,
) {
    while let Some(chunk) = rx.recv().await {
        let mut rec = recorder.lock().unwrap();
        if let Err(e) = rec.write_track(track_name, &chunk) {
            error!("Write track {track_name}: {e}");
        }
    }
}
