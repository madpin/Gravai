//! Notion export — push summary + transcript to a Notion database via API v1.

use crate::ExportData;
use tracing::{info, warn};

/// Push session data to a Notion database.
pub async fn export_notion(
    data: &ExportData,
    api_key: &str,
    database_id: &str,
) -> Result<String, String> {
    if api_key.is_empty() || database_id.is_empty() {
        return Err("Notion API key and database ID are required".into());
    }

    let title = data.title.as_deref().unwrap_or(&data.session_id);

    // Build the Notion page creation payload
    let mut children = Vec::new();

    // Summary block
    if let Some(ref s) = data.summary {
        children.push(serde_json::json!({
            "object": "block",
            "type": "heading_2",
            "heading_2": { "rich_text": [{"type": "text", "text": {"content": "Summary"}}] }
        }));
        children.push(serde_json::json!({
            "object": "block",
            "type": "paragraph",
            "paragraph": { "rich_text": [{"type": "text", "text": {"content": &s.tldr}}] }
        }));

        if !s.action_items.is_empty() {
            children.push(serde_json::json!({
                "object": "block",
                "type": "heading_3",
                "heading_3": { "rich_text": [{"type": "text", "text": {"content": "Action Items"}}] }
            }));
            for a in &s.action_items {
                let desc = a["description"].as_str().unwrap_or("");
                children.push(serde_json::json!({
                    "object": "block",
                    "type": "to_do",
                    "to_do": {
                        "rich_text": [{"type": "text", "text": {"content": desc}}],
                        "checked": false
                    }
                }));
            }
        }
    }

    // Transcript excerpt (first 50 utterances to stay within Notion limits)
    children.push(serde_json::json!({
        "object": "block",
        "type": "heading_2",
        "heading_2": { "rich_text": [{"type": "text", "text": {"content": "Transcript"}}] }
    }));
    for u in data.utterances.iter().take(50) {
        let speaker = u.speaker.as_deref().unwrap_or(&u.source);
        let line = format!("[{}] {}: {}", u.timestamp, speaker, u.text);
        children.push(serde_json::json!({
            "object": "block",
            "type": "paragraph",
            "paragraph": { "rich_text": [{"type": "text", "text": {"content": line}}] }
        }));
    }

    let payload = serde_json::json!({
        "parent": { "database_id": database_id },
        "properties": {
            "Name": { "title": [{"text": {"content": title}}] },
            "Date": { "date": { "start": &data.started_at } },
        },
        "children": children,
    });

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.notion.com/v1/pages")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Notion-Version", "2022-06-28")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Notion API: {e}"))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        warn!("Notion API error: {}", &body[..body.len().min(300)]);
        return Err(format!(
            "Notion API error: {}",
            &body[..body.len().min(200)]
        ));
    }

    let result: serde_json::Value = response.json().await.map_err(|e| format!("Parse: {e}"))?;
    let page_url = result["url"].as_str().unwrap_or("").to_string();

    info!("Notion export: {page_url}");
    Ok(page_url)
}
