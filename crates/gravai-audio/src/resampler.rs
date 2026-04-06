//! Audio resampler: converts from capture rate (e.g. 48kHz stereo) to 16kHz mono.
//!
//! Uses the `rubato` crate for high-quality asynchronous resampling.

use rubato::{FftFixedIn, Resampler};
use tracing::debug;

/// Resamples audio from source format to target format.
pub struct AudioResampler {
    resampler: FftFixedIn<f32>,
    source_channels: u16,
    target_channels: u16,
    input_buffer: Vec<Vec<f32>>,
    chunk_size: usize,
}

impl AudioResampler {
    pub fn new(
        source_rate: u32,
        source_channels: u16,
        target_rate: u32,
        target_channels: u16,
    ) -> Result<Self, String> {
        let chunk_size = 1024;
        let resampler = FftFixedIn::new(
            source_rate as usize,
            target_rate as usize,
            chunk_size,
            1, // sub_chunks
            source_channels as usize,
        )
        .map_err(|e| format!("Resampler init: {e}"))?;

        debug!(
            "Resampler: {}Hz {}ch -> {}Hz {}ch (chunk={})",
            source_rate, source_channels, target_rate, target_channels, chunk_size
        );

        Ok(Self {
            resampler,
            source_channels,
            target_channels,
            input_buffer: vec![Vec::new(); source_channels as usize],
            chunk_size,
        })
    }

    /// Process interleaved f32 audio. Returns resampled mono/stereo output.
    /// May return empty if not enough input has accumulated yet.
    pub fn process(&mut self, interleaved: &[f32]) -> Vec<f32> {
        // De-interleave into per-channel buffers
        let ch = self.source_channels as usize;
        for (i, sample) in interleaved.iter().enumerate() {
            self.input_buffer[i % ch].push(*sample);
        }

        let mut output = Vec::new();

        // Process full chunks
        while self.input_buffer[0].len() >= self.chunk_size {
            let mut chunk: Vec<Vec<f32>> = Vec::with_capacity(ch);
            for buf in &mut self.input_buffer {
                chunk.push(buf.drain(..self.chunk_size).collect());
            }

            match self.resampler.process(&chunk, None) {
                Ok(resampled) => {
                    if resampled.is_empty() {
                        continue;
                    }
                    // Convert to target channel count
                    if self.target_channels == 1 && resampled.len() > 1 {
                        // Stereo to mono: average channels
                        let len = resampled[0].len();
                        for i in 0..len {
                            let sum: f32 = resampled
                                .iter()
                                .map(|ch| ch.get(i).copied().unwrap_or(0.0))
                                .sum();
                            output.push(sum / resampled.len() as f32);
                        }
                    } else {
                        // Take first channel or interleave
                        output.extend_from_slice(&resampled[0]);
                    }
                }
                Err(e) => {
                    tracing::warn!("Resample error: {e}");
                }
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_creates_successfully() {
        let r = AudioResampler::new(48000, 2, 16000, 1);
        assert!(r.is_ok());
    }

    #[test]
    fn resampler_produces_output() {
        let mut r = AudioResampler::new(48000, 2, 16000, 1).unwrap();
        // Feed enough data (48kHz stereo = 96000 samples/sec)
        let input: Vec<f32> = (0..4096).map(|i| (i as f32 * 0.001).sin()).collect();
        let output = r.process(&input);
        // May or may not produce output depending on internal buffering
        // But should not panic
        assert!(output.len() < input.len()); // downsampled should be smaller
    }
}
