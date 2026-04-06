//! Silero VAD implementation via ONNX Runtime.
//!
//! Ported from ears-rust-api audio/vad_silero.rs.

use gravai_config::VadConfig;
use ndarray::Array2;
use ort::session::Session;
use ort::value::TensorRef;
use std::sync::Mutex;
use tracing::{info, warn};

const CHUNK_SIZE_16K: usize = 512;

pub struct SileroVad {
    session: Option<Mutex<Session>>,
    threshold: f32,
    sample_rate: i64,
    h: Array2<f32>,
    c: Array2<f32>,
}

impl SileroVad {
    pub fn new(config: &VadConfig) -> Result<Self, String> {
        let model_path = gravai_config::models_dir().join("silero_vad.onnx");

        let session = if model_path.exists() {
            match Session::builder().and_then(|mut b| b.commit_from_file(&model_path)) {
                Ok(s) => {
                    info!("Silero VAD loaded from {}", model_path.display());
                    Some(Mutex::new(s))
                }
                Err(e) => {
                    warn!("Failed to load Silero VAD: {e}");
                    None
                }
            }
        } else {
            warn!("Silero VAD model not found at {}", model_path.display());
            None
        };

        Ok(Self {
            session,
            threshold: config.silero.threshold,
            sample_rate: 16000,
            h: Array2::zeros((2, 64)),
            c: Array2::zeros((2, 64)),
        })
    }

    pub fn is_available(&self) -> bool {
        self.session.is_some()
    }

    fn process_chunk(&mut self, audio: &[f32]) -> f32 {
        if self.session.is_none() || audio.is_empty() {
            return 0.0;
        }

        let chunk_size = CHUNK_SIZE_16K;
        let mut total_prob = 0.0f32;
        let mut chunk_count = 0;

        // Process in chunk_size windows
        let mut offset = 0;
        while offset < audio.len() {
            let end = (offset + chunk_size).min(audio.len());
            let chunk = &audio[offset..end];

            if chunk.len() < chunk_size {
                // Pad short chunk
                let mut padded = chunk.to_vec();
                padded.resize(chunk_size, 0.0);
                total_prob += self.run_inference(&padded);
            } else {
                total_prob += self.run_inference(chunk);
            }
            chunk_count += 1;
            offset += chunk_size;
        }

        if chunk_count == 0 {
            0.0
        } else {
            total_prob / chunk_count as f32
        }
    }

    fn run_inference(&mut self, chunk: &[f32]) -> f32 {
        let mut session_guard = match &self.session {
            Some(s) => match s.lock() {
                Ok(g) => g,
                Err(_) => return 0.0,
            },
            None => return 0.0,
        };

        // Prepare inputs as ort Tensors
        let input_array = ndarray::Array2::from_shape_vec((1, chunk.len()), chunk.to_vec())
            .unwrap_or_else(|_| ndarray::Array2::zeros((1, chunk.len())));

        let sr_array = ndarray::Array1::from_vec(vec![self.sample_rate]);

        // Use (shape, slice) tuple form for TensorArrayData
        let input_shape: Vec<i64> = input_array.shape().iter().map(|&d| d as i64).collect();
        let sr_shape: Vec<i64> = sr_array.shape().iter().map(|&d| d as i64).collect();
        let h_shape: Vec<i64> = self.h.shape().iter().map(|&d| d as i64).collect();
        let c_shape: Vec<i64> = self.c.shape().iter().map(|&d| d as i64).collect();

        let input_tensor =
            match TensorRef::<f32>::from_array_view((input_shape, input_array.as_slice().unwrap()))
            {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to create input tensor: {e}");
                    return 0.0;
                }
            };

        let sr_tensor =
            match TensorRef::<i64>::from_array_view((sr_shape, sr_array.as_slice().unwrap())) {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to create sr tensor: {e}");
                    return 0.0;
                }
            };

        let h_clone = self.h.clone();
        let c_clone = self.c.clone();

        let h_tensor =
            match TensorRef::<f32>::from_array_view((h_shape, h_clone.as_slice().unwrap())) {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to create h tensor: {e}");
                    return 0.0;
                }
            };

        let c_tensor =
            match TensorRef::<f32>::from_array_view((c_shape, c_clone.as_slice().unwrap())) {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to create c tensor: {e}");
                    return 0.0;
                }
            };

        let result = session_guard.run(ort::inputs![
            "input" => input_tensor,
            "sr" => sr_tensor,
            "h" => h_tensor,
            "c" => c_tensor,
        ]);

        match result {
            Ok(outputs) => {
                // Extract probability - try_extract_tensor returns (&Shape, &[f32])
                let prob = outputs["output"]
                    .try_extract_tensor::<f32>()
                    .ok()
                    .and_then(|(_shape, data)| data.first().copied())
                    .unwrap_or(0.0);

                // Update hidden states
                if let Ok((_shape, hn_data)) = outputs["hn"].try_extract_tensor::<f32>() {
                    if let Ok(arr) =
                        ndarray::Array::from_shape_vec(ndarray::IxDyn(&[2, 64]), hn_data.to_vec())
                    {
                        if let Ok(arr2) = arr.into_dimensionality::<ndarray::Ix2>() {
                            self.h = arr2;
                        }
                    }
                }
                if let Ok((_shape, cn_data)) = outputs["cn"].try_extract_tensor::<f32>() {
                    if let Ok(arr) =
                        ndarray::Array::from_shape_vec(ndarray::IxDyn(&[2, 64]), cn_data.to_vec())
                    {
                        if let Ok(arr2) = arr.into_dimensionality::<ndarray::Ix2>() {
                            self.c = arr2;
                        }
                    }
                }

                prob
            }
            Err(e) => {
                warn!("Silero inference error: {e}");
                0.0
            }
        }
    }
}

impl super::VadProvider for SileroVad {
    fn is_speech(&mut self, audio_16khz: &[f32]) -> bool {
        self.process_chunk(audio_16khz) >= self.threshold
    }

    fn reset(&mut self) {
        self.h = Array2::zeros((2, 64));
        self.c = Array2::zeros((2, 64));
    }

    fn name(&self) -> &str {
        "silero"
    }
}
