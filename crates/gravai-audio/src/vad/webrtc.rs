//! WebRTC VAD implementation.
//!
//! Ported from ears-rust-api audio/vad_webrtc.rs.

use gravai_config::VadConfig;
use webrtc_vad::Vad;

const FRAME_DURATION_MS: usize = 30;

/// Wrapper to make Vad Send-safe.
/// Safety: Vad contains *mut Fvad which is a C pointer that is only accessed
/// through &mut self methods, so it is safe to send between threads.
struct SendVad(Vad);
unsafe impl Send for SendVad {}

pub struct WebrtcVad {
    vad: SendVad,
    frame_size: usize,
}

impl WebrtcVad {
    pub fn new(config: &VadConfig) -> Result<Self, String> {
        let sample_rate = 16000u32;
        let mut vad = Vad::new(sample_rate as i32).map_err(|_| "Failed to create WebRTC VAD")?;

        let mode = match config.webrtc.aggressiveness {
            0 => webrtc_vad::VadMode::Quality,
            1 => webrtc_vad::VadMode::LowBitrate,
            2 => webrtc_vad::VadMode::Aggressive,
            _ => webrtc_vad::VadMode::VeryAggressive,
        };
        vad.fvad_set_mode(mode)
            .map_err(|_| "Failed to set VAD mode")?;

        let frame_size = (sample_rate as usize * FRAME_DURATION_MS) / 1000;

        Ok(Self {
            vad: SendVad(vad),
            frame_size,
        })
    }

    /// Process a chunk and return speech probability (0.0 to 1.0).
    fn process_chunk(&mut self, audio: &[f32]) -> f32 {
        let i16_data = to_int16(audio);

        if i16_data.is_empty() {
            return 0.0;
        }

        let mut speech_frames = 0;
        let mut total_frames = 0;

        for frame in i16_data.chunks(self.frame_size) {
            if frame.len() < self.frame_size {
                // Pad short frame
                let mut padded = frame.to_vec();
                padded.resize(self.frame_size, 0);
                if self.vad.0.is_voice_segment(&padded).unwrap_or(false) {
                    speech_frames += 1;
                }
            } else if self.vad.0.is_voice_segment(frame).unwrap_or(false) {
                speech_frames += 1;
            }
            total_frames += 1;
        }

        if total_frames == 0 {
            0.0
        } else {
            speech_frames as f32 / total_frames as f32
        }
    }
}

impl super::VadProvider for WebrtcVad {
    fn is_speech(&mut self, audio_16khz: &[f32]) -> bool {
        self.process_chunk(audio_16khz) >= 0.5
    }

    fn reset(&mut self) {
        // WebRTC VAD is stateless between frames
    }

    fn name(&self) -> &str {
        "webrtc"
    }
}

/// Convert f32 samples to i16.
fn to_int16(data: &[f32]) -> Vec<i16> {
    data.iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vad::VadProvider;

    #[test]
    fn webrtc_vad_creates() {
        let config = VadConfig::default();
        let vad = WebrtcVad::new(&config);
        assert!(vad.is_ok());
    }

    #[test]
    fn silence_is_not_speech() {
        let config = VadConfig::default();
        let mut vad = WebrtcVad::new(&config).unwrap();
        let silence = vec![0.0f32; 480]; // 30ms at 16kHz
        assert!(!vad.is_speech(&silence));
    }
}
