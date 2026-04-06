//! Audio mixer: combines multiple sources into a single stream.
//!
//! Applies per-source gain and panning, then sums into master.

use crate::capture::AudioChunk;

/// Mix a chunk with gain and pan into an accumulator buffer.
/// Accumulator must be pre-allocated to the correct size.
pub fn mix_into(acc: &mut [f32], chunk: &AudioChunk, gain: f32, pan: f32) {
    let channels = chunk.channels as usize;
    if channels == 0 {
        return;
    }

    // Compute left/right gain from pan
    let left_gain = gain * (1.0 - pan.max(0.0));
    let right_gain = gain * (1.0 + pan.min(0.0));

    for (i, sample) in chunk.samples.iter().enumerate() {
        if i >= acc.len() {
            break;
        }
        if channels >= 2 {
            // Stereo: apply L/R gain
            let ch = i % channels;
            let g = if ch == 0 { left_gain } else { right_gain };
            acc[i] += sample * g;
        } else {
            // Mono: apply center gain
            acc[i] += sample * gain;
        }
    }
}

/// Normalize a buffer to prevent clipping. Returns the peak value.
pub fn normalize(buffer: &mut [f32]) -> f32 {
    let peak = buffer.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    if peak > 1.0 {
        let scale = 1.0 / peak;
        for s in buffer.iter_mut() {
            *s *= scale;
        }
    }
    peak
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_into_adds_samples() {
        let mut acc = vec![0.0f32; 4];
        let chunk = AudioChunk {
            samples: vec![0.5, 0.5, 0.5, 0.5],
            sample_rate: 48000,
            channels: 2,
        };
        mix_into(&mut acc, &chunk, 1.0, 0.0);
        assert!(acc.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn normalize_prevents_clipping() {
        let mut buf = vec![2.0, -1.5, 0.5];
        let peak = normalize(&mut buf);
        assert!((peak - 2.0).abs() < 1e-6);
        assert!(buf[0] <= 1.0);
    }
}
