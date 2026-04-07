//! Silence trimming — detect and remove silent segments from audio.
//!
//! Non-destructive: original file is always preserved.
//! The trimmed output is written to a new file.

use std::path::Path;
use tracing::info;

/// A detected silent region in the audio.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SilentRegion {
    pub start_ms: u64,
    pub end_ms: u64,
    pub duration_ms: u64,
}

/// Detect silent regions in a WAV file.
/// `threshold_db` — RMS level below which audio is considered silence (e.g. -40.0).
/// `min_duration_ms` — minimum silence duration to flag (e.g. 3000 for 3 seconds).
pub fn detect_silence(
    wav_path: &Path,
    threshold_db: f64,
    min_duration_ms: u64,
) -> Result<Vec<SilentRegion>, String> {
    let reader = hound::WavReader::open(wav_path).map_err(|e| format!("Open WAV: {e}"))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as u32;

    // Read all samples as f32
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect(),
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_val = (1i64 << (bits - 1)) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max_val)
                .collect()
        }
    };

    let window_ms = 100u64; // 100ms analysis windows
    let window_samples = (sample_rate as u64 * window_ms / 1000 * channels as u64) as usize;
    let threshold_linear = 10.0f64.powf(threshold_db / 20.0);

    let mut regions = Vec::new();
    let mut silence_start: Option<u64> = None;

    for (i, chunk) in samples.chunks(window_samples).enumerate() {
        let rms: f64 = (chunk.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>()
            / chunk.len().max(1) as f64)
            .sqrt();
        let time_ms = i as u64 * window_ms;

        if rms < threshold_linear {
            if silence_start.is_none() {
                silence_start = Some(time_ms);
            }
        } else if let Some(start) = silence_start.take() {
            let duration = time_ms - start;
            if duration >= min_duration_ms {
                regions.push(SilentRegion {
                    start_ms: start,
                    end_ms: time_ms,
                    duration_ms: duration,
                });
            }
        }
    }

    // Handle trailing silence
    if let Some(start) = silence_start {
        let total_ms = (samples.len() as u64 * 1000) / (sample_rate as u64 * channels as u64);
        let duration = total_ms - start;
        if duration >= min_duration_ms {
            regions.push(SilentRegion {
                start_ms: start,
                end_ms: total_ms,
                duration_ms: duration,
            });
        }
    }

    info!(
        "Detected {} silent regions in {} (threshold: {}dB, min: {}ms)",
        regions.len(),
        wav_path.display(),
        threshold_db,
        min_duration_ms,
    );
    Ok(regions)
}

/// Trim silent regions from a WAV file, writing the result to `output_path`.
/// Original file is never modified.
pub fn trim_silence(
    wav_path: &Path,
    output_path: &Path,
    regions: &[SilentRegion],
) -> Result<(), String> {
    let reader = hound::WavReader::open(wav_path).map_err(|e| format!("Open WAV: {e}"))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as u32;

    let samples: Vec<i32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .into_samples::<i32>()
            .filter_map(|s| s.ok())
            .collect(),
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .map(|s| (s * 8_388_607.0) as i32)
            .collect(),
    };

    let mut writer =
        hound::WavWriter::create(output_path, spec).map_err(|e| format!("Create WAV: {e}"))?;

    let samples_per_ms = (sample_rate * channels) / 1000;

    // Build a set of sample ranges to skip
    let skip_ranges: Vec<(usize, usize)> = regions
        .iter()
        .map(|r| {
            let start = (r.start_ms as usize) * samples_per_ms as usize;
            let end = (r.end_ms as usize) * samples_per_ms as usize;
            (start, end.min(samples.len()))
        })
        .collect();

    let mut pos = 0;
    for (skip_start, skip_end) in &skip_ranges {
        // Write samples before the skip region
        for i in pos..*skip_start {
            if i < samples.len() {
                writer
                    .write_sample(samples[i])
                    .map_err(|e| format!("Write: {e}"))?;
            }
        }
        pos = *skip_end;
    }
    // Write remaining samples after last skip
    for sample in &samples[pos..] {
        writer
            .write_sample(*sample)
            .map_err(|e| format!("Write: {e}"))?;
    }

    writer.finalize().map_err(|e| format!("Finalize: {e}"))?;

    let original_ms = (samples.len() as u64 * 1000) / (sample_rate as u64 * channels as u64);
    let trimmed_ms: u64 = regions.iter().map(|r| r.duration_ms).sum();
    info!(
        "Trimmed {}ms of silence from {}ms → {}",
        trimmed_ms,
        original_ms,
        output_path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_region_serializes() {
        let r = SilentRegion {
            start_ms: 1000,
            end_ms: 4000,
            duration_ms: 3000,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("3000"));
    }
}
