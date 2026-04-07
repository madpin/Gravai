//! Multi-track WAV recorder.
//!
//! Writes each audio source to its own WAV file using 32-bit float format
//! (lossless for f32 audio data, widely supported).

use hound::{SampleFormat, WavSpec, WavWriter};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::capture::AudioChunk;

/// A single-track WAV writer.
pub struct TrackWriter {
    writer: Option<WavWriter<BufWriter<std::fs::File>>>,
    pub path: PathBuf,
    sample_count: u64,
    pub gain: f32,
    pub pan: f32,
    /// Whether the spec has been set from the first chunk
    spec_set: bool,
}

impl TrackWriter {
    /// Create a new track writer. The actual WAV spec is set from the first audio chunk
    /// to avoid sample rate/channel mismatches.
    pub fn new(path: PathBuf) -> Self {
        Self {
            writer: None,
            path,
            sample_count: 0,
            gain: 1.0,
            pan: 0.0,
            spec_set: false,
        }
    }

    /// Initialize the WAV writer from the first chunk's actual format.
    fn ensure_writer(&mut self, chunk: &AudioChunk) -> Result<(), String> {
        if self.spec_set {
            return Ok(());
        }

        let spec = WavSpec {
            channels: chunk.channels,
            sample_rate: chunk.sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };

        let writer = WavWriter::create(&self.path, spec)
            .map_err(|e| format!("Create WAV {}: {e}", self.path.display()))?;

        info!(
            "Recording track: {} ({}Hz {}ch f32)",
            self.path.display(),
            chunk.sample_rate,
            chunk.channels
        );

        self.writer = Some(writer);
        self.spec_set = true;
        Ok(())
    }

    /// Write audio chunk to this track. Applies gain.
    pub fn write(&mut self, chunk: &AudioChunk) -> Result<(), String> {
        self.ensure_writer(chunk)?;
        let writer = self.writer.as_mut().ok_or("Writer not initialized")?;

        for &sample in &chunk.samples {
            let gained = (sample * self.gain).clamp(-1.0, 1.0);
            writer
                .write_sample(gained)
                .map_err(|e| format!("Write sample: {e}"))?;
            self.sample_count += 1;
        }
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
                "Track finalized: {} ({} samples)",
                path.display(),
                self.sample_count
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
    tracks: Vec<(String, TrackWriter)>,
    master: Option<TrackWriter>,
}

impl MultiTrackRecorder {
    pub fn new(session_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(session_dir).map_err(|e| format!("Create session dir: {e}"))?;

        Ok(Self {
            session_dir: session_dir.to_path_buf(),
            tracks: Vec::new(),
            master: None,
        })
    }

    /// Add a named track (e.g. "mic", "system").
    pub fn add_track(&mut self, name: &str) -> Result<(), String> {
        let path = self.session_dir.join(format!("{name}.wav"));
        let writer = TrackWriter::new(path);
        self.tracks.push((name.to_string(), writer));
        Ok(())
    }

    /// Initialize the master mix track.
    pub fn init_master(&mut self) -> Result<(), String> {
        let path = self.session_dir.join("master.wav");
        let writer = TrackWriter::new(path);
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

    #[test]
    fn track_writer_creates_and_writes() {
        let dir = std::env::temp_dir().join("gravai_test_recorder");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.wav");

        let mut tw = TrackWriter::new(path.clone());

        let chunk = AudioChunk {
            samples: vec![0.0f32; 1024],
            sample_rate: 48000,
            channels: 2,
        };
        tw.write(&chunk).unwrap();
        let finalized = tw.finalize().unwrap();
        assert!(finalized.exists());

        // Verify it's a valid WAV
        let reader = hound::WavReader::open(&finalized).unwrap();
        assert_eq!(reader.spec().sample_rate, 48000);
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().bits_per_sample, 32);
        assert_eq!(reader.spec().sample_format, SampleFormat::Float);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
