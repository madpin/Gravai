//! Kaldi-compatible filterbank used by the WeSpeaker embedding model.
//!
//! Matches sherpa-onnx's speaker-embedding front end: 16 kHz, 25 ms Povey
//! window, 10 ms shift, `snip_edges` off (reflect the edges), 80 mel bins
//! from 20 Hz to Nyquist−400 Hz, log energy, then mean subtraction over time.

use std::sync::OnceLock;

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

const SAMPLE_RATE: usize = 16_000;
const FRAME_SHIFT: usize = 160;
const FRAME_LENGTH: usize = 400;
const FFT_SIZE: usize = 512;
const NUM_MELS: usize = 80;

fn povey_window() -> &'static [f32] {
    static WINDOW: OnceLock<Vec<f32>> = OnceLock::new();
    WINDOW.get_or_init(|| {
        let n = FRAME_LENGTH;
        let a = std::f64::consts::TAU / (n - 1) as f64;
        (0..n)
            .map(|i| {
                let hann = 0.5 - 0.5 * (a * i as f64).cos();
                hann.powf(0.85) as f32
            })
            .collect()
    })
}

fn mel_filters() -> &'static [Vec<(usize, f32)>] {
    static FILTERS: OnceLock<Vec<Vec<(usize, f32)>>> = OnceLock::new();
    FILTERS.get_or_init(|| {
        let sample_freq = SAMPLE_RATE as f32;
        let num_fft_bins = FFT_SIZE / 2;
        let nyquist = 0.5 * sample_freq;
        let low = 20.0;
        let high = nyquist - 400.0;
        let mel_low = mel_scale(low);
        let mel_high = mel_scale(high);
        let mel_delta = (mel_high - mel_low) / (NUM_MELS + 1) as f32;
        let fft_bin_width = sample_freq / FFT_SIZE as f32;
        let mut bins = Vec::with_capacity(NUM_MELS);
        for bin in 0..NUM_MELS {
            let left = mel_low + bin as f32 * mel_delta;
            let center = mel_low + (bin + 1) as f32 * mel_delta;
            let right = mel_low + (bin + 2) as f32 * mel_delta;
            let mut weights = Vec::new();
            for i in 0..num_fft_bins {
                let mel = mel_scale(fft_bin_width * i as f32);
                if mel > left && mel < right {
                    let weight = if mel <= center {
                        (mel - left) / (center - left)
                    } else {
                        (right - mel) / (right - center)
                    };
                    weights.push((i, weight));
                }
            }
            bins.push(weights);
        }
        bins
    })
}

fn mel_scale(hz: f32) -> f32 {
    1127.0 * (1.0 + hz / 700.0).ln()
}

fn num_frames(num_samples: usize) -> usize {
    if num_samples == 0 {
        0
    } else {
        (num_samples + FRAME_SHIFT / 2) / FRAME_SHIFT
    }
}

fn reflect(mut index: isize, len: isize) -> usize {
    if len <= 0 {
        return 0;
    }
    while index < 0 || index >= len {
        if index < 0 {
            index = -index - 1;
        } else {
            index = 2 * len - 1 - index;
        }
    }
    index as usize
}

fn frame_samples(audio: &[f32], frame: usize) -> [f32; FFT_SIZE] {
    let wave_len = audio.len() as isize;
    let start = (FRAME_SHIFT * frame + FRAME_SHIFT / 2) as isize - (FRAME_LENGTH as isize) / 2;
    let mut windowed = [0.0f32; FRAME_LENGTH];
    for (i, sample) in windowed.iter_mut().enumerate() {
        *sample = audio[reflect(start + i as isize, wave_len)];
    }

    let mean = windowed.iter().sum::<f32>() / FRAME_LENGTH as f32;
    for sample in &mut windowed {
        *sample -= mean;
    }
    for i in (1..FRAME_LENGTH).rev() {
        windowed[i] -= 0.97 * windowed[i - 1];
    }
    windowed[0] -= 0.97 * windowed[0];

    let povey = povey_window();
    let mut padded = [0.0f32; FFT_SIZE];
    for i in 0..FRAME_LENGTH {
        padded[i] = windowed[i] * povey[i];
    }
    padded
}

/// 80-dim log-mel frames, mean-subtracted across time. Empty when `audio` is empty.
pub(crate) fn log_mel(audio: &[f32]) -> Vec<f32> {
    let frames = num_frames(audio.len());
    if frames == 0 {
        return Vec::new();
    }
    let filters = mel_filters();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut features = Vec::with_capacity(frames * NUM_MELS);
    let eps = f32::EPSILON;
    for frame in 0..frames {
        let samples = frame_samples(audio, frame);
        let mut spectrum: Vec<Complex<f32>> = samples
            .iter()
            .copied()
            .map(|sample| Complex::new(sample, 0.0))
            .collect();
        fft.process(&mut spectrum);
        let mut energy = [0.0f32; NUM_MELS];
        for (bin, weights) in filters.iter().enumerate() {
            let mut sum = 0.0f32;
            for &(fft_bin, weight) in weights {
                sum += weight * spectrum[fft_bin].norm_sqr();
            }
            energy[bin] = sum.max(eps).ln();
        }
        features.extend_from_slice(&energy);
    }

    let mut mean = [0.0f32; NUM_MELS];
    for frame in features.chunks(NUM_MELS) {
        for (acc, value) in mean.iter_mut().zip(frame) {
            *acc += *value;
        }
    }
    let n = frames as f32;
    for value in &mut mean {
        *value /= n;
    }
    for frame in features.chunks_mut(NUM_MELS) {
        for (value, avg) in frame.iter_mut().zip(mean) {
            *value -= avg;
        }
    }
    features
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_second_of_silence_is_one_hundred_zero_frames() {
        let features = log_mel(&vec![0.0; SAMPLE_RATE]);
        assert_eq!(features.len(), 100 * NUM_MELS);
        assert!(features.iter().all(|value| value.abs() < 1e-4));
    }

    #[test]
    fn a_tone_is_mean_centered_and_not_flat() {
        let audio: Vec<f32> = (0..SAMPLE_RATE)
            .map(|i| (2.0 * std::f32::consts::PI * 220.0 * i as f32 / SAMPLE_RATE as f32).sin())
            .collect();
        let features = log_mel(&audio);
        assert_eq!(features.len(), 100 * NUM_MELS);
        let sum: f32 = features.iter().sum();
        assert!(sum.abs() < 1e-2, "mean subtraction left {sum}");
        let energy: f32 = features.iter().map(|value| value * value).sum();
        assert!(energy > 1.0, "a tone should not collapse to silence");
    }
}
