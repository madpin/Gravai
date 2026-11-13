//! Keyboard shortcuts — action → key sequence mappings.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A keyboard shortcut binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutBinding {
    pub action_id: String,
    pub key_sequence: String,
    pub description: String,
    pub is_global: bool,
}

/// All available actions that can be bound to shortcuts.
///
/// Global keys work while another app is in front, so they avoid combinations
/// other apps use (Cmd+Shift+S is Save As almost everywhere).
pub fn default_shortcuts() -> Vec<ShortcutBinding> {
    vec![
        ShortcutBinding {
            action_id: "recording.start".into(),
            key_sequence: "CmdOrCtrl+Alt+R".into(),
            description: "Start recording".into(),
            is_global: true,
        },
        ShortcutBinding {
            action_id: "recording.stop".into(),
            key_sequence: "CmdOrCtrl+Alt+S".into(),
            description: "Stop recording".into(),
            is_global: true,
        },
        ShortcutBinding {
            action_id: "recording.pause".into(),
            key_sequence: "CmdOrCtrl+Alt+P".into(),
            description: "Pause or resume recording".into(),
            is_global: true,
        },
        ShortcutBinding {
            action_id: "recording.bookmark".into(),
            key_sequence: "CmdOrCtrl+Shift+B".into(),
            description: "Add bookmark".into(),
            is_global: false,
        },
    ]
}

/// Defaults before shortcuts were registered. Nothing ever handled them, so
/// a binding still on one of these moves to the new default.
const UNREGISTERED_DEFAULTS: &[(&str, &str)] = &[
    ("recording.start", "CmdOrCtrl+Shift+R"),
    ("recording.stop", "CmdOrCtrl+Shift+S"),
    ("recording.pause", "CmdOrCtrl+Shift+P"),
    ("recording.bookmark", "CmdOrCtrl+Shift+B"),
];

const SHORTCUTS_VERSION: u32 = 1;

/// Shortcut store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutStore {
    pub bindings: HashMap<String, ShortcutBinding>,
    #[serde(default)]
    pub version: u32,
}

impl Default for ShortcutStore {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        for s in default_shortcuts() {
            bindings.insert(s.action_id.clone(), s);
        }
        Self {
            bindings,
            version: SHORTCUTS_VERSION,
        }
    }
}

impl ShortcutStore {
    pub fn load() -> Self {
        let path = super::data_dir().join("shortcuts.json");
        let Some(mut store) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<Self>(&content).ok())
        else {
            return Self::default();
        };
        if store.migrate() {
            let _ = store.save();
        }
        store
    }

    /// Drop actions that no longer exist, add new ones, and move bindings
    /// that were never registered onto the new defaults. Returns true if changed.
    fn migrate(&mut self) -> bool {
        let defaults = default_shortcuts();
        let before = self.bindings.len();
        self.bindings
            .retain(|id, _| defaults.iter().any(|d| &d.action_id == id));
        let mut changed = self.bindings.len() != before;
        for d in defaults {
            match self.bindings.get_mut(&d.action_id) {
                None => {
                    self.bindings.insert(d.action_id.clone(), d);
                    changed = true;
                }
                Some(b) if self.version < 1 => {
                    let untouched = UNREGISTERED_DEFAULTS
                        .iter()
                        .any(|(id, key)| *id == d.action_id && *key == b.key_sequence);
                    if untouched {
                        *b = d;
                    }
                    changed = true;
                }
                Some(_) => {}
            }
        }
        if self.version < SHORTCUTS_VERSION {
            self.version = SHORTCUTS_VERSION;
            changed = true;
        }
        changed
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = super::data_dir().join("shortcuts.json");
        let content = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, content)
    }

    /// Change a binding. An empty key turns the shortcut off.
    pub fn rebind(
        &mut self,
        action_id: &str,
        new_key: &str,
        is_global: bool,
    ) -> Result<(), String> {
        let conflict = self
            .bindings
            .values()
            .find(|b| !new_key.is_empty() && b.key_sequence == new_key && b.action_id != action_id);
        if let Some(c) = conflict {
            return Err(format!(
                "Key '{new_key}' already bound to '{}'",
                c.description
            ));
        }
        if let Some(binding) = self.bindings.get_mut(action_id) {
            binding.key_sequence = new_key.to_string();
            binding.is_global = is_global;
            Ok(())
        } else {
            Err(format!("Unknown action: {action_id}"))
        }
    }

    pub fn get_key(&self, action_id: &str) -> Option<&str> {
        self.bindings
            .get(action_id)
            .map(|b| b.key_sequence.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_store_drops_dead_actions_and_moves_to_new_defaults() {
        let raw = serde_json::json!({ "bindings": {
            "preset.next": { "action_id": "preset.next", "key_sequence": "CmdOrCtrl+Shift+]", "description": "", "is_global": false },
            "profile.switch": { "action_id": "profile.switch", "key_sequence": "CmdOrCtrl+Shift+/", "description": "", "is_global": true },
            "recording.start": { "action_id": "recording.start", "key_sequence": "CmdOrCtrl+Shift+R", "description": "", "is_global": true },
            "recording.stop": { "action_id": "recording.stop", "key_sequence": "F9", "description": "", "is_global": false }
        }});
        let mut store: ShortcutStore = serde_json::from_value(raw).unwrap();
        assert!(store.migrate());
        assert!(!store.bindings.contains_key("preset.next"));
        assert!(!store.bindings.contains_key("profile.switch"));
        assert_eq!(store.get_key("recording.start"), Some("CmdOrCtrl+Alt+R"));
        // A key the user chose is kept.
        assert_eq!(store.get_key("recording.stop"), Some("F9"));
        assert!(store.bindings.contains_key("recording.bookmark"));
        assert!(!store.migrate());
    }

    #[test]
    fn rebind_keeps_global_flag_and_allows_clearing() {
        let mut store = ShortcutStore::default();
        store.rebind("recording.stop", "F9", false).unwrap();
        assert!(!store.bindings["recording.stop"].is_global);
        store.rebind("recording.start", "", true).unwrap();
        store.rebind("recording.pause", "", true).unwrap();
        assert!(store.rebind("recording.bookmark", "F9", false).is_err());
    }
}
