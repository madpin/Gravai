//! Multi-track WAV recorder.
//!
//! Writes each audio source to its own 16- or 24-bit integer-PCM WAV file.

use hound::{SampleFormat, WavSpec, WavWriter};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::capture::AudioChunk;

const MAX_DATA_BYTES: u64 = 4_000_000_000;

/// A single-track WAV writer.
pub struct TrackWriter {
    writer: Option<WavWriter<BufWriter<std::fs::File>>>,
    pub path: PathBuf,
    bits: u16,
    data_bytes: u64,
    max_data_bytes: u64,
    part_index: u32,
    sample_count: u64,
    pub gain: f32,
    pub pan: f32,
    /// Whether the spec has been set from the first chunk
    spec_set: bool,
}

impl TrackWriter {
    /// Create a new track writer. The actual WAV spec is set from the first audio chunk
    /// to avoid sample rate/channel mismatches.
    pub fn new(path: PathBuf, bits: u16) -> Self {
        Self {
            writer: None,
            path,
            bits: if bits == 24 { 24 } else { 16 },
            data_bytes: 0,
            max_data_bytes: MAX_DATA_BYTES,
            part_index: 1,
            sample_count: 0,
            gain: 1.0,
            pan: 0.0,
            spec_set: false,
        }
    }

    fn current_path(&self) -> PathBuf {
        if self.part_index == 1 {
            return self.path.clone();
        }
        let stem = self.path.file_stem().unwrap_or_default().to_string_lossy();
        self.path
            .with_file_name(format!("{stem}.part{:04}.wav", self.part_index))
    }

    /// Initialize the WAV writer from the first chunk's actual format.
    fn ensure_writer(&mut self, chunk: &AudioChunk) -> Result<(), String> {
        if self.spec_set {
            return Ok(());
        }

        let spec = WavSpec {
            channels: chunk.channels,
            sample_rate: chunk.sample_rate,
            bits_per_sample: self.bits,
            sample_format: SampleFormat::Int,
        };
        let path = self.current_path();
        let writer = WavWriter::create(&path, spec)
            .map_err(|e| format!("Create WAV {}: {e}", path.display()))?;

        info!(
            "Recording track: {} ({}Hz {}ch i{})",
            path.display(),
            chunk.sample_rate,
            chunk.channels,
            self.bits
        );
        self.writer = Some(writer);
        self.spec_set = true;
        Ok(())
    }

    fn roll(&mut self, chunk: &AudioChunk) -> Result<(), String> {
        if let Some(writer) = self.writer.take() {
            writer
                .finalize()
                .map_err(|e| format!("Finalize WAV part: {e}"))?;
        }
        self.part_index += 1;
        self.data_bytes = 0;
        self.spec_set = false;
        warn!(
            "Track {} reached the WAV size limit; continuing in part {}",
            self.path.display(),
            self.part_index
        );
        self.ensure_writer(chunk)
    }

    /// Write audio chunk to this track. Applies gain.
    pub fn write(&mut self, chunk: &AudioChunk) -> Result<(), String> {
        let bytes_per_sample = (self.bits / 8) as u64;
        let chunk_bytes = chunk.samples.len() as u64 * bytes_per_sample;
        if chunk_bytes > self.max_data_bytes {
            return Err("Audio chunk exceeds WAV part size limit".into());
        }

        self.ensure_writer(chunk)?;
        if self.data_bytes > 0 && self.data_bytes + chunk_bytes > self.max_data_bytes {
            self.roll(chunk)?;
        }

        let bits = self.bits;
        let gain = self.gain;
        let writer = self.writer.as_mut().ok_or("Writer not initialized")?;
        if bits == 24 {
            for &sample in &chunk.samples {
                let value = ((sample * gain).clamp(-1.0, 1.0) * 8_388_607.0) as i32;
                writer
                    .write_sample(value)
                    .map_err(|e| format!("Write sample: {e}"))?;
            }
        } else {
            for &sample in &chunk.samples {
                let value = ((sample * gain).clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                writer
                    .write_sample(value)
                    .map_err(|e| format!("Write sample: {e}"))?;
            }
        }
        self.data_bytes += chunk_bytes;
        self.sample_count += chunk.samples.len() as u64;
        Ok(())
    }

    /// Finalize the WAV file (writes header with correct length).
    pub fn finalize(mut self) -> Result<PathBuf, String> {
        let path = self.path.clone();
        if let Some(writer) = self.writer.take() {
            writer
                .finalize()
                .map_err(|e| format!("Finalize WAV: {e}"))?;
            info!(
                "Track finalized: {} ({} samples, {} part(s))",
                path.display(),
                self.sample_count,
                self.part_index
            );
        } else {
            warn!("Track {} had no audio data", path.display());
        }
        Ok(path)
    }
}

impl Drop for TrackWriter {
    fn drop(&mut self) {
        // Safety finalize — if finalize() wasn't called explicitly,
        // at least try to flush and close the WAV properly.
        if let Some(writer) = self.writer.take() {
            if let Err(e) = writer.finalize() {
                warn!("Drop-finalize WAV {}: {e}", self.path.display());
            }
        }
    }
}

/// Manages multi-track recording for a session.
pub struct MultiTrackRecorder {
    session_dir: PathBuf,
    bits: u16,
    tracks: Vec<(String, TrackWriter)>,
    master: Option<TrackWriter>,
}

impl MultiTrackRecorder {
    pub fn new(session_dir: &Path, bits: u16) -> Result<Self, String> {
        std::fs::create_dir_all(session_dir).map_err(|e| format!("Create session dir: {e}"))?;

        Ok(Self {
            session_dir: session_dir.to_path_buf(),
            bits: if bits == 24 { 24 } else { 16 },
            tracks: Vec::new(),
            master: None,
        })
    }

    /// Add a named track (e.g. "mic", "system").
    pub fn add_track(&mut self, name: &str) -> Result<(), String> {
        let path = self.session_dir.join(format!("{name}.wav"));
        let writer = TrackWriter::new(path, self.bits);
        self.tracks.push((name.to_string(), writer));
        Ok(())
    }

    /// Initialize the master mix track.
    pub fn init_master(&mut self) -> Result<(), String> {
        let path = self.session_dir.join("master.wav");
        let writer = TrackWriter::new(path, self.bits);
        self.master = Some(writer);
        Ok(())
    }

    /// Write a chunk to the named track.
    pub fn write_track(&mut self, name: &str, chunk: &AudioChunk) -> Result<(), String> {
        if let Some((_, writer)) = self.tracks.iter_mut().find(|(n, _)| n == name) {
            writer.write(chunk)?;
        }
        Ok(())
    }

    /// Write a chunk to the master mix track.
    pub fn write_master(&mut self, chunk: &AudioChunk) -> Result<(), String> {
        if let Some(ref mut master) = self.master {
            master.write(chunk)?;
        }
        Ok(())
    }

    /// Set gain for a track.
    pub fn set_track_gain(&mut self, name: &str, gain: f32) {
        if let Some((_, writer)) = self.tracks.iter_mut().find(|(n, _)| n == name) {
            writer.gain = gain;
        }
    }

    /// Finalize all tracks and the master.
    pub fn finalize(self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for (name, writer) in self.tracks {
            match writer.finalize() {
                Ok(p) => paths.push(p),
                Err(e) => warn!("Finalize track {name}: {e}"),
            }
        }
        if let Some(master) = self.master {
            match master.finalize() {
                Ok(p) => paths.push(p),
                Err(e) => warn!("Finalize master: {e}"),
            }
        }
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(samples: usize) -> AudioChunk {
        AudioChunk {
            samples: vec![0.1; samples],
            sample_rate: 48_000,
            channels: 2,
        }
    }

    #[test]
    fn track_writer_creates_pcm16_wav() {
        let dir = std::env::temp_dir().join("gravai_test_recorder");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.wav");
        let mut writer = TrackWriter::new(path.clone(), 16);

        writer.write(&chunk(1024)).unwrap();
        writer.finalize().unwrap();

        // Verify it's a valid WAV
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 48_000);
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().bits_per_sample, 16);
        assert_eq!(reader.spec().sample_format, SampleFormat::Int);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn track_writer_rolls_before_wav_limit() {
        let dir = std::env::temp_dir().join("gravai_test_recorder_roll");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.wav");
        let mut writer = TrackWriter::new(path.clone(), 16);
        writer.max_data_bytes = 4096;

        writer.write(&chunk(2000)).unwrap();
        writer.write(&chunk(2000)).unwrap();
        writer.finalize().unwrap();

        let part = dir.join("test.part0002.wav");
        assert_eq!(hound::WavReader::open(&path).unwrap().duration(), 1000);
        assert_eq!(hound::WavReader::open(&part).unwrap().duration(), 1000);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
