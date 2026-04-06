//! Voice Activity Detection provider trait and implementations.

pub mod silero;
pub mod webrtc;

/// A VAD provider detects whether an audio frame contains speech.
pub trait VadProvider: Send {
    /// Returns true if the audio chunk (16kHz mono f32) contains speech.
    fn is_speech(&mut self, audio_16khz: &[f32]) -> bool;

    /// Reset internal state (e.g., between utterances or sessions).
    fn reset(&mut self);

    /// Provider name for logging/config.
    fn name(&self) -> &str;
}

/// Create a VAD provider based on config.
pub fn create_vad(config: &gravai_config::VadConfig) -> Result<Box<dyn VadProvider>, String> {
    match config.engine.as_str() {
        "silero" => {
            let vad = silero::SileroVad::new(config)?;
            Ok(Box::new(vad))
        }
        _ => {
            // Default to WebRTC
            let vad = webrtc::WebrtcVad::new(config)?;
            Ok(Box::new(vad))
        }
    }
}
