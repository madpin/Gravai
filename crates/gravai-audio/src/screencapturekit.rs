//! ScreenCaptureKit system audio capture (macOS only).
//!
//! Captures system audio from all apps or a specific app by bundle ID.

/// Check if ScreenCaptureKit is available on this system.
#[cfg(target_os = "macos")]
pub fn can_use_screencapturekit() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub fn can_use_screencapturekit() -> bool {
    false
}

/// List running apps that can be captured.
#[cfg(target_os = "macos")]
pub fn list_running_apps() -> Vec<serde_json::Value> {
    use screencapturekit::shareable_content::SCShareableContent;
    match SCShareableContent::get() {
        Ok(content) => content
            .applications()
            .iter()
            .map(|app| {
                serde_json::json!({
                    "name": app.application_name(),
                    "bundle_id": app.bundle_identifier(),
                })
            })
            .collect(),
        Err(e) => {
            tracing::warn!("Failed to list apps via SCK: {e}");
            Vec::new()
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn list_running_apps() -> Vec<serde_json::Value> {
    Vec::new()
}

/// Audio callback: receives samples, sample_rate, channels.
#[cfg(target_os = "macos")]
type AudioCallback = Box<dyn FnMut(&[f32], u32, u16) + Send>;

/// System audio capture via ScreenCaptureKit.
#[cfg(target_os = "macos")]
pub struct SystemAudioCapture {
    sample_rate: u32,
    channels: u16,
    app_bundle_id: Option<String>,
    callback: Option<AudioCallback>,
    stream: Option<screencapturekit::stream::SCStream>,
}

#[cfg(target_os = "macos")]
impl SystemAudioCapture {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        app_bundle_id: Option<String>,
        callback: impl FnMut(&[f32], u32, u16) + Send + 'static,
    ) -> Self {
        Self {
            sample_rate,
            channels,
            app_bundle_id,
            callback: Some(Box::new(callback)),
            stream: None,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        use screencapturekit::{
            cm::CMSampleBuffer,
            shareable_content::SCShareableContent,
            stream::{
                configuration::SCStreamConfiguration, content_filter::SCContentFilter,
                output_type::SCStreamOutputType, SCStream,
            },
        };

        let content = SCShareableContent::get().map_err(|e| {
            format!("SCShareableContent::get failed (missing Screen Recording permission?): {e}")
        })?;

        let displays = content.displays();
        let display = displays.first().ok_or("No display found")?;

        // Build content filter — per-app if a bundle ID is selected, otherwise all system audio.
        // Per-app audio filtering requires macOS 14+ (Sonoma). On macOS 13, audio is not filtered.
        let filter = if let Some(ref bundle_id) = self.app_bundle_id {
            let apps = content.applications();
            if let Some(target_app) = apps.iter().find(|a| a.bundle_identifier() == *bundle_id) {
                tracing::info!(
                    "Filtering system audio to app: {} (bundle: {}, pid: {})",
                    target_app.application_name(),
                    target_app.bundle_identifier(),
                    target_app.process_id(),
                );
                // Per-app filter: with_display sets up DisplayExcluding internally,
                // then with_including_applications transitions to DisplayIncludingApplications.
                // Do NOT chain with_excluding_windows before with_including_applications.
                SCContentFilter::create()
                    .with_display(display)
                    .with_including_applications(&[target_app], &[])
                    .build()
            } else {
                tracing::warn!(
                    "App '{bundle_id}' not found in running apps — capturing all system audio"
                );
                SCContentFilter::create()
                    .with_display(display)
                    .with_excluding_windows(&[])
                    .build()
            }
        } else {
            SCContentFilter::create()
                .with_display(display)
                .with_excluding_windows(&[])
                .build()
        };

        // Configure stream: request float32 audio at our desired rate.
        // SCK on macOS delivers 32-bit float, native endian (LE on ARM).
        let config = SCStreamConfiguration::default()
            .with_width(2)
            .with_height(2)
            .with_captures_audio(true)
            .with_excludes_current_process_audio(true)
            .with_sample_rate(self.sample_rate as i32)
            .with_channel_count(self.channels as i32);

        let mut stream = SCStream::new(&filter, &config);

        let callback = self.callback.take().ok_or("Callback already consumed")?;
        let callback = std::sync::Arc::new(std::sync::Mutex::new(callback));
        let sample_rate = self.sample_rate;
        let channels = self.channels;
        let logged = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        stream.add_output_handler(
            move |sample_buffer: CMSampleBuffer, output_type: SCStreamOutputType| {
                if output_type != SCStreamOutputType::Audio {
                    return;
                }

                let Some(abl) = sample_buffer.audio_buffer_list() else {
                    return;
                };

                let buffers: Vec<&screencapturekit::cm::AudioBuffer> = abl.iter().collect();
                if buffers.is_empty() {
                    return;
                }

                // Log format info once
                if !logged.load(std::sync::atomic::Ordering::Relaxed) {
                    logged.store(true, std::sync::atomic::Ordering::Relaxed);
                    let buf_info: Vec<String> = buffers
                        .iter()
                        .map(|b| format!("{}ch {}bytes", b.number_channels, b.data_byte_size()))
                        .collect();
                    tracing::info!(
                        "SCK audio: {} buffer(s) [{}], requested {}Hz {}ch",
                        buffers.len(),
                        buf_info.join(", "),
                        sample_rate,
                        channels,
                    );
                }

                if buffers.len() == 1 && buffers[0].number_channels >= 1 {
                    // Single buffer — interleaved audio (most common case)
                    let raw = buffers[0].data();
                    let f32_data = audio_bytes_to_f32(raw);
                    if !f32_data.is_empty() {
                        let actual_ch = buffers[0].number_channels as u16;
                        if let Ok(mut cb) = callback.lock() {
                            cb(&f32_data, sample_rate, actual_ch.max(channels));
                        }
                    }
                } else if buffers.len() >= 2 {
                    // Multiple buffers — non-interleaved (one buffer per channel)
                    // Interleave them into a single buffer
                    let per_ch: Vec<Vec<f32>> = buffers
                        .iter()
                        .map(|b| audio_bytes_to_f32(b.data()))
                        .collect();

                    if per_ch.iter().all(|c| !c.is_empty()) {
                        let frame_count = per_ch[0].len();
                        let ch_count = per_ch.len();
                        let mut interleaved = Vec::with_capacity(frame_count * ch_count);
                        for i in 0..frame_count {
                            for ch in &per_ch {
                                interleaved.push(if i < ch.len() { ch[i] } else { 0.0 });
                            }
                        }
                        if let Ok(mut cb) = callback.lock() {
                            cb(&interleaved, sample_rate, ch_count as u16);
                        }
                    }
                }
            },
            SCStreamOutputType::Audio,
        );

        stream
            .start_capture()
            .map_err(|e| format!("SCK start: {e}"))?;
        self.stream = Some(stream);
        tracing::info!(
            "ScreenCaptureKit audio capture started ({}Hz {}ch)",
            sample_rate,
            channels
        );
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.stop_capture();
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Convert SCK audio buffer bytes to f32 samples.
/// SCK delivers 32-bit float PCM in native endian (little-endian on Apple Silicon).
/// Uses align_to for efficiency when the buffer is properly aligned.
#[cfg(target_os = "macos")]
fn audio_bytes_to_f32(data: &[u8]) -> Vec<f32> {
    if data.len() < 4 {
        return Vec::new();
    }

    // Try zero-copy via align_to (works when data is 4-byte aligned)
    let (prefix, aligned, suffix) = unsafe { data.align_to::<f32>() };
    if prefix.is_empty() && suffix.is_empty() {
        // Data was properly aligned — use directly
        return aligned.to_vec();
    }

    // Fallback: manual native-endian f32 parsing
    if !data.len().is_multiple_of(4) {
        return Vec::new();
    }
    data.chunks_exact(4)
        .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Placeholder for non-macOS.
#[cfg(not(target_os = "macos"))]
pub struct SystemAudioCapture;

#[cfg(not(target_os = "macos"))]
impl SystemAudioCapture {
    pub fn new(
        _sample_rate: u32,
        _channels: u16,
        _app_bundle_id: Option<String>,
        _callback: impl FnMut(&[f32], u32, u16) + Send + 'static,
    ) -> Self {
        Self
    }
    pub fn start(&mut self) -> Result<(), String> {
        Err("ScreenCaptureKit is macOS only".into())
    }
    pub fn stop(&mut self) {}
}
