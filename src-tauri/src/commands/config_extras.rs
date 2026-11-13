//! Setup, shortcut, and automation commands.

use gravai_config::setups::{Setup, SetupStore, Snapshot};
use gravai_core::AppState;
use std::sync::Arc;
use tauri::{Emitter, State};

/// Saved setups, in display order. Each says whether it matches the live
/// settings (`current`) and which models it still needs (`missing`).
#[tauri::command]
pub async fn list_setups(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let config = state.config.read().await;
    let live = serde_json::to_value(Snapshot::capture(&config)).map_err(|e| e.to_string())?;
    let setups = SetupStore::load(&config).setups;
    let matches = |s: &Setup| serde_json::to_value(&s.snapshot).ok().as_ref() == Some(&live);
    // Several setups can hold the same values. Mark one, and prefer a named
    // setup over the migration's "Settings before update".
    let current_id = setups
        .iter()
        .filter(|s| matches(s))
        .min_by_key(|s| s.id == "before-update")
        .map(|s| s.id.clone());
    let items: Vec<_> = setups
        .into_iter()
        .map(|setup| {
            serde_json::json!({
                "id": setup.id,
                "name": setup.name,
                "description": setup.description,
                "builtin": setup.builtin,
                "summary": setup.snapshot.summary(),
                "current": current_id.as_deref() == Some(setup.id.as_str()),
                "missing": super::models::missing_models(&setup.snapshot),
            })
        })
        .collect();
    Ok(serde_json::json!(items))
}

/// Save the live settings under `name`. An existing setup with that name is replaced.
#[tauri::command]
pub async fn save_current_setup(
    state: State<'_, Arc<AppState>>,
    name: String,
) -> Result<Setup, String> {
    if name.trim().is_empty() {
        return Err("Give the setup a name".into());
    }
    let config = state.config.read().await;
    let mut store = SetupStore::load(&config);
    let setup = store.save_as(&name, Snapshot::capture(&config));
    store.save().map_err(|e| e.to_string())?;
    Ok(setup)
}

#[tauri::command]
pub async fn rename_setup(
    state: State<'_, Arc<AppState>>,
    setup_id: String,
    name: String,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Give the setup a name".into());
    }
    let mut store = SetupStore::load(&*state.config.read().await);
    if !store.rename(&setup_id, &name) {
        return Err(format!("Setup not found: {setup_id}"));
    }
    store.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_setup(state: State<'_, Arc<AppState>>, setup_id: String) -> Result<(), String> {
    let mut store = SetupStore::load(&*state.config.read().await);
    store.delete(&setup_id);
    store.save().map_err(|e| e.to_string())
}

/// Bring back Meeting, Podcast, and Light if they were deleted.
#[tauri::command]
pub async fn restore_builtin_setups(state: State<'_, Arc<AppState>>) -> Result<Vec<Setup>, String> {
    let mut store = SetupStore::load(&*state.config.read().await);
    store.restore_builtins();
    store.save().map_err(|e| e.to_string())?;
    Ok(store.setups)
}

/// Write `snapshot` into the live settings and return what was there before,
/// so the window can offer Undo.
async fn apply(
    app: &tauri::AppHandle,
    state: &AppState,
    snapshot: &Snapshot,
) -> Result<Snapshot, String> {
    let mut config = state.config.write().await;
    let previous = Snapshot::capture(&config);
    snapshot.apply_to(&mut config);
    gravai_config::save_config(&config).map_err(|e| e.to_string())?;
    let _ = app.emit("gravai:config-changed", ());
    Ok(previous)
}

/// Apply a saved setup. Returns the settings it replaced.
#[tauri::command]
pub async fn apply_setup(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    setup_id: String,
) -> Result<Snapshot, String> {
    let snapshot = {
        let config = state.config.read().await;
        SetupStore::load(&config)
            .get(&setup_id)
            .map(|s| s.snapshot.clone())
            .ok_or_else(|| format!("Setup not found: {setup_id}"))?
    };
    apply(&app, &state, &snapshot).await
}

/// Put back the settings a previous recording started with.
#[tauri::command]
pub async fn apply_recent(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<Snapshot, String> {
    let json = super::session::shared_db()?
        .get_session_snapshot(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or("That recording has no stored settings")?;
    let snapshot: Snapshot = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    apply(&app, &state, &snapshot).await
}

/// Apply a snapshot directly. Used by Undo.
#[tauri::command]
pub async fn apply_snapshot(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    snapshot: Snapshot,
) -> Result<Snapshot, String> {
    apply(&app, &state, &snapshot).await
}

/// The last eight recording starts, each with a one-line summary of its settings.
#[tauri::command]
pub async fn list_recent_starts(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let live = serde_json::to_value(Snapshot::capture(&*state.config.read().await))
        .map_err(|e| e.to_string())?;
    let rows = super::session::shared_db()?
        .recent_session_snapshots(8)
        .map_err(|e| e.to_string())?;
    let items: Vec<_> = rows
        .into_iter()
        .filter_map(|(session, json)| {
            let snapshot: Snapshot = serde_json::from_str(&json).ok()?;
            let current = serde_json::to_value(&snapshot).ok()? == live;
            Some(serde_json::json!({
                "current": current,
                "session_id": session.id,
                "title": session.title,
                "started_at": session.started_at,
                "summary": snapshot.summary(),
                "missing": super::models::missing_models(&snapshot),
            }))
        })
        .collect();
    Ok(serde_json::json!(items))
}

/// One-line summary of what the next recording will use.
#[tauri::command]
pub async fn get_run_summary(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    Ok(Snapshot::capture(&*state.config.read().await).summary())
}

/// Get all keyboard shortcuts.
#[tauri::command]
pub async fn get_shortcuts() -> Result<serde_json::Value, String> {
    let store = gravai_config::shortcuts::ShortcutStore::load();
    serde_json::to_value(&store).map_err(|e| e.to_string())
}

/// Rebind a keyboard shortcut and re-register global keys. An empty key turns it off.
#[tauri::command]
pub async fn rebind_shortcut(
    app: tauri::AppHandle,
    action_id: String,
    key_sequence: String,
    is_global: bool,
) -> Result<(), String> {
    let mut store = gravai_config::shortcuts::ShortcutStore::load();
    store.rebind(&action_id, &key_sequence, is_global)?;
    store.save().map_err(|e| e.to_string())?;
    crate::shortcuts::register(&app, &store)
}

/// Get all automations.
#[tauri::command]
pub async fn get_automations() -> Result<serde_json::Value, String> {
    let store = gravai_config::automations::AutomationStore::load();
    serde_json::to_value(&store).map_err(|e| e.to_string())
}

/// Toggle an automation on/off.
#[tauri::command]
pub async fn toggle_automation(automation_id: String, enabled: bool) -> Result<(), String> {
    let mut store = gravai_config::automations::AutomationStore::load();
    if !store.set_enabled(&automation_id, enabled) {
        return Err(format!("Automation not found: {automation_id}"));
    }
    store.save().map_err(|e| e.to_string())
}

/// Save a new or updated automation.
#[tauri::command]
pub async fn save_automation(automation: serde_json::Value) -> Result<(), String> {
    let automation: gravai_config::automations::Automation =
        serde_json::from_value(automation).map_err(|e| e.to_string())?;
    let mut store = gravai_config::automations::AutomationStore::load();
    store.automations.insert(automation.id.clone(), automation);
    store.save().map_err(|e| e.to_string())
}
