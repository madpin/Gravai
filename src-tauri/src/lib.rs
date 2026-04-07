//! Gravai Tauri application — thin glue layer between core crates and the UI.

mod commands;

use gravai_config::load_config;
use gravai_core::{AppState, GravaiEvent};
use std::sync::Arc;
use tauri::Emitter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    gravai_core::logging::init_logging();
    gravai_core::perf::init();
    tracing::info!("Gravai starting up");

    // Load config
    let config = load_config();

    // Run preflight checks in background
    let preflight_config = config.clone();
    std::thread::spawn(move || {
        gravai_core::preflight::run_preflight_checks(&preflight_config);
    });

    // Ensure data directories exist
    let _ = std::fs::create_dir_all(gravai_config::data_dir());
    let _ = std::fs::create_dir_all(gravai_config::sessions_dir());
    let _ = std::fs::create_dir_all(gravai_config::models_dir());

    // Download required models in background (non-blocking)
    let model_config = config.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime for model download");
        rt.block_on(gravai_models::ensure_models(&model_config));
    });

    // Create shared app state
    let app_state = Arc::new(AppState::new(config));

    // Clone for the event bridge
    let event_bus = app_state.event_bus.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .setup(move |app| {
            // Bridge EventBus → Tauri frontend events
            let handle = app.handle().clone();
            let mut rx = event_bus.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            let event_name = match &event {
                                GravaiEvent::VolumeLevel { .. } => "gravai:volume",
                                GravaiEvent::TranscriptUpdated { .. } => "gravai:transcript",
                                GravaiEvent::SessionStateChanged { .. } => "gravai:session",
                                GravaiEvent::MeetingDetected { .. } => "gravai:meeting",
                                GravaiEvent::Error { .. } => "gravai:error",
                                _ => "gravai:event",
                            };
                            let _ = handle.emit(event_name, &event);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("Event bridge lagged by {n} events");
                        }
                        Err(_) => break,
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::get_config,
            commands::update_config,
            commands::export_config,
            commands::import_config,
            commands::get_recent_logs,
            commands::get_health_report,
            commands::list_audio_devices,
            commands::list_running_apps,
            commands::set_source_gain,
            commands::start_session,
            commands::pause_session,
            commands::resume_session,
            commands::stop_session,
            commands::get_transcript,
            commands::search_utterances,
            commands::list_sessions,
            commands::detect_meetings,
            // Phase 3: Intelligence
            commands::summarize_session,
            commands::get_export_formats,
            commands::export_session_audio,
            // Phase 3: Presets, profiles, shortcuts, automations
            commands::get_presets,
            commands::activate_preset,
            commands::save_preset,
            commands::delete_preset,
            commands::get_profiles,
            commands::activate_profile,
            commands::save_profile,
            commands::get_shortcuts,
            commands::rebind_shortcut,
            commands::get_automations,
            commands::toggle_automation,
            commands::save_automation,
            // Phase 4: Search + Chat + Export
            commands::generate_embeddings,
            commands::semantic_search,
            commands::hybrid_search,
            commands::search_sessions_filtered,
            commands::ask_gravai,
            commands::get_chat_history,
            commands::export_markdown,
            commands::export_markdown_file,
            commands::export_pdf,
            commands::export_obsidian,
            commands::export_notion,
            // Phase 5: Tools
            commands::detect_silence,
            commands::trim_silence,
            commands::get_perf_snapshot,
            // Storage management
            commands::get_storage_info,
            commands::delete_session_audio,
            commands::delete_full_session,
            commands::save_realtime_transcript,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Gravai");
}
