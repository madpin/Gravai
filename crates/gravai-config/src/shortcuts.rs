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
pub fn default_shortcuts() -> Vec<ShortcutBinding> {
    vec![
        ShortcutBinding {
            action_id: "recording.start".into(),
            key_sequence: "CmdOrCtrl+Shift+R".into(),
            description: "Start recording".into(),
            is_global: true,
        },
        ShortcutBinding {
            action_id: "recording.stop".into(),
            key_sequence: "CmdOrCtrl+Shift+S".into(),
            description: "Stop recording".into(),
            is_global: true,
        },
        ShortcutBinding {
            action_id: "recording.pause".into(),
            key_sequence: "CmdOrCtrl+Shift+P".into(),
            description: "Pause/Resume recording".into(),
            is_global: true,
        },
        ShortcutBinding {
            action_id: "preset.next".into(),
            key_sequence: "CmdOrCtrl+Shift+]".into(),
            description: "Next capture preset".into(),
            is_global: false,
        },
        ShortcutBinding {
            action_id: "preset.prev".into(),
            key_sequence: "CmdOrCtrl+Shift+[".into(),
            description: "Previous capture preset".into(),
            is_global: false,
        },
        ShortcutBinding {
            action_id: "profile.switch".into(),
            key_sequence: "CmdOrCtrl+Shift+/".into(),
            description: "Switch profile".into(),
            is_global: true,
        },
        ShortcutBinding {
            action_id: "recording.bookmark".into(),
            key_sequence: "CmdOrCtrl+Shift+B".into(),
            description: "Add bookmark".into(),
            is_global: true,
        },
    ]
}

/// Shortcut store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutStore {
    pub bindings: HashMap<String, ShortcutBinding>,
}

impl Default for ShortcutStore {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        for s in default_shortcuts() {
            bindings.insert(s.action_id.clone(), s);
        }
        Self { bindings }
    }
}

impl ShortcutStore {
    pub fn load() -> Self {
        let path = super::data_dir().join("shortcuts.json");
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(store) = serde_json::from_str(&content) {
                    return store;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = super::data_dir().join("shortcuts.json");
        let content = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, content)
    }

    pub fn rebind(&mut self, action_id: &str, new_key: &str) -> Result<(), String> {
        // Check for conflicts
        let conflict = self
            .bindings
            .values()
            .find(|b| b.key_sequence == new_key && b.action_id != action_id);
        if let Some(c) = conflict {
            return Err(format!(
                "Key '{new_key}' already bound to '{}'",
                c.description
            ));
        }
        if let Some(binding) = self.bindings.get_mut(action_id) {
            binding.key_sequence = new_key.to_string();
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
