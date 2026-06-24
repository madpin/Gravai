//! Automations — trigger + condition(s) + action(s) rules.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Trigger types for automations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTrigger {
    /// Any meeting app detected (generic).
    MeetingDetected,
    /// Specific meeting app detected (e.g. "Zoom", "Microsoft Teams").
    MeetingAppDetected {
        app_name: String,
    },
    /// Specific meeting app ended.
    MeetingAppEnded {
        app_name: String,
    },
    /// Microphone and system audio have both been silent for a while.
    /// Evaluated by the silence monitor in `lib.rs`, not the standard engine.
    AudioSilent,
    SessionStarted,
    SessionEnded,
    CalendarEventStarting,
    TimeOfDay {
        hour: u8,
        minute: u8,
    },
    AppForegrounded {
        app_name: String,
    },
}

/// Condition to evaluate before executing actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCondition {
    AppRunning { app_name: String },
    DayOfWeek { days: Vec<String> },
    NoActiveSession,
    ActiveSession,
}

/// Action to execute when an automation fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationAction {
    ActivateProfile { profile_id: String },
    ActivatePreset { preset_id: String },
    StartRecording,
    StopRecording,
    ShowNotification { message: String },
    RunExport { format: String },
}

/// A complete automation rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Automation {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger: AutomationTrigger,
    pub conditions: Vec<AutomationCondition>,
    pub actions: Vec<AutomationAction>,
    pub last_run: Option<String>,
    pub run_count: u32,
}

/// Built-in automation templates.
pub fn builtin_automations() -> Vec<Automation> {
    vec![
        Automation {
            id: "auto-meeting-record".into(),
            name: "Record on meeting detection".into(),
            enabled: false,
            trigger: AutomationTrigger::MeetingDetected,
            conditions: vec![AutomationCondition::NoActiveSession],
            actions: vec![
                AutomationAction::ActivateProfile {
                    profile_id: "meeting".into(),
                },
                AutomationAction::StartRecording,
            ],
            last_run: None,
            run_count: 0,
        },
        Automation {
            id: "zoom-auto-start".into(),
            name: "Start recording when Zoom meeting starts".into(),
            enabled: false,
            trigger: AutomationTrigger::MeetingAppDetected {
                app_name: "Zoom".into(),
            },
            conditions: vec![AutomationCondition::NoActiveSession],
            actions: vec![
                AutomationAction::ActivateProfile {
                    profile_id: "meeting".into(),
                },
                AutomationAction::StartRecording,
            ],
            last_run: None,
            run_count: 0,
        },
        Automation {
            id: "zoom-auto-stop".into(),
            name: "Stop recording when Zoom meeting ends".into(),
            enabled: true,
            trigger: AutomationTrigger::MeetingAppEnded {
                app_name: "Zoom".into(),
            },
            conditions: vec![AutomationCondition::ActiveSession],
            actions: vec![AutomationAction::StopRecording],
            last_run: None,
            run_count: 0,
        },
        Automation {
            id: "silence-auto-stop".into(),
            name: "Stop recording when mic and system audio go silent".into(),
            enabled: true,
            trigger: AutomationTrigger::AudioSilent,
            conditions: vec![AutomationCondition::ActiveSession],
            actions: vec![AutomationAction::StopRecording],
            last_run: None,
            run_count: 0,
        },
        Automation {
            id: "auto-export-on-stop".into(),
            name: "Export transcript on session end".into(),
            enabled: false,
            trigger: AutomationTrigger::SessionEnded,
            conditions: vec![],
            actions: vec![AutomationAction::RunExport {
                format: "markdown".into(),
            }],
            last_run: None,
            run_count: 0,
        },
    ]
}

/// Bumped when a builtin's default `enabled` state changes, so existing
/// stores get the new defaults applied exactly once (see `load`).
const CURRENT_DEFAULTS_VERSION: u32 = 1;

/// Automation store.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutomationStore {
    pub automations: HashMap<String, Automation>,
    /// Tracks which round of default-enabled migrations has been applied.
    #[serde(default)]
    pub defaults_version: u32,
}

impl AutomationStore {
    pub fn load() -> Self {
        let path = super::data_dir().join("automations.json");
        let mut store = if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                serde_json::from_str::<Self>(&content).unwrap_or_default()
            } else {
                Self::default()
            }
        } else {
            Self::default()
        };
        // Merge any builtin automations that don't yet exist in the store
        // (handles the case where new builtins are added after first run).
        for a in builtin_automations() {
            store.automations.entry(a.id.clone()).or_insert(a);
        }
        // One-time migration: `or_insert` never overwrites an existing entry, so
        // builtins that shipped disabled and later became default-on won't flip
        // for current users. Force-enable them exactly once (guarded by
        // `defaults_version`) so a user who deliberately disables them afterwards
        // is respected.
        if store.apply_default_migrations() {
            let _ = store.save();
        }
        store
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = super::data_dir().join("automations.json");
        let content = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, content)
    }

    /// Find automations matching a trigger (only enabled ones).
    pub fn find_by_trigger(&self, trigger: &AutomationTrigger) -> Vec<&Automation> {
        self.automations
            .values()
            .filter(|a| a.enabled && a.trigger == *trigger)
            .collect()
    }

    /// Find automations that should fire when a meeting app is detected.
    /// Matches both the generic `MeetingDetected` trigger and
    /// `MeetingAppDetected { app_name }` for the specific app.
    pub fn find_for_meeting_detected(&self, app_name: &str) -> Vec<&Automation> {
        self.automations
            .values()
            .filter(|a| {
                a.enabled
                    && matches!(
                        &a.trigger,
                        AutomationTrigger::MeetingDetected
                            | AutomationTrigger::MeetingAppDetected { .. }
                    )
                    && match &a.trigger {
                        AutomationTrigger::MeetingDetected => true,
                        AutomationTrigger::MeetingAppDetected { app_name: n } => n == app_name,
                        _ => false,
                    }
            })
            .collect()
    }

    /// Find automations that should fire when a meeting app ends.
    pub fn find_for_meeting_ended(&self, app_name: &str) -> Vec<&Automation> {
        self.automations
            .values()
            .filter(|a| {
                a.enabled
                    && match &a.trigger {
                        AutomationTrigger::MeetingAppEnded { app_name: n } => n == app_name,
                        _ => false,
                    }
            })
            .collect()
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if let Some(a) = self.automations.get_mut(id) {
            a.enabled = enabled;
            true
        } else {
            false
        }
    }

    pub fn record_run(&mut self, id: &str) {
        if let Some(a) = self.automations.get_mut(id) {
            a.last_run = Some(chrono::Utc::now().to_rfc3339());
            a.run_count += 1;
        }
    }

    /// Apply the one-time default-enabled migration to an in-memory store.
    /// Returns true if anything changed (caller decides whether to persist).
    /// Extracted from `load` so it can be unit-tested without disk I/O.
    fn apply_default_migrations(&mut self) -> bool {
        if self.defaults_version >= CURRENT_DEFAULTS_VERSION {
            return false;
        }
        for id in ["zoom-auto-stop", "silence-auto-stop"] {
            self.set_enabled(id, true);
        }
        self.defaults_version = CURRENT_DEFAULTS_VERSION;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_stop_builtins_default_on() {
        let builtins = builtin_automations();
        for id in ["zoom-auto-stop", "silence-auto-stop"] {
            let a = builtins
                .iter()
                .find(|a| a.id == id)
                .expect("builtin exists");
            assert!(a.enabled, "{id} should ship default-on");
        }
    }

    #[test]
    fn silence_builtin_uses_audio_silent_trigger() {
        let a = builtin_automations()
            .into_iter()
            .find(|a| a.id == "silence-auto-stop")
            .unwrap();
        assert_eq!(a.trigger, AutomationTrigger::AudioSilent);
        assert!(matches!(
            a.actions.as_slice(),
            [AutomationAction::StopRecording]
        ));
    }

    #[test]
    fn migration_force_enables_legacy_disabled_auto_stop_once() {
        // Simulate an existing store where zoom-auto-stop was left disabled and
        // the new field is absent (defaults_version = 0).
        let mut store = AutomationStore::default();
        for a in builtin_automations() {
            store.automations.entry(a.id.clone()).or_insert(a);
        }
        store.set_enabled("zoom-auto-stop", false);
        store.defaults_version = 0;

        assert!(store.apply_default_migrations());
        assert!(store.automations["zoom-auto-stop"].enabled);
        assert!(store.automations["silence-auto-stop"].enabled);
        assert_eq!(store.defaults_version, CURRENT_DEFAULTS_VERSION);

        // Migration is one-time: a later user-disable is respected.
        store.set_enabled("zoom-auto-stop", false);
        assert!(!store.apply_default_migrations());
        assert!(!store.automations["zoom-auto-stop"].enabled);
    }
}
