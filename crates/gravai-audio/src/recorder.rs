//! Multi-track WAV recorder.
//!
//! Writes each audio source to its own WAV file at 48kHz/24-bit stereo.
//! Also writes a mixed master track.

use hound::{SampleFormat, WavSpec, WavWriter};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use tracing::{error, info};

use crate::capture::AudioChunk;

/// A single-track WAV writer.
pub struct TrackWriter {
    writer: WavWriter<BufWriter<std::fs::File>>,
    pub path: PathBuf,
    sample_count: u64,
    pub gain: f32,
    pub pan: f32, // -1.0 = left, 0.0 = center, 1.0 = right
}

impl TrackWriter {
    /// Create a new track writer at the given path.
    pub fn new(path: PathBuf, sample_rate: u32, channels: u16) -> Result<Self, String> {
        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 24,
            sample_format: SampleFormat::Int,
        };

        let writer = WavWriter::create(&path, spec)
            .map_err(|e| format!("Create WAV {}: {e}", path.display()))?;

        info!("Recording track: {}", path.display());

        Ok(Self {
            writer,
            path,
            sample_count: 0,
            gain: 1.0,
            pan: 0.0,
        })
    }

    /// Write audio chunk to this track. Applies gain.
    pub fn write(&mut self, chunk: &AudioChunk) -> Result<(), String> {
        for &sample in &chunk.samples {
            let gained = sample * self.gain;
            // Convert f32 [-1.0, 1.0] to i32 for 24-bit
            let i24 = (gained.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
            self.writer
                .write_sample(i24)
                .map_err(|e| format!("Write sample: {e}"))?;
            self.sample_count += 1;
        }
        Ok(())
    }

    /// Finalize the WAV file.
    pub fn finalize(self) -> Result<PathBuf, String> {
        let path = self.path.clone();
        self.writer
            .finalize()
            .map_err(|e| format!("Finalize WAV: {e}"))?;
        info!(
            "Track finalized: {} ({} samples)",
            path.display(),
            self.sample_count
        );
        Ok(path)
    }
}

/// Manages multi-track recording for a session.
pub struct MultiTrackRecorder {
    session_dir: PathBuf,
    tracks: Vec<(String, TrackWriter)>,
    master: Option<TrackWriter>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl MultiTrackRecorder {
    pub fn new(session_dir: &Path, sample_rate: u32, channels: u16) -> Result<Self, String> {
        std::fs::create_dir_all(session_dir).map_err(|e| format!("Create session dir: {e}"))?;

        Ok(Self {
            session_dir: session_dir.to_path_buf(),
            tracks: Vec::new(),
            master: None,
            sample_rate,
            channels,
        })
    }

    /// Add a named track (e.g. "mic", "system").
    pub fn add_track(&mut self, name: &str) -> Result<(), String> {
        let path = self.session_dir.join(format!("{name}.wav"));
        let writer = TrackWriter::new(path, self.sample_rate, self.channels)?;
        self.tracks.push((name.to_string(), writer));
        Ok(())
    }

    /// Initialize the master mix track.
    pub fn init_master(&mut self) -> Result<(), String> {
        let path = self.session_dir.join("master.wav");
        let writer = TrackWriter::new(path, self.sample_rate, self.channels)?;
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
                Err(e) => error!("Finalize track {name}: {e}"),
            }
        }
        if let Some(master) = self.master {
            match master.finalize() {
                Ok(p) => paths.push(p),
                Err(e) => error!("Finalize master: {e}"),
            }
        }
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_writer_creates_and_writes() {
        let dir = std::env::temp_dir().join("gravai_test_recorder");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.wav");

        let mut tw = TrackWriter::new(path.clone(), 48000, 2).unwrap();

        let chunk = AudioChunk {
            samples: vec![0.0f32; 1024],
            sample_rate: 48000,
            channels: 2,
        };
        tw.write(&chunk).unwrap();
        let finalized = tw.finalize().unwrap();
        assert!(finalized.exists());

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }
}
