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

    fn start_mic(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = if self.config.audio.microphone.device_index < 0 {
            host.default_input_device()
                .ok_or("No default input device")?
        } else {
            host.input_devices()
                .map_err(|e| e.to_string())?
                .nth(self.config.audio.microphone.device_index as usize)
                .ok_or("Device index out of range")?
        };

        let dev_name = device.name().unwrap_or_default();
        let supported = device
            .default_input_config()
            .map_err(|e| format!("No supported config: {e}"))?;

        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        self.mic_sample_rate = Some(sample_rate);
        self.mic_channels = Some(channels);

        info!("Mic: {} @ {}Hz {}ch", dev_name, sample_rate, channels);

        let stream_config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Fixed(1024),
        };

        // HQ channel for recording
        let (hq_tx, hq_rx) = mpsc::channel::<AudioChunk>(512);
        self.mic_hq_rx = Some(hq_rx);

        // LQ channel for transcription (16kHz mono) — resampling happens here
        let (lq_tx, lq_rx) = mpsc::channel::<Vec<f32>>(512);
        self.mic_lq_rx = Some(lq_rx);

        let recording = self.recording.clone();
        let paused = self.paused.clone();
        let on_volume = self.on_volume.clone();
        let mut volume_sample_counter = 0u64;
        let volume_interval_samples = (sample_rate as f64 * VOLUME_INTERVAL) as u64;

        // Create resampler for this source
        let mut resampler = if sample_rate != 16000 || channels != 1 {
            Some(crate::resampler::AudioResampler::new(
                sample_rate,
                channels,
                16000,
                1,
            )?)
        } else {
            None
        };

        let stream = device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if !recording.load(Ordering::Relaxed) || paused.load(Ordering::Relaxed) {
                        return;
                    }

                    // Send HQ chunk
                    let _ = hq_tx.try_send(AudioChunk {
                        samples: data.to_vec(),
                        sample_rate,
                        channels,
                    });

                    // Send LQ (16kHz mono) chunk
                    let mono_16k = match &mut resampler {
                        Some(r) => r.process(data),
                        None => data.to_vec(),
                    };
                    if !mono_16k.is_empty() {
                        let _ = lq_tx.try_send(mono_16k);
                    }

                    // Volume callback
                    volume_sample_counter += data.len() as u64 / channels as u64;
                    if volume_sample_counter >= volume_interval_samples {
                        volume_sample_counter = 0;
                        if let Some(ref cb) = on_volume {
                            cb("microphone", rms_db(data));
                        }
                    }
                },
                move |err| {
                    error!("Mic stream error: {err}");
                },
                None,
            )
            .map_err(|e| format!("Build mic stream: {e}"))?;

        stream.play().map_err(|e| format!("Play mic: {e}"))?;
        self._mic_stream = Some(stream);
        info!("Microphone capture started");
        Ok(())
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

        // SCK captures at 48kHz stereo typically
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
