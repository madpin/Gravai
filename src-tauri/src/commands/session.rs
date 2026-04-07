//! Session lifecycle commands: start, pause, resume, stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
    let _start_guard = SessionStartGuard::try_acquire()
        .ok_or("A session is already starting")?;

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
            if let Some(enabled) = profile.diarization_enabled {
                config.features.diarization.enabled = enabled;
            }
            if let Some(enabled) = profile.echo_suppression_enabled {
                config.features.echo_suppression.enabled = enabled;
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

    let session_id = gravai_core::session::generate_session_id();
    let session = Arc::new(Session::new(session_id.clone(), config.clone()));
    session.set_state(SessionState::Recording);

    // Auto-name session from calendar events (before DB write)
    let calendar_title = gravai_meeting::calendar::find_meeting_title(
        config.features.meeting_detection.lead_time_seconds,
    );

    // Check for running meeting apps
    let meeting_app = {
        let meetings = gravai_meeting::detector::detect_meeting_apps();
        meetings.first().map(|m| m.app_name.clone())
    };

    // Store session in DB early — pipelines may insert utterances before this block was reached
    {
        let db_path = gravai_config::data_dir().join("gravai.db");
        if let Ok(db) = gravai_storage::Database::open(&db_path) {
            let record = gravai_storage::SessionRecord {
                id: session_id.clone(),
                started_at: session.started_at.to_rfc3339(),
                ended_at: None,
                duration_seconds: None,
                title: calendar_title.clone(),
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

        let volume_cb: VolumeCallback = Arc::new(move |source: &str, db: f64| {
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

    // Load transcription provider on a blocking thread (model loading is slow for large models)
    let trans_config = config.transcription.clone();
    let trans_bus = state.event_bus.clone();
    let transcriber: Option<Arc<dyn gravai_transcription::TranscriptionProvider>> =
        match tokio::task::spawn_blocking(move || {
            gravai_transcription::create_provider(&trans_config)
        })
        .await
        {
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

    // Speaker labels: mic = "You", system = "Remote" (always, no diarizer needed)

    // Load sentiment engine (go-emotions ONNX) — only if model files are present
    let sentiment_engine: Option<Arc<dyn gravai_intelligence::SentimentEngine>> =
        tokio::task::spawn_blocking(|| {
            gravai_intelligence::OnnxSentimentEngine::try_load()
                .map(|e| Arc::new(e) as Arc<dyn gravai_intelligence::SentimentEngine>)
        })
        .await
        .unwrap_or(None);

    // Callback that writes utterances to DB and publishes events
    let event_bus = state.event_bus.clone();
    let sid = session_id.clone();
    let db_path = gravai_config::data_dir().join("gravai.db");
    let on_utterance: pipeline::OnUtterance = Arc::new(move |utterance| {
        let timestamp = utterance.timestamp.to_rfc3339();

        // Write to database
        if let Ok(db) = gravai_storage::Database::open(&db_path) {
            let record = gravai_storage::UtteranceRecord {
                id: 0,
                session_id: sid.clone(),
                timestamp: timestamp.clone(),
                source: utterance.source.clone(),
                speaker: utterance.speaker.clone(),
                text: utterance.text.clone(),
                confidence: None,
                start_ms: None,
                end_ms: None,
                sentiment_label: None,
                sentiment_score: None,
                emotions_json: None,
            };
            match db.insert_utterance(&record) {
                Ok(id) => {
                    event_bus.publish(GravaiEvent::TranscriptUpdated {
                        session_id: sid.clone(),
                        utterance_id: id,
                        source: utterance.source.clone(),
                        speaker: utterance.speaker.clone(),
                        text: utterance.text.clone(),
                        timestamp,
                    });

                    // Run sentiment on system audio only (async, non-blocking)
                    if (utterance.source == "system_audio" || utterance.source == "system")
                        && sentiment_engine.is_some()
                    {
                        let engine = sentiment_engine.as_ref().unwrap().clone();
                        let text = utterance.text.clone();
                        let db_path_clone = db_path.clone();
                        tokio::spawn(async move {
                            let result =
                                tokio::task::spawn_blocking(move || engine.analyze(&text)).await;
                            if let Ok(sentiment) = result {
                                let emotions_json = sentiment
                                    .emotions
                                    .as_ref()
                                    .and_then(|e| serde_json::to_string(e).ok());
                                if let Ok(db) = gravai_storage::Database::open(&db_path_clone) {
                                    let _ = db.update_utterance_sentiment(
                                        id,
                                        &sentiment.label,
                                        sentiment.score,
                                        emotions_json.as_deref(),
                                    );
                                }
                            }
                        });
                    }
                }
                Err(e) => error!("Insert utterance: {e}"),
            }
        }
    });

    // Spawn transcription pipelines for each LQ source
    if let Some(rx) = mic_lq {
        let vad = gravai_audio::vad::create_vad(&config.vad).map_err(|e| format!("VAD: {e}"))?;
        let input = pipeline::PipelineInput {
            rx,
            source: "microphone".into(),
            vad,
            transcriber: transcriber.clone(),
            echo_suppressor: echo_suppressor.clone(),
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

                let db_path = gravai_config::data_dir().join("gravai.db");
                if let Ok(db) = gravai_storage::Database::open(&db_path) {
                    if let Ok(utterances) = db.get_utterances(&save_sid) {
                        if !utterances.is_empty() {
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
        "title": calendar_title,
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
    let db_path = gravai_config::data_dir().join("gravai.db");
    if let Ok(db) = gravai_storage::Database::open(&db_path) {
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
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let utterances = db.get_utterances(&session_id).map_err(|e| e.to_string())?;
    serde_json::to_value(&utterances).map_err(|e| e.to_string())
}

/// Search utterances across all sessions.
#[tauri::command]
pub async fn search_utterances(query: String) -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let results = db.search_utterances(&query).map_err(|e| e.to_string())?;
    serde_json::to_value(&results).map_err(|e| e.to_string())
}

/// List all past sessions.
#[tauri::command]
pub async fn list_sessions() -> Result<serde_json::Value, String> {
    let db_path = gravai_config::data_dir().join("gravai.db");
    let db = gravai_storage::Database::open(&db_path).map_err(|e| e.to_string())?;
    let sessions = db.list_sessions().map_err(|e| e.to_string())?;
    serde_json::to_value(&sessions).map_err(|e| e.to_string())
}

/// Detect running meeting apps.
#[tauri::command]
pub async fn detect_meetings() -> Result<serde_json::Value, String> {
    let meetings = gravai_meeting::detector::detect_meeting_apps();
    serde_json::to_value(&meetings).map_err(|e| e.to_string())
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
