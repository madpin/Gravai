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
    /// Any detected meeting app ended.
    MeetingEnded,
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
    /// One line shown on the Automations page.
    #[serde(default)]
    pub description: String,
    pub enabled: bool,
    pub trigger: AutomationTrigger,
    pub conditions: Vec<AutomationCondition>,
    pub actions: Vec<AutomationAction>,
    pub last_run: Option<String>,
    pub run_count: u32,
}

impl Automation {
    /// Stored conditions that this build actually checks.
    pub fn conditions_hold(&self, recording: bool) -> bool {
        self.conditions.iter().all(|condition| match condition {
            AutomationCondition::NoActiveSession => !recording,
            AutomationCondition::ActiveSession => recording,
            AutomationCondition::AppRunning { .. } | AutomationCondition::DayOfWeek { .. } => true,
        })
    }

    fn starts_recording(&self) -> bool {
        self.actions
            .iter()
            .any(|action| matches!(action, AutomationAction::StartRecording))
    }
}

/// Built-in automation templates.
///
/// Meeting start and meeting end are one pair. Silence is a separate safety
/// net. The old Zoom-only start rule duplicated the generic start and is gone.
pub fn builtin_automations() -> Vec<Automation> {
    vec![
        Automation {
            id: "auto-meeting-record".into(),
            name: "Start recording when a meeting starts".into(),
            description: "Zoom, Teams, Slack, FaceTime, Discord, or Webex joins a call. Switches to the Meeting profile and starts recording. Skipped if a recording is already going.".into(),
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
            id: "zoom-auto-stop".into(),
            name: "Stop recording when the meeting ends".into(),
            description: "When that call ends, waits 10 seconds, then stops. Keep recording cancels it. Applies to every detected meeting app.".into(),
            enabled: true,
            trigger: AutomationTrigger::MeetingEnded,
            conditions: vec![AutomationCondition::ActiveSession],
            actions: vec![AutomationAction::StopRecording],
            last_run: None,
            run_count: 0,
        },
        Automation {
            id: "silence-auto-stop".into(),
            name: "Stop recording after prolonged silence".into(),
            description: "Mic and system audio both quiet for 30 seconds, then a 60 second countdown. Any sound cancels it. This is separate from the meeting-end stop.".into(),
            enabled: true,
            trigger: AutomationTrigger::AudioSilent,
            conditions: vec![AutomationCondition::ActiveSession],
            actions: vec![AutomationAction::StopRecording],
            last_run: None,
            run_count: 0,
        },
        Automation {
            id: "auto-export-on-stop".into(),
            name: "Save a markdown transcript when recording stops".into(),
            description: "Writes the transcript into your export folder when a recording ends.".into(),
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
const CURRENT_DEFAULTS_VERSION: u32 = 2;

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

    /// Start rules for this app. A Zoom-specific start suppresses the generic
    /// "any meeting" start so both don't launch a recording.
    pub fn meeting_start_automations(&self, app_name: &str) -> Vec<&Automation> {
        let mut matched = self.find_for_meeting_detected(app_name);
        let has_specific_start = matched.iter().any(|automation| {
            matches!(
                automation.trigger,
                AutomationTrigger::MeetingAppDetected { .. }
            ) && automation.starts_recording()
        });
        if has_specific_start {
            matched.retain(|automation| {
                !matches!(automation.trigger, AutomationTrigger::MeetingDetected)
            });
        }
        matched
    }

    /// Find automations that should fire when a meeting app ends.
    /// `MeetingEnded` matches every app. `MeetingAppEnded` matches one name.
    pub fn find_for_meeting_ended(&self, app_name: &str) -> Vec<&Automation> {
        self.automations
            .values()
            .filter(|a| {
                a.enabled
                    && match &a.trigger {
                        AutomationTrigger::MeetingEnded => true,
                        AutomationTrigger::MeetingAppEnded { app_name: name } => name == app_name,
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
        let mut changed = false;
        if self.defaults_version < 1 {
            for id in ["zoom-auto-stop", "silence-auto-stop"] {
                self.set_enabled(id, true);
            }
            changed = true;
        }
        if self.defaults_version < 2 {
            self.collapse_duplicate_meeting_rules();
            changed = true;
        }
        if self.defaults_version < CURRENT_DEFAULTS_VERSION {
            self.defaults_version = CURRENT_DEFAULTS_VERSION;
            changed = true;
        }
        changed
    }

    /// The Zoom-only start rule did the same work as "any meeting". Drop it.
    /// The Zoom-only stop becomes "any meeting ended", keeping its on/off state.
    fn collapse_duplicate_meeting_rules(&mut self) {
        let zoom_start_on = self
            .automations
            .get("zoom-auto-start")
            .is_some_and(|automation| automation.enabled);
        if zoom_start_on {
            self.set_enabled("auto-meeting-record", true);
        }
        self.automations.remove("zoom-auto-start");

        for builtin in builtin_automations() {
            if let Some(existing) = self.automations.get_mut(&builtin.id) {
                existing.name = builtin.name.clone();
                existing.description = builtin.description.clone();
                if builtin.id == "zoom-auto-stop" {
                    existing.trigger = builtin.trigger.clone();
                }
            }
        }
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

    #[test]
    fn duplicate_zoom_start_is_folded_into_any_meeting() {
        let mut store = AutomationStore::default();
        for automation in builtin_automations() {
            store
                .automations
                .entry(automation.id.clone())
                .or_insert(automation);
        }
        store.automations.insert(
            "zoom-auto-start".into(),
            Automation {
                id: "zoom-auto-start".into(),
                name: "Start recording when Zoom meeting starts".into(),
                description: String::new(),
                enabled: true,
                trigger: AutomationTrigger::MeetingAppDetected {
                    app_name: "Zoom".into(),
                },
                conditions: vec![AutomationCondition::NoActiveSession],
                actions: vec![AutomationAction::StartRecording],
                last_run: None,
                run_count: 0,
            },
        );
        store.set_enabled("auto-meeting-record", false);
        store.defaults_version = 1;

        assert!(store.apply_default_migrations());
        assert!(store.automations["auto-meeting-record"].enabled);
        assert!(!store.automations.contains_key("zoom-auto-start"));
        assert_eq!(
            store.automations["zoom-auto-stop"].trigger,
            AutomationTrigger::MeetingEnded
        );
        assert!(!store.apply_default_migrations());
    }

    #[test]
    fn specific_start_suppresses_generic_start() {
        let mut store = AutomationStore {
            defaults_version: CURRENT_DEFAULTS_VERSION,
            ..AutomationStore::default()
        };
        for automation in builtin_automations() {
            store
                .automations
                .entry(automation.id.clone())
                .or_insert(automation);
        }
        store.set_enabled("auto-meeting-record", true);
        store.automations.insert(
            "teams-only".into(),
            Automation {
                id: "teams-only".into(),
                name: "Teams".into(),
                description: String::new(),
                enabled: true,
                trigger: AutomationTrigger::MeetingAppDetected {
                    app_name: "Microsoft Teams".into(),
                },
                conditions: vec![AutomationCondition::NoActiveSession],
                actions: vec![AutomationAction::StartRecording],
                last_run: None,
                run_count: 0,
            },
        );

        let zoom: Vec<_> = store
            .meeting_start_automations("Zoom")
            .into_iter()
            .map(|automation| automation.id.clone())
            .collect();
        assert_eq!(zoom, vec!["auto-meeting-record".to_string()]);

        let teams: Vec<_> = store
            .meeting_start_automations("Microsoft Teams")
            .into_iter()
            .map(|automation| automation.id.clone())
            .collect();
        assert_eq!(teams, vec!["teams-only".to_string()]);
    }
}
