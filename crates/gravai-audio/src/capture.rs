//! Audio capture manager — cpal (microphone) + ScreenCaptureKit (system audio).
//!
//! Ported from ears-rust-api audio/capture.rs, adapted for Gravai:
//! - Captures at device native rate (typically 48kHz stereo) for recording
//! - Produces a separate 16kHz mono stream for transcription via resampler
//! - Per-source volume/gain control

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use gravai_config::AppConfig;

/// Volume update interval in seconds (10 Hz updates).
const VOLUME_INTERVAL: f64 = 0.1;

/// Audio chunk with metadata.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Callback for volume level events.
pub type VolumeCallback = Arc<dyn Fn(&str, f64) + Send + Sync>;

/// Compute RMS level in decibels from f32 samples.
pub fn rms_db(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return -100.0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum_sq / samples.len() as f64).sqrt();
    if rms < 1e-10 {
        -100.0
    } else {
        20.0 * rms.log10()
    }
}

/// Match a saved microphone name against the current input list.
pub fn find_named_input(names: &[impl AsRef<str>], wanted: &str) -> Option<usize> {
    if wanted.is_empty() {
        return None;
    }
    names.iter().position(|name| name.as_ref() == wanted)
}

fn resolve_input_device(
    host: &cpal::Host,
    mic: &gravai_config::MicrophoneConfig,
) -> Result<(cpal::Device, String), String> {
    if !mic.device_name.is_empty() {
        let wanted = mic.device_name.clone();
        let device = host
            .input_devices()
            .map_err(|e| e.to_string())?
            .find(|device| device.name().ok().as_deref() == Some(wanted.as_str()))
            .ok_or_else(|| format!("Microphone \"{wanted}\" is not connected"))?;
        return Ok((device, wanted));
    }
    if mic.device_index >= 0 {
        let index = mic.device_index as usize;
        let device = host
            .input_devices()
            .map_err(|e| e.to_string())?
            .nth(index)
            .ok_or_else(|| format!("Microphone index {index} is not connected"))?;
        let name = device.name().unwrap_or_else(|_| format!("Device {index}"));
        return Ok((device, name));
    }
    let device = host
        .default_input_device()
        .ok_or("No default input device")?;
    let name = device.name().unwrap_or_else(|_| "Default".into());
    Ok((device, name))
}

/// Change channel count without changing the sample rate.
fn remix(interleaved: &[f32], in_ch: u16, out_ch: u16) -> Vec<f32> {
    if in_ch == 0 || out_ch == 0 || in_ch == out_ch {
        return interleaved.to_vec();
    }
    let in_ch = in_ch as usize;
    let out_ch = out_ch as usize;
    let frames = interleaved.len() / in_ch;
    let mut output = Vec::with_capacity(frames * out_ch);
    for frame in 0..frames {
        let base = frame * in_ch;
        if out_ch == 1 {
            let sum: f32 = interleaved[base..base + in_ch].iter().sum();
            output.push(sum / in_ch as f32);
        } else {
            for channel in 0..out_ch {
                let src = if channel < in_ch { channel } else { 0 };
                output.push(interleaved[base + src]);
            }
        }
    }
    output
}

/// Information about an audio device.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AudioDeviceInfo {
    pub index: usize,
    pub name: String,
    pub device_type: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
}

/// Manages microphone and system audio capture.
pub struct AudioCaptureManager {
    config: AppConfig,
    recording: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,

    // High-quality channels (native rate, for recording)
    pub mic_hq_rx: Option<mpsc::Receiver<AudioChunk>>,
    pub sys_hq_rx: Option<mpsc::Receiver<AudioChunk>>,

    // Low-quality channels (16kHz mono, for transcription/VAD)
    pub mic_lq_rx: Option<mpsc::Receiver<Vec<f32>>>,
    pub sys_lq_rx: Option<mpsc::Receiver<Vec<f32>>>,

    pub on_volume: Option<VolumeCallback>,

    // Hold stream handles to keep them alive
    _mic_stream: Option<cpal::Stream>,
    _sys_capture: Option<Box<dyn std::any::Any + Send>>,

    // Kept across mic swaps so the recording and transcription tasks
    // keep reading the same channels.
    mic_hq_tx: Option<mpsc::Sender<AudioChunk>>,
    mic_lq_tx: Option<mpsc::Sender<Vec<f32>>>,
    /// Format of the first mic opened this session. Later devices are
    /// adapted to it so the WAV header stays valid.
    mic_locked_rate: Option<u32>,
    mic_locked_channels: Option<u16>,
    active_mic_name: Option<String>,
    /// Set when the device disappears. Cleared while we are swapping on purpose.
    mic_fault: Arc<AtomicBool>,
    mic_switching: Arc<AtomicBool>,

    // Track actual capture rates for resampler configuration
    pub mic_sample_rate: Option<u32>,
    pub mic_channels: Option<u16>,
    pub sys_sample_rate: Option<u32>,
    pub sys_channels: Option<u16>,
}

impl AudioCaptureManager {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            recording: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            mic_hq_rx: None,
            sys_hq_rx: None,
            mic_lq_rx: None,
            sys_lq_rx: None,
            on_volume: None,
            _mic_stream: None,
            _sys_capture: None,
            mic_hq_tx: None,
            mic_lq_tx: None,
            mic_locked_rate: None,
            mic_locked_channels: None,
            active_mic_name: None,
            mic_fault: Arc::new(AtomicBool::new(false)),
            mic_switching: Arc::new(AtomicBool::new(false)),
            mic_sample_rate: None,
            mic_channels: None,
            sys_sample_rate: None,
            sys_channels: None,
        }
    }

    pub fn set_volume_callback(&mut self, cb: VolumeCallback) {
        self.on_volume = Some(cb);
    }

    /// Start capturing audio from configured sources.
    pub fn start(&mut self) -> Result<(), gravai_core::GravaiError> {
        self.recording.store(true, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);

        if self.config.audio.microphone.enabled {
            if let Err(e) = self.start_mic() {
                error!("Failed to start microphone capture: {e}");
                return Err(gravai_core::GravaiError::Audio(format!("Microphone: {e}")));
            }
        }

        if self.config.audio.system_audio.enabled {
            if let Err(e) = self.start_sys() {
                warn!("Failed to start system audio capture: {e}");
                // Non-fatal: continue without system audio
            }
        }

        Ok(())
    }

    /// Stop all audio capture.
    pub fn stop(&mut self) {
        self.recording.store(false, Ordering::SeqCst);
        self._mic_stream = None;
        self._sys_capture = None;
        info!("Audio capture stopped");
    }

    pub fn pause(&mut self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&mut self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn active_mic_name(&self) -> Option<&str> {
        self.active_mic_name.as_deref()
    }

    /// True once since the last check if the open mic disappeared.
    pub fn take_mic_fault(&self) -> bool {
        self.mic_fault.swap(false, Ordering::Relaxed)
    }

    pub fn mark_mic_missing(&self) {
        self.mic_fault.store(true, Ordering::Relaxed);
    }

    /// Next `restart_mic` follows the system default input.
    pub fn use_default_mic(&mut self) {
        self.config.audio.microphone.device_name.clear();
        self.config.audio.microphone.device_index = -1;
    }

    pub fn pin_mic_name(&mut self, name: String) {
        self.config.audio.microphone.device_name = name;
        self.config.audio.microphone.device_index = -1;
    }

    pub fn follows_default(&self) -> bool {
        self.config.audio.microphone.device_name.is_empty()
            && self.config.audio.microphone.device_index < 0
    }

    pub fn default_input_name() -> Option<String> {
        cpal::default_host().default_input_device()?.name().ok()
    }

    /// Point the live mic stream at another input. Empty `device_name` uses
    /// the system default. Recording and transcription keep their channels.
    pub fn switch_mic(&mut self, device_name: String) -> Result<String, String> {
        self.config.audio.microphone.device_name = device_name;
        self.config.audio.microphone.device_index = -1;
        self.restart_mic()
    }

    /// Reopen the mic described by the current config.
    pub fn restart_mic(&mut self) -> Result<String, String> {
        self.mic_switching.store(true, Ordering::Relaxed);
        self._mic_stream = None;
        let result = self.open_mic_stream();
        self.mic_switching.store(false, Ordering::Relaxed);
        result
    }

    fn start_mic(&mut self) -> Result<(), String> {
        self.open_mic_stream().map(|_| ())
    }

    fn open_mic_stream(&mut self) -> Result<String, String> {
        let host = cpal::default_host();
        let (device, dev_name) = resolve_input_device(&host, &self.config.audio.microphone)?;

        let supported = device
            .default_input_config()
            .map_err(|e| format!("No supported config: {e}"))?;

        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        self.mic_sample_rate = Some(sample_rate);
        self.mic_channels = Some(channels);
        if self.mic_locked_rate.is_none() {
            self.mic_locked_rate = Some(sample_rate);
            self.mic_locked_channels = Some(channels);
        }
        let locked_rate = self.mic_locked_rate.unwrap_or(sample_rate);
        let locked_channels = self.mic_locked_channels.unwrap_or(channels);

        info!("Mic: {} @ {}Hz {}ch", dev_name, sample_rate, channels);

        let stream_config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Fixed(1024),
        };

        // Create the session channels once. A later swap reuses the senders
        // so the recorder and transcriber do not have to be restarted.
        if self.mic_hq_tx.is_none() {
            let (hq_tx, hq_rx) = mpsc::channel::<AudioChunk>(512);
            self.mic_hq_tx = Some(hq_tx);
            self.mic_hq_rx = Some(hq_rx);
        }
        if self.mic_lq_tx.is_none() {
            let (lq_tx, lq_rx) = mpsc::channel::<Vec<f32>>(512);
            self.mic_lq_tx = Some(lq_tx);
            self.mic_lq_rx = Some(lq_rx);
        }
        let hq_tx = self
            .mic_hq_tx
            .clone()
            .ok_or("Microphone recording channel is closed")?;
        let lq_tx = self
            .mic_lq_tx
            .clone()
            .ok_or("Microphone transcription channel is closed")?;

        let recording = self.recording.clone();
        let paused = self.paused.clone();
        let on_volume = self.on_volume.clone();
        let mut volume_sample_counter = 0u64;
        let volume_interval_samples = (sample_rate as f64 * VOLUME_INTERVAL) as u64;

        // Transcription is always 16 kHz mono. The recording track stays at
        // the format of the first mic so a swap does not rewrite the WAV header.
        let mut lq_resampler = if sample_rate != 16000 || channels != 1 {
            Some(crate::resampler::AudioResampler::new(
                sample_rate,
                channels,
                16000,
                1,
            )?)
        } else {
            None
        };
        let mut hq_resampler = if sample_rate != locked_rate {
            Some(crate::resampler::AudioResampler::new(
                sample_rate,
                channels,
                locked_rate,
                locked_channels,
            )?)
        } else {
            None
        };

        let fault = self.mic_fault.clone();
        let switching = self.mic_switching.clone();
        fault.store(false, Ordering::Relaxed);

        let stream = device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if !recording.load(Ordering::Relaxed) || paused.load(Ordering::Relaxed) {
                        return;
                    }

                    let hq_samples = match &mut hq_resampler {
                        Some(resampler) => resampler.process(data),
                        None => remix(data, channels, locked_channels),
                    };
                    if !hq_samples.is_empty() {
                        let _ = hq_tx.try_send(AudioChunk {
                            samples: hq_samples,
                            sample_rate: locked_rate,
                            channels: locked_channels,
                        });
                    }

                    let mono_16k = match &mut lq_resampler {
                        Some(resampler) => resampler.process(data),
                        None => data.to_vec(),
                    };
                    if !mono_16k.is_empty() {
                        let _ = lq_tx.try_send(mono_16k);
                    }

                    volume_sample_counter += data.len() as u64 / channels as u64;
                    if volume_sample_counter >= volume_interval_samples {
                        volume_sample_counter = 0;
                        if let Some(ref cb) = on_volume {
                            cb("microphone", rms_db(data));
                        }
                    }
                },
                move |err| {
                    if switching.load(Ordering::Relaxed) {
                        return;
                    }
                    error!("Mic stream error: {err}");
                    fault.store(true, Ordering::Relaxed);
                },
                None,
            )
            .map_err(|e| format!("Build mic stream: {e}"))?;

        stream.play().map_err(|e| format!("Play mic: {e}"))?;
        self._mic_stream = Some(stream);
        self.active_mic_name = Some(dev_name.clone());
        info!("Microphone capture started");
        Ok(dev_name)
    }

    fn start_sys(&mut self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            if crate::screencapturekit::can_use_screencapturekit() {
                return self.start_sys_sck();
            }
        }
        warn!("ScreenCaptureKit not available; system audio capture disabled");
        Err("ScreenCaptureKit not available".into())
    }

    #[cfg(target_os = "macos")]
    fn start_sys_sck(&mut self) -> Result<(), String> {
        use crate::screencapturekit::SystemAudioCapture;

        let app_bundle_id = if self.config.audio.system_audio.app_bundle_id.is_empty() {
            None
        } else {
            Some(self.config.audio.system_audio.app_bundle_id.clone())
        };

        let (hq_tx, hq_rx) = mpsc::channel::<AudioChunk>(512);
        self.sys_hq_rx = Some(hq_rx);

        let (lq_tx, lq_rx) = mpsc::channel::<Vec<f32>>(512);
        self.sys_lq_rx = Some(lq_rx);

        let recording = self.recording.clone();
        let paused = self.paused.clone();
        let on_volume = self.on_volume.clone();

        // SCK: request 48kHz stereo. The WAV recorder reads the actual
        // format from the first audio chunk, so even if SCK delivers at
        // a different rate, the header will match.
        let sck_sample_rate = 48000u32;
        let sck_channels = 2u16;
        self.sys_sample_rate = Some(sck_sample_rate);
        self.sys_channels = Some(sck_channels);

        let mut resampler = if sck_sample_rate != 16000 || sck_channels != 1 {
            Some(
                crate::resampler::AudioResampler::new(sck_sample_rate, sck_channels, 16000, 1)
                    .map_err(|e| format!("Resampler: {e}"))?,
            )
        } else {
            None
        };

        let mut volume_sample_counter = 0u64;
        let volume_interval_samples = (sck_sample_rate as f64 * VOLUME_INTERVAL) as u64;

        let callback = move |data: &[f32], sample_rate: u32, channels: u16| {
            if !recording.load(Ordering::Relaxed) || paused.load(Ordering::Relaxed) {
                return;
            }

            let _ = hq_tx.try_send(AudioChunk {
                samples: data.to_vec(),
                sample_rate,
                channels,
            });

            let mono_16k = match &mut resampler {
                Some(r) => r.process(data),
                None => data.to_vec(),
            };
            if !mono_16k.is_empty() {
                let _ = lq_tx.try_send(mono_16k);
            }

            volume_sample_counter += data.len() as u64 / channels.max(1) as u64;
            if volume_sample_counter >= volume_interval_samples {
                volume_sample_counter = 0;
                if let Some(ref cb) = on_volume {
                    cb("system_audio", rms_db(data));
                }
            }
        };

        let mut capture =
            SystemAudioCapture::new(sck_sample_rate, sck_channels, app_bundle_id, callback);
        capture.start().map_err(|e| format!("SCK start: {e}"))?;

        self._sys_capture = Some(Box::new(capture));
        info!("System audio capture started via ScreenCaptureKit");
        Ok(())
    }

    /// List available audio input devices.
    pub fn list_devices() -> Vec<AudioDeviceInfo> {
        let host = cpal::default_host();
        let mut devices = Vec::new();

        if let Ok(input_devices) = host.input_devices() {
            for (i, device) in input_devices.enumerate() {
                let name = device.name().unwrap_or_else(|_| format!("Device {i}"));
                let (sr, ch) = device
                    .default_input_config()
                    .map(|c| (Some(c.sample_rate().0), Some(c.channels())))
                    .unwrap_or((None, None));

                let device_type = if name.to_lowercase().contains("blackhole")
                    || name.to_lowercase().contains("loopback")
                {
                    "loopback"
                } else {
                    "microphone"
                };

                devices.push(AudioDeviceInfo {
                    index: i,
                    name,
                    device_type: device_type.into(),
                    sample_rate: sr,
                    channels: ch,
                });
            }
        }

        debug!("Found {} audio devices", devices.len());
        devices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_input_ignores_a_blank_choice() {
        assert_eq!(find_named_input(&["MacBook Pro Microphone"], ""), None);
    }

    #[test]
    fn named_input_finds_the_device_after_the_list_shifts() {
        let names = ["MacBook Pro Microphone", "Logitech BRIO"];
        assert_eq!(find_named_input(&names, "Logitech BRIO"), Some(1));
        let shifted = ["Logitech BRIO"];
        assert_eq!(find_named_input(&shifted, "Logitech BRIO"), Some(0));
        assert_eq!(find_named_input(&shifted, "MacBook Pro Microphone"), None);
    }

    #[test]
    fn remix_mono_to_stereo_duplicates_the_channel() {
        let stereo = remix(&[0.25, -0.5], 1, 2);
        assert_eq!(stereo, vec![0.25, 0.25, -0.5, -0.5]);
    }
}
