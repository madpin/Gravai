//! Export integrations: Markdown, PDF (text), Obsidian, Notion.

pub mod markdown;
pub mod notion;
pub mod obsidian;
pub mod pdf;

use serde::{Deserialize, Serialize};

/// Data passed to export functions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportData {
    pub session_id: String,
    pub title: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<f64>,
    pub meeting_app: Option<String>,
    pub utterances: Vec<ExportUtterance>,
    pub summary: Option<ExportSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportUtterance {
    pub timestamp: String,
    pub source: String,
    pub speaker: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSummary {
    pub tldr: String,
    pub key_decisions: Vec<String>,
    pub action_items: Vec<serde_json::Value>,
    pub open_questions: Vec<String>,
}

/// Which sections to include in the export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportOptions {
    pub include_summary: bool,
    pub include_transcript: bool,
    pub include_action_items: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            include_summary: true,
            include_transcript: true,
            include_action_items: true,
        }
    }
}
