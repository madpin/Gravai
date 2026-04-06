//! Automations — trigger + condition(s) + action(s) rules.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Trigger types for automations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTrigger {
    MeetingDetected,
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
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(store) = serde_json::from_str(&content) {
                    return store;
                }
            }
        }
        let mut store = Self::default();
        for a in builtin_automations() {
            store.automations.insert(a.id.clone(), a);
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
