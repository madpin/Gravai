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
    MeetingAppDetected { app_name: String },
    /// Specific meeting app ended.
    MeetingAppEnded { app_name: String },
    SessionStarted,
    SessionEnded,
    CalendarEventStarting,
    TimeOfDay { hour: u8, minute: u8 },
    AppForegrounded { app_name: String },
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
            enabled: false,
            trigger: AutomationTrigger::MeetingAppEnded {
                app_name: "Zoom".into(),
            },
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

/// Automation store.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutomationStore {
    pub automations: HashMap<String, Automation>,
}

impl AutomationStore {
    pub fn load() -> Self {
        let path = super::data_dir().join("automations.json");
        let mut store = if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(s) = serde_json::from_str::<Self>(&content) {
                    s
                } else {
                    Self::default()
                }
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
}
