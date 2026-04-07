//! Obsidian export — Markdown with YAML frontmatter pushed to a vault folder.

use crate::{markdown, ExportData, ExportOptions};
use std::path::Path;
use tracing::info;

/// Export to an Obsidian vault folder.
pub fn export_obsidian(
    data: &ExportData,
    options: &ExportOptions,
    vault_folder: &Path,
) -> Result<String, String> {
    std::fs::create_dir_all(vault_folder).map_err(|e| format!("Create vault folder: {e}"))?;

    let filename = sanitize_filename(data.title.as_deref().unwrap_or(&data.session_id));
    let filepath = vault_folder.join(format!("{filename}.md"));

    let md = markdown::export_markdown(data, options);
    std::fs::write(&filepath, md).map_err(|e| format!("Write: {e}"))?;

    info!("Obsidian export: {}", filepath.display());
    Ok(filepath.display().to_string())
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}
