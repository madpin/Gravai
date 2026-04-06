//! Preset, profile, shortcut, and automation management commands.

/// Get all capture presets.
#[tauri::command]
pub async fn get_presets() -> Result<serde_json::Value, String> {
    let store = gravai_config::presets::PresetStore::load();
    serde_json::to_value(&store).map_err(|e| e.to_string())
}

/// Activate a capture preset by ID.
#[tauri::command]
pub async fn activate_preset(preset_id: String) -> Result<serde_json::Value, String> {
    let mut store = gravai_config::presets::PresetStore::load();
    let preset = store
        .activate(&preset_id)
        .ok_or_else(|| format!("Preset not found: {preset_id}"))?
        .clone();
    store.save().map_err(|e| e.to_string())?;
    serde_json::to_value(&preset).map_err(|e| e.to_string())
}

/// Save a new or updated preset.
#[tauri::command]
pub async fn save_preset(preset: serde_json::Value) -> Result<(), String> {
    let preset: gravai_config::presets::CapturePreset =
        serde_json::from_value(preset).map_err(|e| e.to_string())?;
    let mut store = gravai_config::presets::PresetStore::load();
    store.presets.insert(preset.id.clone(), preset);
    store.save().map_err(|e| e.to_string())
}

/// Delete a preset by ID.
#[tauri::command]
pub async fn delete_preset(preset_id: String) -> Result<(), String> {
    let mut store = gravai_config::presets::PresetStore::load();
    store.presets.remove(&preset_id);
    store.save().map_err(|e| e.to_string())
}

/// Get all profiles.
#[tauri::command]
pub async fn get_profiles() -> Result<serde_json::Value, String> {
    let store = gravai_config::profiles::ProfileStore::load();
    serde_json::to_value(&store).map_err(|e| e.to_string())
}

/// Activate a profile by ID.
#[tauri::command]
pub async fn activate_profile(profile_id: String) -> Result<serde_json::Value, String> {
    let mut store = gravai_config::profiles::ProfileStore::load();
    let profile = store
        .activate(&profile_id)
        .ok_or_else(|| format!("Profile not found: {profile_id}"))?
        .clone();
    store.save().map_err(|e| e.to_string())?;
    serde_json::to_value(&profile).map_err(|e| e.to_string())
}

/// Save a new or updated profile.
#[tauri::command]
pub async fn save_profile(profile: serde_json::Value) -> Result<(), String> {
    let profile: gravai_config::profiles::Profile =
        serde_json::from_value(profile).map_err(|e| e.to_string())?;
    let mut store = gravai_config::profiles::ProfileStore::load();
    store.profiles.insert(profile.id.clone(), profile);
    store.save().map_err(|e| e.to_string())
}

/// Get all keyboard shortcuts.
#[tauri::command]
pub async fn get_shortcuts() -> Result<serde_json::Value, String> {
    let store = gravai_config::shortcuts::ShortcutStore::load();
    serde_json::to_value(&store).map_err(|e| e.to_string())
}

/// Rebind a keyboard shortcut.
#[tauri::command]
pub async fn rebind_shortcut(action_id: String, key_sequence: String) -> Result<(), String> {
    let mut store = gravai_config::shortcuts::ShortcutStore::load();
    store.rebind(&action_id, &key_sequence)?;
    store.save().map_err(|e| e.to_string())
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
