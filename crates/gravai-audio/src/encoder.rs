//! Audio format export: WAV, AIFF, and stubs for M4A (AAC/ALAC).
//!
//! WAV is handled by hound (already in recorder.rs).
//! AIFF also uses hound.
//! M4A encoding will use CoreAudio AudioToolbox in a future update.

use std::path::Path;
use tracing::info;

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportFormat {
    Wav,
    Aiff,
    Caf,
    M4aAac,
    M4aAlac,
}

impl ExportFormat {
    pub fn parse(s: &str) -> Self {
        match s {
            "aiff" => Self::Aiff,
            "caf" => Self::Caf,
            "m4a-aac" => Self::M4aAac,
            "m4a-alac" => Self::M4aAlac,
            _ => Self::Wav,
        }
    }

    pub fn extension(&self) -> &str {
        match self {
            Self::Wav => "wav",
            Self::Aiff => "aiff",
            Self::Caf => "caf",
            Self::M4aAac | Self::M4aAlac => "m4a",
        }
    }
}

/// Export a WAV file to a different format.
pub fn export_audio(
    source_wav: &Path,
    output_path: &Path,
    format: ExportFormat,
    _aac_bitrate_kbps: u32,
) -> Result<(), String> {
    match format {
        ExportFormat::Wav => {
            // Just copy
            std::fs::copy(source_wav, output_path).map_err(|e| format!("Copy WAV: {e}"))?;
            info!("Exported WAV: {}", output_path.display());
            Ok(())
        }
        ExportFormat::Aiff => export_aiff(source_wav, output_path),
        ExportFormat::Caf | ExportFormat::M4aAac | ExportFormat::M4aAlac => {
            // Use macOS afconvert command-line tool as a pragmatic approach
            #[cfg(target_os = "macos")]
            {
                export_via_afconvert(source_wav, output_path, format, _aac_bitrate_kbps)
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(format!("{:?} export requires macOS", format))
            }
        }
    }
}

/// Export to AIFF via afconvert.
fn export_aiff(source_wav: &Path, output_path: &Path) -> Result<(), String> {
    // hound doesn't write AIFF directly; use afconvert on macOS
    #[cfg(target_os = "macos")]
    {
        export_via_afconvert(source_wav, output_path, ExportFormat::Aiff, 0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("AIFF export requires macOS afconvert".into())
    }
}

/// Use macOS `afconvert` CLI tool for format conversion.
#[cfg(target_os = "macos")]
fn export_via_afconvert(
    source: &Path,
    output: &Path,
    format: ExportFormat,
    aac_bitrate: u32,
) -> Result<(), String> {
    let (data_format, file_format) = match format {
        ExportFormat::Aiff => ("BEI24", "AIFF"),
        ExportFormat::Caf => ("lpcm", "caff"),
        ExportFormat::M4aAac => ("aac", "m4af"),
        ExportFormat::M4aAlac => ("alac", "m4af"),
        ExportFormat::Wav => ("LEI24", "WAVE"),
    };

    let mut cmd = std::process::Command::new("afconvert");
    cmd.arg(source)
        .arg(output)
        .arg("-d")
        .arg(data_format)
        .arg("-f")
        .arg(file_format);

    if format == ExportFormat::M4aAac && aac_bitrate > 0 {
        cmd.arg("-b").arg(format!("{}", aac_bitrate * 1000));
    }

    let result = cmd.output().map_err(|e| format!("afconvert: {e}"))?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("afconvert failed: {stderr}"));
    }

    info!("Exported {:?}: {}", format, output.display());
    Ok(())
}

/// List which export formats are available on this platform.
pub fn available_formats() -> Vec<(&'static str, &'static str)> {
    let mut formats = vec![("wav", "WAV (PCM)")];

    #[cfg(target_os = "macos")]
    {
        // Check if afconvert is available
        if std::process::Command::new("afconvert")
            .arg("--help")
            .output()
            .is_ok()
        {
            formats.extend([
                ("aiff", "AIFF"),
                ("caf", "CAF"),
                ("m4a-aac", "M4A (AAC)"),
                ("m4a-alac", "M4A (ALAC)"),
            ]);
        }
    }

    formats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_from_str() {
        assert_eq!(ExportFormat::parse("wav"), ExportFormat::Wav);
        assert_eq!(ExportFormat::parse("m4a-aac"), ExportFormat::M4aAac);
        assert_eq!(ExportFormat::parse("unknown"), ExportFormat::Wav);
    }

    #[test]
    fn available_formats_includes_wav() {
        let fmts = available_formats();
        assert!(fmts.iter().any(|(id, _)| *id == "wav"));
    }
}
