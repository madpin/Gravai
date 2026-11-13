//! Find and repair recordings that did not finish cleanly.
//!
//! `hound` writes the WAV length into the header only when a track is
//! finalized. If Gravai quits, crashes, or loses power mid-recording, the
//! samples are on disk but the header still says "0 bytes", so every reader
//! (and compaction) treats the track as empty. This module reads the RIFF
//! chunks directly, works out the real length from the file size, and can
//! rewrite the two length fields so the audio is playable again.

use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Files a cleanly stopped recording never leaves behind.
const TEMP_FILES: &[&str] = &["playback.building.m4a", "_playback_temp.wav"];

/// What one WAV file on disk really holds.
#[derive(Debug, Clone, Serialize)]
pub struct WavInfo {
    pub path: PathBuf,
    pub file_bytes: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    /// Seconds of audio actually present in the file.
    pub seconds: f64,
    /// False when the header disagrees with the file (never finalized).
    pub header_ok: bool,
    /// Byte offset of the `data` length field and of the samples.
    #[serde(skip)]
    data_size_offset: u64,
    #[serde(skip)]
    data_start: u64,
    #[serde(skip)]
    header_data_bytes: u32,
}

impl WavInfo {
    fn block_align(&self) -> u64 {
        (u64::from(self.channels) * u64::from(self.bits_per_sample) / 8).max(1)
    }

    /// Whole frames of audio present after the header.
    fn real_data_bytes(&self) -> u64 {
        let available = self.file_bytes.saturating_sub(self.data_start);
        available - available % self.block_align()
    }
}

fn read_u32(buf: &[u8]) -> u32 {
    u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}

fn read_u16(buf: &[u8]) -> u16 {
    u16::from_le_bytes([buf[0], buf[1]])
}

/// Read a WAV's chunks without trusting its length fields.
pub fn inspect_wav(path: &Path) -> Result<WavInfo, String> {
    let mut file = File::open(path).map_err(|e| format!("Open {}: {e}", path.display()))?;
    let file_bytes = file.metadata().map_err(|e| e.to_string())?.len();
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff)
        .map_err(|_| format!("{} is too short to be a WAV", path.display()))?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return Err(format!("{} is not a WAV file", path.display()));
    }

    let mut format: Option<(u16, u32, u16)> = None;
    let mut pos = 12u64;
    loop {
        let mut head = [0u8; 8];
        if file.read_exact(&mut head).is_err() {
            return Err(format!("{} has no audio data chunk", path.display()));
        }
        let size = read_u32(&head[4..8]);
        match &head[0..4] {
            b"fmt " => {
                let mut fmt = vec![0u8; size.max(16) as usize];
                file.read_exact(&mut fmt)
                    .map_err(|_| format!("{} has a short format chunk", path.display()))?;
                format = Some((
                    read_u16(&fmt[2..4]),
                    read_u32(&fmt[4..8]),
                    read_u16(&fmt[14..16]),
                ));
                if size % 2 == 1 {
                    file.seek(SeekFrom::Current(1)).map_err(|e| e.to_string())?;
                }
            }
            b"data" => {
                let (channels, sample_rate, bits_per_sample) = format
                    .ok_or_else(|| format!("{} has data before its format", path.display()))?;
                let data_start = pos + 8;
                let mut info = WavInfo {
                    path: path.to_path_buf(),
                    file_bytes,
                    sample_rate,
                    channels,
                    bits_per_sample,
                    seconds: 0.0,
                    header_ok: true,
                    data_size_offset: pos + 4,
                    data_start,
                    header_data_bytes: size,
                };
                let real = info.real_data_bytes();
                info.header_ok = u64::from(size) == real
                    && u64::from(read_u32(&riff[4..8])) == file_bytes.saturating_sub(8);
                let bytes_per_second = u64::from(sample_rate) * info.block_align();
                if bytes_per_second > 0 {
                    info.seconds = real as f64 / bytes_per_second as f64;
                }
                return Ok(info);
            }
            _ => {
                let skip = i64::from(size) + i64::from(size % 2);
                file.seek(SeekFrom::Current(skip))
                    .map_err(|e| e.to_string())?;
            }
        }
        pos = file.stream_position().map_err(|e| e.to_string())?;
    }
}

/// Rewrite the RIFF and data lengths so they match what is on disk.
/// Drops a trailing partial frame. Returns true if anything changed.
pub fn repair_wav(path: &Path) -> Result<bool, String> {
    let info = inspect_wav(path)?;
    if info.header_ok {
        return Ok(false);
    }
    let real = info.real_data_bytes();
    if real > u64::from(u32::MAX) {
        return Err(format!(
            "{} is larger than a WAV can describe",
            path.display()
        ));
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("Open {}: {e}", path.display()))?;
    let new_len = info.data_start + real;
    if new_len < info.file_bytes {
        file.set_len(new_len).map_err(|e| e.to_string())?;
    }
    let riff_size = (new_len - 8) as u32;
    file.seek(SeekFrom::Start(4)).map_err(|e| e.to_string())?;
    file.write_all(&riff_size.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(info.data_size_offset))
        .map_err(|e| e.to_string())?;
    file.write_all(&(real as u32).to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    tracing::info!(
        "Repaired WAV header {} ({} -> {} data bytes)",
        path.display(),
        info.header_data_bytes,
        real
    );
    Ok(true)
}

/// One folder of a recording: the original, or a continuation segment.
#[derive(Debug, Clone, Serialize)]
pub struct SegmentReport {
    pub dir: PathBuf,
    /// Start of this segment in the session timeline.
    pub offset_ms: u64,
    pub tracks: Vec<WavInfo>,
    pub has_playback: bool,
    pub temp_files: Vec<PathBuf>,
    /// Newest modification time of any file here (Unix seconds).
    pub last_write: Option<u64>,
}

impl SegmentReport {
    /// Longest track in this segment.
    pub fn seconds(&self) -> f64 {
        self.tracks.iter().map(|t| t.seconds).fold(0.0, f64::max)
    }

    pub fn needs_repair(&self) -> bool {
        self.tracks.iter().any(|t| !t.header_ok)
    }

    pub fn raw_bytes(&self) -> u64 {
        self.tracks.iter().map(|t| t.file_bytes).sum()
    }
}

/// Survey a session folder and any continuation segments inside it.
pub fn inspect_session_dir(dir: &Path) -> Vec<SegmentReport> {
    let mut segments = Vec::new();
    if let Some(seg) = inspect_segment(dir, 0) {
        segments.push(seg);
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("continuations")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let offset = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| {
                    n.trim_start_matches(|c: char| !c.is_ascii_digit())
                        .parse()
                        .ok()
                })
                .unwrap_or(0);
            if let Some(seg) = inspect_segment(&path, offset) {
                segments.push(seg);
            }
        }
    }
    segments.sort_by_key(|s| s.offset_ms);
    segments
}

fn inspect_segment(dir: &Path, offset_ms: u64) -> Option<SegmentReport> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut tracks = Vec::new();
    let mut temp_files = Vec::new();
    let mut has_playback = false;
    let mut last_write = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(modified) = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        {
            last_write = Some(last_write.unwrap_or(0).max(modified.as_secs()));
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if TEMP_FILES.contains(&name.as_str()) {
            temp_files.push(path);
            continue;
        }
        if name == "playback.m4a" || name == "playback.wav" {
            has_playback = true;
            continue;
        }
        let is_track = name.ends_with(".wav")
            && !name.starts_with('_')
            && !name.starts_with("export")
            && !name.contains("trimmed");
        if is_track {
            if let Ok(info) = inspect_wav(&path) {
                tracks.push(info);
            }
        }
    }
    tracks.sort_by(|a, b| a.path.cmp(&b.path));
    if tracks.is_empty() && temp_files.is_empty() && !has_playback {
        return None;
    }
    Some(SegmentReport {
        dir: dir.to_path_buf(),
        offset_ms,
        tracks,
        has_playback,
        temp_files,
        last_write,
    })
}

/// Repair every track header and remove leftover temp files in each segment.
/// Returns the number of tracks repaired.
pub fn repair_session_dir(dir: &Path) -> Result<usize, String> {
    let mut repaired = 0;
    for segment in inspect_session_dir(dir) {
        for track in &segment.tracks {
            if !track.header_ok && repair_wav(&track.path)? {
                repaired += 1;
            }
        }
        for temp in &segment.temp_files {
            let _ = std::fs::remove_file(temp);
        }
    }
    Ok(repaired)
}

/// Length of the whole recording across segments, in seconds.
pub fn timeline_seconds(segments: &[SegmentReport]) -> f64 {
    segments
        .iter()
        .map(|s| s.offset_ms as f64 / 1000.0 + s.seconds())
        .fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp dir, removed when dropped.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!("gravai_recovery_{name}_{nonce}"));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write_wav(path: &Path, frames: usize, finalize: bool) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames * 2 {
            writer.write_sample((i % 1000) as i16).unwrap();
        }
        writer.finalize().unwrap();
        if !finalize {
            // What a crash leaves: hound writes zero lengths when it creates
            // the file and only fills them in on finalize.
            let info = inspect_wav(path).unwrap();
            let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
            f.seek(SeekFrom::Start(4)).unwrap();
            f.write_all(&((info.data_start - 8) as u32).to_le_bytes())
                .unwrap();
            f.seek(SeekFrom::Start(info.data_size_offset)).unwrap();
            f.write_all(&0u32.to_le_bytes()).unwrap();
        }
    }

    #[test]
    fn finalized_wav_is_healthy() {
        let dir = TempDir::new("t1");
        let path = dir.path().join("mic.wav");
        write_wav(&path, 48_000, true);
        let info = inspect_wav(&path).unwrap();
        assert!(info.header_ok);
        assert!((info.seconds - 1.0).abs() < 1e-6);
        assert!(!repair_wav(&path).unwrap());
    }

    #[test]
    fn unfinalized_wav_is_detected_and_repaired() {
        let dir = TempDir::new("t2");
        let path = dir.path().join("system.wav");
        write_wav(&path, 96_000, false);
        let info = inspect_wav(&path).unwrap();
        assert!(!info.header_ok);
        assert!(
            (info.seconds - 2.0).abs() < 1e-6,
            "seconds from file size, not header"
        );

        assert!(repair_wav(&path).unwrap());
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.duration(), 96_000);
        assert!(inspect_wav(&path).unwrap().header_ok);
    }

    #[test]
    fn partial_trailing_frame_is_dropped() {
        let dir = TempDir::new("t3");
        let path = dir.path().join("mic.wav");
        write_wav(&path, 1_000, false);
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(&[1, 2, 3]).unwrap();
        drop(f);
        assert!(repair_wav(&path).unwrap());
        assert_eq!(hound::WavReader::open(&path).unwrap().duration(), 1_000);
    }

    #[test]
    fn session_dir_survey_and_repair() {
        let dir = TempDir::new("t4");
        write_wav(&dir.path().join("mic.wav"), 48_000, false);
        write_wav(&dir.path().join("system.wav"), 24_000, true);
        std::fs::write(dir.path().join("playback.building.m4a"), b"x").unwrap();
        let cont = dir.path().join("continuations").join("000000090000");
        std::fs::create_dir_all(&cont).unwrap();
        write_wav(&cont.join("mic.wav"), 48_000, false);

        let segments = inspect_session_dir(dir.path());
        assert_eq!(segments.len(), 2);
        assert!(segments[0].needs_repair());
        assert_eq!(segments[0].temp_files.len(), 1);
        assert_eq!(segments[1].offset_ms, 90_000);
        assert!((timeline_seconds(&segments) - 91.0).abs() < 1e-6);

        assert_eq!(repair_session_dir(dir.path()).unwrap(), 2);
        let after = inspect_session_dir(dir.path());
        assert!(after
            .iter()
            .all(|s| !s.needs_repair() && s.temp_files.is_empty()));
    }

    /// The Convert path: a crashed recording compacts only after repair.
    #[cfg(target_os = "macos")]
    #[test]
    fn repaired_session_compacts_to_playback() {
        let dir = TempDir::new("convert");
        write_wav(&dir.path().join("mic.wav"), 96_000, false);
        write_wav(&dir.path().join("system.wav"), 96_000, false);
        assert!(
            crate::encoder::compact_session(dir.path(), 128, false)
                .unwrap()
                .is_none(),
            "unrepaired tracks look empty, so there is nothing to compact"
        );
        assert_eq!(repair_session_dir(dir.path()).unwrap(), 2);
        let result = crate::encoder::compact_session(dir.path(), 128, true)
            .unwrap()
            .expect("repaired tracks compact");
        assert!(result.playback_path.exists());
        assert_eq!(result.removed, 2);
        let segments = inspect_session_dir(dir.path());
        assert!(segments[0].has_playback && segments[0].tracks.is_empty());
    }
}
