//! Gravai Tauri application — thin glue layer between core crates and the UI.

mod commands;

use gravai_config::load_config;
use gravai_core::{AppState, GravaiEvent};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Emitter;
use tauri::Manager;

/// Build a 22×22 RGBA waveform icon for the macOS menu bar.
///
/// Produces black pixels on a transparent background — `icon_as_template(true)`
/// then lets macOS invert it automatically for dark/light menu bars.
/// Five vertical bars of varying height form a classic audio waveform shape.
fn make_tray_icon() -> tauri::image::Image<'static> {
    const W: usize = 22;
    const H: usize = 22;
    let mut px = vec![0u8; W * H * 4]; // all transparent

    // (x_start, bar_width, bar_height) — centred vertically
    let bars: &[(usize, usize, usize)] =
        &[(1, 3, 6), (5, 3, 12), (9, 3, 18), (13, 3, 12), (17, 3, 6)];

    for &(bx, bw, bh) in bars {
        let top = (H - bh) / 2;
        for y in top..top + bh {
            for x in bx..bx + bw {
                let i = (y * W + x) * 4;
                px[i] = 0;
                px[i + 1] = 0;
                px[i + 2] = 0;
                px[i + 3] = 255;
            }
        }
    }

    tauri::image::Image::new_owned(px, W as u32, H as u32)
}

/// Execute a list of automation actions by emitting events to the frontend.
/// `currently_recording` is the live recording state at the time of the call.
fn run_automation_actions(
    handle: &tauri::AppHandle,
    actions: &[gravai_config::automations::AutomationAction],
    currently_recording: bool,
) {
    use gravai_config::automations::AutomationAction;
    for action in actions {
        match action {
            AutomationAction::StartRecording => {
                if !currently_recording {
                    tracing::info!("Automation: starting recording");
                    let _ = handle.emit("gravai:automation-start", ());
                }
            }
            AutomationAction::StopRecording => {
                if currently_recording {
                    tracing::info!("Automation: stopping recording");
                    let _ = handle.emit("gravai:stop-session", ());
                }
            }
            AutomationAction::ShowNotification { message } => {
                #[cfg(target_os = "macos")]
                {
                    use tauri_plugin_notification::NotificationExt;
                    let _ = handle
                        .notification()
                        .builder()
                        .title("Gravai Automation")
                        .body(message)
                        .show();
                }
            }
            AutomationAction::ActivateProfile { profile_id } => {
                // Profile activation is fire-and-forget via the frontend
                let _ = handle.emit("gravai:automation-activate-profile", profile_id);
            }
            // Other actions (ActivatePreset, RunExport) are handled by the frontend
            _ => {
                let _ = handle.emit(
                    "gravai:automation-action",
                    serde_json::to_value(action).ok(),
                );
            }
        }
    }
}

/// Holds dynamic tray menu item references so the event bridge can update them.
struct TrayItems {
    start: tauri::menu::MenuItem<tauri::Wry>,
    stop: tauri::menu::MenuItem<tauri::Wry>,
    status: tauri::menu::MenuItem<tauri::Wry>,
}
type TrayItemsState = Arc<std::sync::Mutex<TrayItems>>;

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
    let app_state = Arc::new(AppState::new(config.clone()));

    // Clone event bus for setup closure
    let event_bus = app_state.event_bus.clone();

    // ── Background meeting detection loop ────────────────────────────────
    // Publishes GravaiEvent::MeetingDetected / MeetingEnded to the event bus,
    // which the event bridge picks up and uses to fire automations.
    {
        let meeting_bus = app_state.event_bus.clone();
        let meeting_config = config.features.meeting_detection.clone();
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        tauri::async_runtime::spawn(async move {
            gravai_meeting::detector::run_detection_loop(meeting_config, meeting_bus, active).await;
        });
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(app_state)
        .setup(move |app| {
            // ── System Tray ──────────────────────────────────────────────
            {
                use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let show_item     = MenuItemBuilder::with_id("show",   "Show Gravai").build(app)?;
                let status_item   = MenuItemBuilder::with_id("status", "◉  Idle").enabled(false).build(app)?;
                let start_item    = MenuItemBuilder::with_id("start",  "⏺  Start Recording").enabled(true).build(app)?;
                let stop_item     = MenuItemBuilder::with_id("stop",   "⏹  Stop Recording").enabled(false).build(app)?;
                let recording_tab = MenuItemBuilder::with_id("go_rec", "Open Recording Tab").build(app)?;
                let models_tab    = MenuItemBuilder::with_id("go_models", "Open Models Tab").build(app)?;
                let quit_item     = MenuItemBuilder::with_id("quit",   "Quit Gravai").build(app)?;
                let sep1 = PredefinedMenuItem::separator(app)?;
                let sep2 = PredefinedMenuItem::separator(app)?;
                let sep3 = PredefinedMenuItem::separator(app)?;

                let menu = MenuBuilder::new(app)
                    .item(&show_item)
                    .item(&sep1)
                    .item(&status_item)
                    .item(&start_item)
                    .item(&stop_item)
                    .item(&sep2)
                    .item(&recording_tab)
                    .item(&models_tab)
                    .item(&sep3)
                    .item(&quit_item)
                    .build()?;


                let handle_for_tray_click = app.handle().clone();
                let handle_for_menu = app.handle().clone();

                TrayIconBuilder::with_id("gravai-tray")
                    .icon(make_tray_icon())
                    .icon_as_template(true)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .tooltip("Gravai")
                    .on_tray_icon_event(move |_tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            if let Some(win) = handle_for_tray_click.get_webview_window("main") {
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                    })
                    .on_menu_event(move |app, event| {
                        match event.id().as_ref() {
                            "show" => {
                                if let Some(win) = app.get_webview_window("main") {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                }
                            }
                            "start" => {
                                // Optimistically update tray before session starts
                                if let Some(items) = app.try_state::<TrayItemsState>() {
                                    if let Ok(t) = items.lock() {
                                        let _ = t.start.set_enabled(false);
                                        let _ = t.status.set_text("⏳  Starting...");
                                    }
                                }
                                if let Some(win) = app.get_webview_window("main") {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                    let _ = win.emit("gravai:automation-start", ());
                                }
                            }
                            "stop" => {
                                // Optimistically update tray before session stops
                                if let Some(items) = app.try_state::<TrayItemsState>() {
                                    if let Ok(t) = items.lock() {
                                        let _ = t.stop.set_enabled(false);
                                        let _ = t.status.set_text("⏳  Stopping...");
                                    }
                                }
                                if let Some(win) = app.get_webview_window("main") {
                                    let _ = win.emit("gravai:stop-session", ());
                                }
                            }
                            "go_rec" => {
                                if let Some(win) = app.get_webview_window("main") {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                    let _ = win.emit("gravai:navigate", "recording");
                                }
                            }
                            "go_models" => {
                                if let Some(win) = app.get_webview_window("main") {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                    let _ = win.emit("gravai:navigate", "models");
                                }
                            }
                            "quit" => {
                                use tauri_plugin_dialog::DialogExt;
                                let app_handle = handle_for_menu.clone();
                                let state = app_handle.state::<Arc<AppState>>();
                                let state_clone = state.inner().clone();
                                tauri::async_runtime::spawn(async move {
                                    let is_recording = {
                                        let session = state_clone.session.read().await;
                                        session.as_ref().map(|s| s.is_active()).unwrap_or(false)
                                    };
                                    if is_recording {
                                        app_handle.dialog()
                                            .message("A recording is in progress. Quitting will stop it. Are you sure?")
                                            .title("Quit Gravai?")
                                            .kind(tauri_plugin_dialog::MessageDialogKind::Warning)
                                            .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancel)
                                            .show(move |confirmed| {
                                                if confirmed { app_handle.exit(0); }
                                            });
                                    } else {
                                        app_handle.exit(0);
                                    }
                                });
                            }
                            _ => {}
                        }
                    })
                    .build(app)?;

                // Bundle menu items into managed state for dynamic updates
                let tray_items: TrayItemsState = Arc::new(std::sync::Mutex::new(TrayItems { start: start_item, stop: stop_item, status: status_item }));
                app.manage(tray_items);
            }

            // ── Close-to-tray ─────────────────────────────────────────────
            if let Some(main_window) = app.get_webview_window("main") {
                let win_clone = main_window.clone();
                main_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_clone.hide();
                    }
                });
            }

            // ── Event Bridge → Frontend + Tray state ─────────────────────
            let handle = app.handle().clone();
            let mut rx = event_bus.subscribe();

            // Shared state for silence monitoring
            let is_recording = Arc::new(AtomicBool::new(false));
            let last_audio_time = Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
            let silence_alerted = Arc::new(AtomicBool::new(false));

            let is_recording_silence = is_recording.clone();
            let last_audio_silence = last_audio_time.clone();
            let silence_alerted_clone = silence_alerted.clone();
            let handle_for_silence = handle.clone();

            // Silence monitor task — checks every 2s
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    if !is_recording_silence.load(Ordering::Relaxed) {
                        continue;
                    }
                    let elapsed = last_audio_silence
                        .lock()
                        .unwrap()
                        .elapsed()
                        .as_secs();
                    if elapsed >= 10 && !silence_alerted_clone.load(Ordering::Relaxed) {
                        silence_alerted_clone.store(true, Ordering::Relaxed);
                        // Emit to frontend
                        let _ = handle_for_silence.emit("gravai:silence-alert", serde_json::json!({
                            "message": "No audio detected on mic or system for 10+ seconds",
                            "elapsed": elapsed
                        }));
                        // Also send macOS notification
                        #[cfg(target_os = "macos")]
                        {
                            use tauri_plugin_notification::NotificationExt;
                            let _ = handle_for_silence
                                .notification()
                                .builder()
                                .title("Gravai — Silence Detected")
                                .body("No audio detected for 10+ seconds. Check your microphone and system audio.")
                                .show();
                        }
                    }
                }
            });

            // Main event bridge loop
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            let (event_name, payload) = match &event {
                                GravaiEvent::VolumeLevel { source, db } => {
                                    // Update last audio time if either channel is active
                                    if *db > -50.0 {
                                        *last_audio_time.lock().unwrap() = std::time::Instant::now();
                                        silence_alerted.store(false, Ordering::Relaxed);
                                    }
                                    (
                                        "gravai:volume",
                                        serde_json::json!({ "source": source, "db": db }),
                                    )
                                }
                                GravaiEvent::TranscriptUpdated { session_id, utterance_id, source, speaker, text, timestamp } => (
                                    "gravai:transcript",
                                    serde_json::json!({ "session_id": session_id, "utterance_id": utterance_id, "source": source, "speaker": speaker, "text": text, "timestamp": timestamp }),
                                ),
                                GravaiEvent::SessionStateChanged { state, session_id } => {
                                    let recording = state == "recording";
                                    is_recording.store(recording, Ordering::Relaxed);
                                    if recording {
                                        *last_audio_time.lock().unwrap() = std::time::Instant::now();
                                        silence_alerted.store(false, Ordering::Relaxed);
                                    }
                                    // Update tray tooltip and dynamic menu items
                                    if let Some(tray) = handle.tray_by_id("gravai-tray") {
                                        let tooltip = if recording { "Gravai — Recording 🔴" } else { "Gravai" };
                                        let _ = tray.set_tooltip(Some(tooltip));
                                    }
                                    // Update stop button and status label dynamically
                                    if let Some(items) = handle.try_state::<TrayItemsState>() {
                                        if let Ok(t) = items.lock() {
                                            let _ = t.start.set_enabled(!recording);
                                            let _ = t.stop.set_enabled(recording);
                                            let _ = t.status.set_text(if recording { "🔴  Recording" } else { "◉  Idle" });
                                        }
                                    }
                                    (
                                        "gravai:session",
                                        serde_json::json!({ "state": state, "session_id": session_id }),
                                    )
                                }
                                GravaiEvent::MeetingDetected { app_name, window_title } => {
                                    // Fire any automations triggered by this meeting app
                                    let store = gravai_config::automations::AutomationStore::load();
                                    for automation in store.find_for_meeting_detected(app_name) {
                                        run_automation_actions(&handle, &automation.actions, is_recording.load(Ordering::Relaxed));
                                    }
                                    (
                                        "gravai:meeting",
                                        serde_json::json!({ "app_name": app_name, "window_title": window_title }),
                                    )
                                }
                                GravaiEvent::MeetingEnded { app_name } => {
                                    // Fire any automations triggered by this meeting ending
                                    let store = gravai_config::automations::AutomationStore::load();
                                    for automation in store.find_for_meeting_ended(app_name) {
                                        run_automation_actions(&handle, &automation.actions, is_recording.load(Ordering::Relaxed));
                                    }
                                    (
                                        "gravai:meeting-ended",
                                        serde_json::json!({ "app_name": app_name }),
                                    )
                                }
                                GravaiEvent::Error { message } => (
                                    "gravai:error",
                                    serde_json::json!({ "message": message }),
                                ),
                                _ => ("gravai:event", serde_json::json!({})),
                            };
                            let _ = handle.emit(event_name, &payload);
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
            commands::list_capturable_apps,
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
            commands::get_session_sentiment,
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
            // Model management
            commands::get_models_status,
            commands::download_model,
            commands::delete_model,
            // Storage management
            commands::get_storage_info,
            commands::delete_session_audio,
            commands::delete_full_session,
            commands::save_realtime_transcript,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Gravai");
}
