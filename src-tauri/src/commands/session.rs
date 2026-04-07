//! Session lifecycle commands: start, pause, resume, stop.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gravai_audio::capture::{AudioCaptureManager, AudioChunk, VolumeCallback};
use gravai_audio::echo::EchoSuppressor;
use gravai_audio::pipeline;
use gravai_audio::recorder::MultiTrackRecorder;
use gravai_core::{AppState, GravaiEvent, Session, SessionState};
use tauri::State;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Start a new recording session.
#[tauri::command]
pub async fn start_session(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    // Check no active session
    {
        let current = state.session.read().await;
        if let Some(ref s) = *current {
            if s.is_active() {
                return Err("A session is already active".into());
            }
        }
    }

    let config = state.config.read().await.clone();
    let session_id = gravai_core::session::generate_session_id();
    let session = Arc::new(Session::new(session_id.clone(), config.clone()));
    session.set_state(SessionState::Recording);

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
        let recording_sr = config.audio.recording.sample_rate;
        let recording_ch = config.audio.recording.channels;
        let mut rec = MultiTrackRecorder::new(&output_dir, recording_sr, recording_ch)
            .map_err(|e| format!("Recorder: {e}"))?;

        if config.audio.microphone.enabled {
            rec.add_track("mic")
                .map_err(|e| format!("Add mic track: {e}"))?;
        }
        if config.audio.system_audio.enabled {
            rec.add_track("system")
                .map_err(|e| format!("Add system track: {e}"))?;
        }
        rec.init_master().map_err(|e| format!("Init master: {e}"))?;
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

    std::thread::spawn(move || {
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

        // Keep capture alive until thread is unparked
        std::thread::park();
    });

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

    // Set up transcription pipeline for LQ (16kHz mono) streams
    let transcriber: Option<Arc<dyn gravai_transcription::TranscriptionProvider>> =
        match gravai_transcription::create_provider(&config.transcription) {
            Ok(provider) => Some(Arc::from(provider)),
            Err(e) => {
                warn!("Transcription not available: {e}");
                None
            }
        };

    let echo_suppressor = Arc::new(tokio::sync::Mutex::new(EchoSuppressor::new(
        config.features.echo_suppression.similarity_threshold,
    )));
    let pipeline_active = Arc::new(AtomicBool::new(true));

    // Create diarizer if enabled
    let diarizer: Option<
        Arc<tokio::sync::Mutex<Box<dyn gravai_intelligence::DiarizationProvider>>>,
    > = if config.features.diarization.enabled {
        let d = gravai_intelligence::diarization::create_diarizer(&config.features.diarization);
        info!("Diarization enabled ({} engine)", d.name());
        Some(Arc::new(tokio::sync::Mutex::new(d)))
    } else {
        None
    };

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
            };
            match db.insert_utterance(&record) {
                Ok(id) => {
                    event_bus.publish(GravaiEvent::TranscriptUpdated {
                        session_id: sid.clone(),
                        utterance_id: id,
                        source: utterance.source.clone(),
                        text: utterance.text.clone(),
                        timestamp,
                    });
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
            diarizer: diarizer.clone(),
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
            diarizer: diarizer.clone(),
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

    // Auto-name session from calendar events
    let calendar_title = gravai_meeting::calendar::find_meeting_title(
        config.features.meeting_detection.lead_time_seconds,
    );

    // Check for running meeting apps
    let meeting_app = {
        let meetings = gravai_meeting::detector::detect_meeting_apps();
        meetings.first().map(|m| m.app_name.clone())
    };

    // Store session
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

        let mut session_lock = state.session.write().await;
        *session_lock = Some(session.clone());
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

/// Record audio chunks from a receiver to a named track + master.
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
        if let Err(e) = rec.write_master(&chunk) {
            error!("Write master from {track_name}: {e}");
        }
    }
}
