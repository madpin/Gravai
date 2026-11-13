//! Global keyboard shortcuts.
//!
//! Global bindings are registered with the OS and work while another app is
//! in front. Local bindings are handled by the window (StatusBar.svelte).
//! Both end up as a `gravai:shortcut` event carrying the action id.

use std::sync::Mutex;

use gravai_config::shortcuts::ShortcutStore;
use tauri::Emitter;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Registered global shortcuts and the action each one runs.
static REGISTERED: Mutex<Vec<(Shortcut, String)>> = Mutex::new(Vec::new());

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let action = REGISTERED.lock().ok().and_then(|r| {
                r.iter()
                    .find(|(s, _)| s == shortcut)
                    .map(|(_, a)| a.clone())
            });
            if let Some(action) = action {
                tracing::info!("Shortcut: {action}");
                let _ = app.emit("gravai:shortcut", action);
            }
        })
        .build()
}

/// Replace all global registrations with the global bindings in `store`.
/// Returns an error naming any key the OS refused, after registering the rest.
pub fn register(app: &tauri::AppHandle, store: &ShortcutStore) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut registered = Vec::new();
    let mut failed = Vec::new();
    for binding in store.bindings.values() {
        if !binding.is_global || binding.key_sequence.is_empty() {
            continue;
        }
        let parsed = binding.key_sequence.parse::<Shortcut>();
        match parsed.map_err(|e| e.to_string()).and_then(|s| {
            gs.register(s).map_err(|e| e.to_string())?;
            Ok(s)
        }) {
            Ok(s) => registered.push((s, binding.action_id.clone())),
            Err(e) => {
                tracing::warn!("Could not register {}: {e}", binding.key_sequence);
                failed.push(binding.key_sequence.clone());
            }
        }
    }
    if let Ok(mut r) = REGISTERED.lock() {
        *r = registered;
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} is in use by macOS or another app. Pick another key.",
            failed.join(", ")
        ))
    }
}
