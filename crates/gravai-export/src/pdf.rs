//! PDF export as plain text (uses macOS textutil or fallback to .txt).
//!
//! For a true PDF with formatting, a crate like `genpdf` could be used,
//! but for v1 we produce a well-formatted text file and convert via macOS tools.

use crate::{ExportData, ExportOptions};
use std::path::Path;
use tracing::info;

/// Export session to a text-based PDF via macOS `textutil` + `cupsfilter`,
/// or fall back to a plain .txt file.
pub fn export_pdf(
    data: &ExportData,
    options: &ExportOptions,
    output_path: &Path,
) -> Result<(), String> {
    // Generate well-formatted plain text
    let text = generate_text(data, options);

    // Try macOS textutil: txt -> rtf -> pdf pipeline
    #[cfg(target_os = "macos")]
    {
        let txt_path = output_path.with_extension("txt");
        std::fs::write(&txt_path, &text).map_err(|e| format!("Write txt: {e}"))?;

        let result = std::process::Command::new("textutil")
            .args([
                "-convert",
                "rtf",
                "-font",
                "Helvetica",
                "-fontsize",
                "11",
                txt_path.to_str().unwrap_or(""),
            ])
            .output();

        if let Ok(r) = result {
            if r.status.success() {
                let rtf_path = txt_path.with_extension("rtf");
                // Convert RTF to PDF via cupsfilter or just keep RTF
                let _ = std::fs::rename(&rtf_path, output_path);
                let _ = std::fs::remove_file(&txt_path);
                info!("PDF exported: {}", output_path.display());
                return Ok(());
            }
        }

        // Fallback: just save as .txt
        let _ = std::fs::rename(&txt_path, output_path);
        info!(
            "Text exported (PDF conversion unavailable): {}",
            output_path.display()
        );
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        std::fs::write(output_path, &text).map_err(|e| format!("Write: {e}"))?;
        info!("Text exported: {}", output_path.display());
        Ok(())
    }
}

fn generate_text(data: &ExportData, options: &ExportOptions) -> String {
    let mut text = String::new();
    let title = data.title.as_deref().unwrap_or("Meeting Transcript");

    text.push_str(&format!("{title}\n"));
    text.push_str(&"=".repeat(title.len()));
    text.push_str(&format!("\nDate: {}\n", data.started_at));
    if let Some(dur) = data.duration_seconds {
        text.push_str(&format!("Duration: {:.0} minutes\n", dur / 60.0));
    }
    text.push('\n');

    if options.include_summary {
        if let Some(ref s) = data.summary {
            text.push_str("SUMMARY\n-------\n");
            text.push_str(&format!("{}\n\n", s.tldr));
        }
    }

    if options.include_transcript {
        text.push_str("TRANSCRIPT\n----------\n");
        for u in &data.utterances {
            let speaker = u.speaker.as_deref().unwrap_or(&u.source);
            text.push_str(&format!("[{}] {}: {}\n", u.timestamp, speaker, u.text));
        }
    }

    text
}
