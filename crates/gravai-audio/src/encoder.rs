//! Audio format export: WAV, AIFF, and stubs for M4A (AAC/ALAC).
//!
//! WAV is handled by hound (already in recorder.rs).
//! AIFF also uses hound.
//! M4A encoding will use CoreAudio AudioToolbox in a future update.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
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

/// Convert a 32-bit float WAV to 16-bit PCM WAV for browser compatibility.
/// If the source is already 16-bit PCM, copies it as-is.
/// Returns Ok(true) if conversion happened, Ok(false) if already compatible.
pub fn ensure_pcm16_wav(source: &Path, output: &Path) -> Result<(), String> {
    let reader =
        hound::WavReader::open(source).map_err(|e| format!("Open {}: {e}", source.display()))?;
    let spec = reader.spec();

    // Already 16-bit int? Just copy.
    if spec.sample_format == hound::SampleFormat::Int && spec.bits_per_sample == 16 {
        if source != output {
            std::fs::copy(source, output).map_err(|e| format!("Copy: {e}"))?;
        }
        return Ok(());
    }

    let out_spec = hound::WavSpec {
        channels: spec.channels,
        sample_rate: spec.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer =
        hound::WavWriter::create(output, out_spec).map_err(|e| format!("Create: {e}"))?;

    match spec.sample_format {
        hound::SampleFormat::Float => {
            for sample in reader.into_samples::<f32>() {
                let s = sample.map_err(|e| format!("Read sample: {e}"))?;
                let clamped = s.clamp(-1.0, 1.0);
                let pcm = (clamped * 32767.0) as i16;
                writer
                    .write_sample(pcm)
                    .map_err(|e| format!("Write: {e}"))?;
            }
        }
        hound::SampleFormat::Int => {
            let max_val = (1i64 << (spec.bits_per_sample - 1)) as f64;
            for sample in reader.into_samples::<i32>() {
                let s = sample.map_err(|e| format!("Read sample: {e}"))?;
                let normalized = s as f64 / max_val;
                let pcm = (normalized * 32767.0).clamp(-32768.0, 32767.0) as i16;
                writer
                    .write_sample(pcm)
                    .map_err(|e| format!("Write: {e}"))?;
            }
        }
    }

    writer.finalize().map_err(|e| format!("Finalize: {e}"))?;
    info!(
        "Converted {}→{} ({}Hz {}ch, {}bit {} → 16bit PCM)",
        source.display(),
        output.display(),
        spec.sample_rate,
        spec.channels,
        spec.bits_per_sample,
        if spec.sample_format == hound::SampleFormat::Float {
            "float"
        } else {
            "int"
        }
    );
    Ok(())
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

type WavFileReader = BufReader<std::fs::File>;

enum SampleStream {
    Float(hound::WavIntoSamples<WavFileReader, f32>),
    Int(hound::WavIntoSamples<WavFileReader, i32>, f32),
}

impl SampleStream {
    fn open(path: &Path) -> Result<(Self, hound::WavSpec, u32), String> {
        let reader = hound::WavReader::open(path)
            .map_err(|e| format!("Open WAV {}: {e}", path.display()))?;
        let spec = reader.spec();
        let duration = reader.duration();
        let stream = match spec.sample_format {
            hound::SampleFormat::Float => Self::Float(reader.into_samples::<f32>()),
            hound::SampleFormat::Int => {
                let scale = (1_i64 << (spec.bits_per_sample - 1)) as f32;
                Self::Int(reader.into_samples::<i32>(), scale)
            }
        };
        Ok((stream, spec, duration))
    }

    fn next(&mut self) -> Result<Option<f32>, String> {
        match self {
            Self::Float(samples) => samples
                .next()
                .transpose()
                .map_err(|e| format!("Read WAV sample: {e}")),
            Self::Int(samples, scale) => samples
                .next()
                .transpose()
                .map(|sample| sample.map(|value| value as f32 / *scale))
                .map_err(|e| format!("Read WAV sample: {e}")),
        }
    }
}

#[derive(Clone)]
struct TrackGroup {
    paths: Vec<PathBuf>,
    sample_rate: u32,
    channels: u16,
    frames: u64,
}

fn source_track_groups(session_dir: &Path) -> Result<Vec<TrackGroup>, String> {
    let mut grouped: BTreeMap<String, Vec<(u32, PathBuf)>> = BTreeMap::new();
    let mut master = Vec::new();

    for entry in std::fs::read_dir(session_dir).map_err(|e| format!("Read dir: {e}"))? {
        let path = entry.map_err(|e| format!("Read dir entry: {e}"))?.path();
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
        {
            continue;
        }

        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let lower = stem.to_lowercase();
        if lower.contains("trimmed")
            || lower.starts_with("_mixed")
            || lower.starts_with("_playback")
            || lower.starts_with("export")
            || lower.starts_with("playback")
        {
            continue;
        }

        let (track, part) = match stem.rsplit_once(".part") {
            Some((track, suffix))
                if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) =>
            {
                (track.to_string(), suffix.parse().unwrap_or(u32::MAX))
            }
            _ => (stem, 1),
        };
        if track.eq_ignore_ascii_case("master") {
            master.push((part, path));
        } else {
            grouped.entry(track).or_default().push((part, path));
        }
    }

    if grouped.is_empty() && !master.is_empty() {
        grouped.insert("master".into(), master);
    }

    let mut groups = Vec::with_capacity(grouped.len());
    for (_, mut parts) in grouped {
        parts.sort_by_key(|(part, _)| *part);
        let mut sample_rate = 0;
        let mut channels = 0;
        let mut frames = 0u64;
        let mut paths = Vec::with_capacity(parts.len());
        for (_, path) in parts {
            let (_, spec, duration) = SampleStream::open(&path)?;
            if sample_rate == 0 {
                sample_rate = spec.sample_rate;
                channels = spec.channels;
            } else if sample_rate != spec.sample_rate || channels != spec.channels {
                return Err(format!(
                    "WAV parts for one track have different formats: {}",
                    path.display()
                ));
            }
            frames += u64::from(duration);
            paths.push(path);
        }
        if frames > 0 {
            groups.push(TrackGroup {
                paths,
                sample_rate,
                channels,
                frames,
            });
        }
    }
    Ok(groups)
}

struct TrackStream {
    group: TrackGroup,
    part_index: usize,
    samples: Option<SampleStream>,
    current_frame: Vec<f32>,
    current_index: Option<u64>,
    next_index: u64,
}

impl TrackStream {
    fn new(group: TrackGroup) -> Self {
        Self {
            group,
            part_index: 0,
            samples: None,
            current_frame: Vec::new(),
            current_index: None,
            next_index: 0,
        }
    }

    fn open_next_part(&mut self) -> Result<bool, String> {
        let Some(path) = self.group.paths.get(self.part_index) else {
            return Ok(false);
        };
        let (samples, spec, _) = SampleStream::open(path)?;
        if spec.sample_rate != self.group.sample_rate || spec.channels != self.group.channels {
            return Err(format!("WAV part format changed: {}", path.display()));
        }
        self.part_index += 1;
        self.samples = Some(samples);
        Ok(true)
    }

    fn read_frame(&mut self) -> Result<bool, String> {
        loop {
            if self.samples.is_none() && !self.open_next_part()? {
                return Ok(false);
            }

            let mut frame = Vec::with_capacity(self.group.channels as usize);
            for _ in 0..self.group.channels {
                match self.samples.as_mut().unwrap().next()? {
                    Some(sample) => frame.push(sample),
                    None if frame.is_empty() => {
                        self.samples = None;
                        break;
                    }
                    None => return Err("Truncated WAV frame".into()),
                }
            }
            if frame.len() != self.group.channels as usize {
                continue;
            }

            self.current_frame = frame;
            self.current_index = Some(self.next_index);
            self.next_index += 1;
            return Ok(true);
        }
    }

    fn sample_at(
        &mut self,
        output_frame: u64,
        output_rate: u32,
        output_channel: usize,
    ) -> Result<f32, String> {
        let target =
            ((output_frame as u128 * self.group.sample_rate as u128) / output_rate as u128) as u64;
        while self.current_index.is_none_or(|index| index < target) {
            if !self.read_frame()? {
                return Ok(0.0);
            }
        }
        let channel = if self.group.channels == 1 {
            0
        } else {
            output_channel.min(self.group.channels as usize - 1)
        };
        Ok(self.current_frame[channel])
    }
}

fn mix_plan(groups: &[TrackGroup]) -> Result<(u32, u16, u64), String> {
    let max_seconds = groups
        .iter()
        .map(|track| track.frames as f64 / track.sample_rate as f64)
        .fold(0.0f64, f64::max);
    let max_rate = groups
        .iter()
        .map(|track| track.sample_rate)
        .max()
        .unwrap_or(0);
    let channels = groups
        .iter()
        .map(|track| track.channels)
        .max()
        .unwrap_or(0)
        .min(2);
    if max_seconds <= 0.0 || max_rate == 0 || channels == 0 {
        return Err("No readable WAV tracks found to merge".into());
    }

    let max_rate_for_wav = (3_900_000_000f64 / (max_seconds * channels as f64 * 2.0)) as u32;
    let output_rate = if max_rate <= max_rate_for_wav {
        max_rate
    } else {
        [
            48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025, 8_000,
        ]
        .into_iter()
        .find(|rate| *rate <= max_rate && *rate <= max_rate_for_wav)
        .ok_or("Recording is too long for a safe temporary WAV")?
    };
    let output_frames = (max_seconds * output_rate as f64).ceil() as u64;
    Ok((output_rate, channels, output_frames))
}

fn streaming_mix(groups: &[TrackGroup], output: &Path) -> Result<(), String> {
    let (output_rate, output_channels, output_frames) = mix_plan(groups)?;
    let mut tracks: Vec<_> = groups.iter().cloned().map(TrackStream::new).collect();
    let mut peak = 0.0f32;
    for frame in 0..output_frames {
        for channel in 0..output_channels as usize {
            let mut mixed = 0.0f32;
            for track in &mut tracks {
                mixed += track.sample_at(frame, output_rate, channel)?;
            }
            peak = peak.max(mixed.abs());
        }
    }

    let scale = if peak > 1.0 { peak.recip() } else { 1.0 };
    let spec = hound::WavSpec {
        channels: output_channels,
        sample_rate: output_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output, spec)
        .map_err(|e| format!("Create mix {}: {e}", output.display()))?;
    let mut tracks: Vec<_> = groups.iter().cloned().map(TrackStream::new).collect();
    for frame in 0..output_frames {
        for channel in 0..output_channels as usize {
            let mut mixed = 0.0f32;
            for track in &mut tracks {
                mixed += track.sample_at(frame, output_rate, channel)?;
            }
            writer
                .write_sample((mixed * scale * i16::MAX as f32) as i16)
                .map_err(|e| format!("Write mix: {e}"))?;
        }
    }
    writer
        .finalize()
        .map_err(|e| format!("Finalize mix: {e}"))?;
    Ok(())
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
    let groups = source_track_groups(session_dir)?;
    if groups.is_empty() {
        return Err("No WAV files found in session".into());
    }
    if groups.len() == 1 && groups[0].paths.len() == 1 {
        return export_audio(&groups[0].paths[0], output_path, format, aac_bitrate_kbps);
    }

    let output_stem = output_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let mixed_wav = session_dir.join(format!("_mixed_{output_stem}.wav"));
    let _ = std::fs::remove_file(&mixed_wav);
    info!(
        "Streaming {} track(s) from {}",
        groups.len(),
        session_dir.display()
    );
    let result = streaming_mix(&groups, &mixed_wav)
        .and_then(|()| export_audio(&mixed_wav, output_path, format, aac_bitrate_kbps));
    let _ = std::fs::remove_file(&mixed_wav);
    result
}

static COMPACTION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(target_os = "macos")]
fn audio_duration(path: &Path) -> Result<f64, String> {
    let output = std::process::Command::new("afinfo")
        .arg(path)
        .output()
        .map_err(|e| format!("afinfo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "afinfo failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("estimated duration:")
                .and_then(|value| value.split_whitespace().next())
                .and_then(|value| value.parse().ok())
        })
        .ok_or_else(|| "afinfo did not report a duration".into())
}

#[cfg(not(target_os = "macos"))]
fn audio_duration(_path: &Path) -> Result<f64, String> {
    Err("Audio compaction requires macOS".into())
}

/// Outcome of [`compact_session`].
#[derive(Debug, Clone)]
pub struct CompactResult {
    /// The compact playback file that now represents the session's audio.
    pub playback_path: PathBuf,
    /// Bytes reclaimed by deleting raw WAVs (0 when `delete_raw` is false).
    pub freed_bytes: u64,
    /// Number of raw WAV files removed.
    pub removed: usize,
}

/// Compress a session's raw multi-track WAVs into a single compact AAC
/// `playback.m4a`, then optionally delete the raw WAVs to reclaim disk space.
///
/// Transcripts, summaries and speaker labels live in the database and are never
/// touched here. Returns `Ok(None)` when there is nothing to compact (no source
/// WAVs present). A raw WAV is only deleted after the compressed file has been
/// produced and passes a size sanity check, so a failed/partial encode always
/// leaves the originals intact.
pub fn compact_session(
    session_dir: &Path,
    aac_bitrate_kbps: u32,
    delete_raw: bool,
) -> Result<Option<CompactResult>, String> {
    let _guard = COMPACTION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Audio compaction lock poisoned")?;
    let groups = source_track_groups(session_dir)?;
    if groups.is_empty() {
        return Ok(None);
    }
    let mut wavs: Vec<PathBuf> = groups
        .iter()
        .flat_map(|group| group.paths.iter().cloned())
        .collect();
    for name in ["master.wav", "_playback_temp.wav", "playback.wav"] {
        let path = session_dir.join(name);
        if path.is_file() && !wavs.contains(&path) {
            wavs.push(path);
        }
    }
    for path in std::fs::read_dir(session_dir)
        .map_err(|e| format!("Read dir: {e}"))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
    {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
            && (name.starts_with("_mixed_")
                || name.starts_with("_playback_")
                || name.starts_with("master.part"))
            && !wavs.contains(&path)
        {
            wavs.push(path);
        }
    }

    let source_duration = groups
        .iter()
        .map(|track| track.frames as f64 / track.sample_rate as f64)
        .fold(0.0f64, f64::max);
    let playback = session_dir.join("playback.m4a");
    let building = session_dir.join("playback.building.m4a");
    let _ = std::fs::remove_file(&building);
    if let Err(error) = merge_and_export(
        session_dir,
        &building,
        ExportFormat::M4aAac,
        aac_bitrate_kbps,
    ) {
        let _ = std::fs::remove_file(&building);
        return Err(error);
    }

    let out_bytes = std::fs::metadata(&building)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let output_duration = match audio_duration(&building) {
        Ok(duration) => duration,
        Err(error) => {
            let _ = std::fs::remove_file(&building);
            return Err(error);
        }
    };
    let tolerance = (source_duration * 0.01).max(2.0);
    if out_bytes <= 100
        || source_duration <= 0.0
        || (source_duration - output_duration).abs() > tolerance
    {
        let _ = std::fs::remove_file(&building);
        return Err(format!(
            "Compressed audio verification failed: source {source_duration:.3}s, output {output_duration:.3}s, {out_bytes} bytes; raw WAVs kept"
        ));
    }
    std::fs::rename(&building, &playback).map_err(|e| format!("Install playback.m4a: {e}"))?;

    let mut freed_bytes = 0u64;
    let mut removed = 0usize;
    if delete_raw {
        for wav in &wavs {
            let size = std::fs::metadata(wav).map(|meta| meta.len()).unwrap_or(0);
            if std::fs::remove_file(wav).is_ok() {
                freed_bytes += size;
                removed += 1;
            }
        }
        info!(
            "Compacted {}: playback.m4a {} bytes, freed {} bytes across {} WAV(s)",
            session_dir.display(),
            out_bytes,
            freed_bytes,
            removed
        );
    }

    Ok(Some(CompactResult {
        playback_path: playback,
        freed_bytes,
        removed,
    }))
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

    fn write_test_wav(path: &Path, sample_rate: u32, channels: u16, frames: u32) {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for _ in 0..frames * u32::from(channels) {
            writer.write_sample(1000i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    fn test_dir(name: &str) -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("gravai_{name}_{suffix}"))
    }

    #[test]
    fn streaming_mix_concatenates_parts_and_resamples_tracks() {
        let dir = test_dir("encoder");
        std::fs::create_dir_all(&dir).unwrap();
        write_test_wav(&dir.join("mic.wav"), 16_000, 1, 1600);
        write_test_wav(&dir.join("mic.part0002.wav"), 16_000, 1, 1600);
        write_test_wav(&dir.join("system.wav"), 48_000, 2, 9600);

        let groups = source_track_groups(&dir).unwrap();
        assert_eq!(groups.len(), 2);
        let output = dir.join("mixed.wav");
        streaming_mix(&groups, &output).unwrap();

        let reader = hound::WavReader::open(&output).unwrap();
        assert_eq!(reader.spec().sample_rate, 48_000);
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.duration(), 9600);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn compact_session_verifies_output_before_deleting_raw_tracks() {
        let dir = test_dir("compact");
        std::fs::create_dir_all(&dir).unwrap();
        let mic = dir.join("mic.wav");
        let mic_part = dir.join("mic.part0002.wav");
        let system = dir.join("system.wav");
        let export = dir.join("export.wav");
        write_test_wav(&mic, 16_000, 1, 1600);
        write_test_wav(&mic_part, 16_000, 1, 1600);
        write_test_wav(&system, 48_000, 2, 9600);
        write_test_wav(&export, 48_000, 2, 4800);

        let result = compact_session(&dir, 128, true).unwrap().unwrap();
        assert_eq!(result.removed, 3);
        assert!(!mic.exists());
        assert!(!mic_part.exists());
        assert!(!system.exists());
        assert!(export.exists());
        assert!(result.playback_path.exists());
        assert!(audio_duration(&result.playback_path).unwrap() >= 0.19);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// Allocate a separate capture directory. The prefix is its position on the
/// session timeline; the suffix prevents a failed/retried start overwriting audio.
pub fn continuation_directory(session_dir: &Path, offset_ms: u64) -> Result<PathBuf, String> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = session_dir
        .join("continuations")
        .join(format!("{offset_ms:020}_{nonce}"));
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Export the complete timeline, including captures made after a session stopped.
/// Original files remain untouched. Compaction uses the same lock so compressed
/// sources cannot disappear while this export is reading them.
pub fn export_session_timeline(
    session_dir: &Path,
    output: &Path,
    format: ExportFormat,
    bitrate: u32,
) -> Result<(), String> {
    let _guard = COMPACTION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Audio compaction lock poisoned")?;
    if !session_dir.join("continuations").exists() && !source_track_groups(session_dir)?.is_empty()
    {
        return merge_and_export(session_dir, output, format, bitrate);
    }
    let mut segments = vec![(0_u64, session_dir.to_path_buf())];
    let continuation_root = session_dir.join("continuations");
    if continuation_root.exists() {
        let mut continued = Vec::new();
        for entry in std::fs::read_dir(continuation_root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.path().is_dir() {
                let name = entry.file_name();
                let offset = name
                    .to_string_lossy()
                    .split('_')
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .ok_or("Invalid continuation directory")?;
                continued.push((offset, entry.path()));
            }
        }
        continued.sort();
        segments.extend(continued);
    }
    let scratch = continuation_scratch()?;
    let result = (|| {
        let mut prepared = Vec::new();
        for (index, (offset, dir)) in segments.iter().enumerate() {
            let wav = scratch.0.join(format!("{index}.wav"));
            let groups = source_track_groups(dir)?;
            if !groups.is_empty() {
                streaming_mix(&groups, &wav)?;
            } else if dir.join("playback.m4a").is_file() {
                decode_playback(&dir.join("playback.m4a"), &wav)?;
            } else if dir.join("playback.wav").is_file() {
                ensure_pcm16_wav(&dir.join("playback.wav"), &wav)?;
            } else {
                continue; // Transcription-only captures have no audio.
            }
            let (_, spec, frames) = SampleStream::open(&wav)?;
            prepared.push((
                *offset,
                TrackGroup {
                    paths: vec![wav],
                    sample_rate: spec.sample_rate,
                    channels: spec.channels,
                    frames: frames as u64,
                },
            ));
        }
        if prepared.is_empty() {
            return Err("No audio files found for this session".into());
        }
        let mixed = scratch.0.join("timeline.wav");
        concatenate_captures(&prepared, &mixed)?;
        export_audio(&mixed, output, format, bitrate)
    })();
    result
}

struct TimelineScratch(PathBuf);
impl Drop for TimelineScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn continuation_scratch() -> Result<TimelineScratch, String> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("gravai-timeline-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok(TimelineScratch(path))
}

fn decode_playback(source: &Path, output: &Path) -> Result<(), String> {
    let result = std::process::Command::new("afconvert")
        .arg(source)
        .arg(output)
        .args(["-f", "WAVE", "-d", "LEI16"])
        .output()
        .map_err(|e| format!("Decode saved audio: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "Decode saved audio: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(())
}

fn concatenate_captures(captures: &[(u64, TrackGroup)], output: &Path) -> Result<(), String> {
    let seconds = captures
        .iter()
        .map(|(offset, group)| {
            *offset as f64 / 1000.0 + group.frames as f64 / group.sample_rate as f64
        })
        .fold(0.0_f64, f64::max);
    // Keep the temporary PCM below WAV's 4 GB limit for long recordings.
    let max_rate = (3_900_000_000.0 / (seconds * 4.0)) as u32;
    let output_rate = [48_000, 44_100, 32_000, 24_000, 16_000, 12_000, 8_000]
        .into_iter()
        .find(|rate| *rate <= max_rate)
        .ok_or("Recording is too long for a safe timeline WAV")?;
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: output_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output, spec).map_err(|e| e.to_string())?;
    let mut position = 0_u64;
    for (index, (offset, group)) in captures.iter().enumerate() {
        let start = offset.saturating_mul(output_rate as u64) / 1000;
        while position < start {
            writer.write_sample(0_i16).map_err(|e| e.to_string())?;
            writer.write_sample(0_i16).map_err(|e| e.to_string())?;
            position += 1;
        }
        let frames =
            (group.frames as f64 * output_rate as f64 / group.sample_rate as f64).ceil() as u64;
        let end = captures
            .get(index + 1)
            .map(|(next, _)| next.saturating_mul(output_rate as u64) / 1000)
            .unwrap_or(start.saturating_add(frames));
        let mut track = TrackStream::new(group.clone());
        for frame in 0..frames.min(end.saturating_sub(start)) {
            for channel in 0..2 {
                let sample = track.sample_at(frame, output_rate, channel)?;
                writer
                    .write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                    .map_err(|e| e.to_string())?;
            }
            position += 1;
        }
    }
    writer.finalize().map_err(|e| e.to_string())
}

#[cfg(test)]
mod continuation_tests {
    use super::*;

    fn tone(path: &Path, rate: u32, sample: i16) {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..rate / 10 {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn continued_audio_keeps_sources_and_aligns_multiple_captures() {
        let dir = continuation_scratch().unwrap();
        tone(&dir.0.join("mic.wav"), 16_000, 8_000);
        let original = std::fs::read(dir.0.join("mic.wav")).unwrap();
        let second = continuation_directory(&dir.0, 200).unwrap();
        tone(&second.join("mic.wav"), 48_000, -8_000);
        let third = continuation_directory(&dir.0, 300).unwrap();
        tone(&third.join("system.wav"), 24_000, 4_000);
        let output = dir.0.join("export.wav");
        export_session_timeline(&dir.0, &output, ExportFormat::Wav, 128).unwrap();
        let reader = hound::WavReader::open(&output).unwrap();
        assert_eq!(reader.duration(), 19_200); // .4 seconds at 48kHz
        let samples: Vec<i16> = reader.into_samples::<i16>().map(Result::unwrap).collect();
        assert!(samples[100] > 7_900);
        assert_eq!(samples[14_400], 0); // .15s: silence padding before continuation
        assert!(samples[24_000] < -7_900); // .25s: second capture
        assert!(samples[33_600] > 3_900); // .35s: third capture
        assert_eq!(std::fs::read(dir.0.join("mic.wav")).unwrap(), original);
        // Rebuilding playback must not accidentally include the last export.
        export_session_timeline(&dir.0, &output, ExportFormat::Wav, 128).unwrap();
        assert_eq!(hound::WavReader::open(output).unwrap().duration(), 19_200);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn continued_audio_includes_compacted_original() {
        let dir = continuation_scratch().unwrap();
        tone(&dir.0.join("mic.wav"), 48_000, 8_000);
        compact_session(&dir.0, 128, true).unwrap().unwrap();
        let original = std::fs::read(dir.0.join("playback.m4a")).unwrap();
        let second = continuation_directory(&dir.0, 100).unwrap();
        tone(&second.join("mic.wav"), 48_000, -8_000);
        compact_session(&second, 128, true).unwrap().unwrap();
        let output = dir.0.join("export.wav");
        export_session_timeline(&dir.0, &output, ExportFormat::Wav, 128).unwrap();
        let reader = hound::WavReader::open(output).unwrap();
        assert!(reader.duration() >= 9_600);
        let samples: Vec<i16> = reader.into_samples::<i16>().map(Result::unwrap).collect();
        assert!(samples[2_400] > 5_000);
        assert!(samples[14_400] < -5_000);
        assert_eq!(std::fs::read(dir.0.join("playback.m4a")).unwrap(), original);
    }
}
