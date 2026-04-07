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

/// Merge all WAV files in a session directory into a single mixed WAV,
/// then export to the requested format.
/// This handles sources with different sample rates by resampling to the highest rate.
pub fn merge_and_export(
    session_dir: &Path,
    output_path: &Path,
    format: ExportFormat,
    aac_bitrate_kbps: u32,
) -> Result<(), String> {
    // Find all WAV files in the session directory
    let wav_files: Vec<std::path::PathBuf> = std::fs::read_dir(session_dir)
        .map_err(|e| format!("Read dir: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
                && !p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .contains("trimmed")
        })
        .collect();

    if wav_files.is_empty() {
        return Err("No WAV files found in session".into());
    }

    // If only one file, just export it directly
    if wav_files.len() == 1 {
        return export_audio(&wav_files[0], output_path, format, aac_bitrate_kbps);
    }

    info!(
        "Merging {} tracks from {}",
        wav_files.len(),
        session_dir.display()
    );

    // Read all tracks as f32 samples + find the highest sample rate and channel count
    let mut tracks: Vec<(Vec<f32>, u32, u16)> = Vec::new();
    let mut max_rate: u32 = 0;
    let mut max_channels: u16 = 0;

    for path in &wav_files {
        let reader =
            hound::WavReader::open(path).map_err(|e| format!("Open {}: {e}", path.display()))?;
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .filter_map(|s| s.ok())
                .collect(),
            hound::SampleFormat::Int => {
                let max_val = (1i64 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .into_samples::<i32>()
                    .filter_map(|s| s.ok())
                    .map(|s| s as f32 / max_val)
                    .collect()
            }
        };
        max_rate = max_rate.max(spec.sample_rate);
        max_channels = max_channels.max(spec.channels);
        tracks.push((samples, spec.sample_rate, spec.channels));
    }

    // Determine output length (longest track at max_rate)
    let output_len = tracks
        .iter()
        .map(|(samples, rate, channels)| {
            let duration_secs = samples.len() as f64 / (*rate as f64 * *channels as f64);
            (duration_secs * max_rate as f64 * max_channels as f64) as usize
        })
        .max()
        .unwrap_or(0);

    // Mix all tracks into the output buffer
    let mut mixed = vec![0.0f32; output_len];

    for (samples, rate, channels) in &tracks {
        let rate_ratio = *rate as f64 / max_rate as f64;
        let ch = *channels as usize;
        let out_ch = max_channels as usize;

        for (out_i, out_sample) in mixed.iter_mut().enumerate() {
            let out_frame = out_i / out_ch;
            let out_channel = out_i % out_ch;

            // Map output frame to source frame via rate ratio
            let src_frame = (out_frame as f64 * rate_ratio) as usize;
            let src_channel = if ch == 1 { 0 } else { out_channel.min(ch - 1) };
            let src_i = src_frame * ch + src_channel;

            if src_i < samples.len() {
                *out_sample += samples[src_i];
            }
        }
    }

    // Soft clip to prevent distortion
    let peak = mixed.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    if peak > 1.0 {
        let scale = 1.0 / peak;
        for s in &mut mixed {
            *s *= scale;
        }
        info!("Normalized mix (peak was {:.2})", peak);
    }

    // Write mixed WAV to a temp file
    let mixed_wav = session_dir.join("_mixed_temp.wav");
    {
        let spec = hound::WavSpec {
            channels: max_channels,
            sample_rate: max_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer =
            hound::WavWriter::create(&mixed_wav, spec).map_err(|e| format!("Create mix: {e}"))?;
        for &s in &mixed {
            writer
                .write_sample(s)
                .map_err(|e| format!("Write mix: {e}"))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("Finalize mix: {e}"))?;
    }

    info!(
        "Mixed {} tracks → {} ({}Hz {}ch, {} samples)",
        tracks.len(),
        mixed_wav.display(),
        max_rate,
        max_channels,
        mixed.len()
    );

    // Export the mixed WAV to the target format
    let result = export_audio(&mixed_wav, output_path, format, aac_bitrate_kbps);

    // Clean up temp file
    let _ = std::fs::remove_file(&mixed_wav);

    result
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
