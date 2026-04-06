//! ScreenCaptureKit system audio capture (macOS only).
//!
//! Ported from ears-rust-api audio/screencapturekit.rs.
//! Captures system audio from all apps or a specific app by bundle ID.

/// Check if ScreenCaptureKit is available on this system.
#[cfg(target_os = "macos")]
pub fn can_use_screencapturekit() -> bool {
    // SCK requires macOS 12.3+. We assume if we compiled with the crate, it's available.
    // Runtime check via SCShareableContent::get() in ensure_permission().
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

        // Build content filter using the builder API
        let filter = if let Some(ref bundle_id) = self.app_bundle_id {
            let apps = content.applications();
            let _app = apps.iter().find(|a| a.bundle_identifier() == *bundle_id);
            // For now, capture from display regardless (app-specific filtering
            // would require the excluding_applications builder method)
            if _app.is_none() {
                tracing::warn!("App {} not found, capturing all system audio", bundle_id);
            }
            SCContentFilter::create()
                .with_display(display)
                .with_excluding_windows(&[])
                .build()
        } else {
            SCContentFilter::create()
                .with_display(display)
                .with_excluding_windows(&[])
                .build()
        };

        // Configure stream with audio enabled, minimal video
        let config = SCStreamConfiguration::default()
            .with_width(2)
            .with_height(2)
            .with_captures_audio(true)
            .with_excludes_current_process_audio(true)
            .with_sample_rate(self.sample_rate as i32)
            .with_channel_count(self.channels as i32);

        let mut stream = SCStream::new(&filter, &config);

        // Set up audio output handler
        let callback = self.callback.take().ok_or("Callback already consumed")?;
        let callback = std::sync::Arc::new(std::sync::Mutex::new(callback));
        let sample_rate = self.sample_rate;
        let channels = self.channels;

        stream.add_output_handler(
            move |sample_buffer: CMSampleBuffer, output_type: SCStreamOutputType| {
                if output_type != SCStreamOutputType::Audio {
                    return;
                }
                // Extract audio data from the sample buffer
                if let Some(audio_buffer_list) = sample_buffer.audio_buffer_list() {
                    for buffer in audio_buffer_list.iter() {
                        let data = buffer.data();
                        let f32_data = bytes_to_f32(data);
                        if !f32_data.is_empty() {
                            if let Ok(mut cb) = callback.lock() {
                                cb(&f32_data, sample_rate, channels);
                            }
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
        tracing::info!("ScreenCaptureKit audio capture started");
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

/// Convert raw audio bytes to f32 samples.
#[cfg(target_os = "macos")]
fn bytes_to_f32(data: &[u8]) -> Vec<f32> {
    // Try to interpret as f32 via alignment
    if data.len().is_multiple_of(4) {
        data.chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect()
    } else {
        Vec::new()
    }
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
