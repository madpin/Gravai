//! ONNX-based sentence embedding provider.
//!
//! Supports:
//!   - all-MiniLM-L6-v2  (model id "all-minilm",   ~22 MB,  dim 384)
//!   - nomic-embed-text   (model id "gemma-embed", ~274 MB,  dim 768)
//!   - BGE-M3             (model id "bge-m3",      ~572 MB,  dim 1024)
//!
//! Models are loaded from `~/.gravai/models/embeddings/<model-id>/`.
//! Required files: `model.onnx`, `tokenizer.json`.

use super::EmbeddingProvider;
use gravai_core::GravaiError;
use ort::session::Session;
use ort::value::Tensor;
use std::sync::Mutex;
use tokenizers::Tokenizer;

/// Max tokens to feed into the model (safety truncation).
const MAX_SEQ_LEN: usize = 512;

pub struct OnnxEmbeddingProvider {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    dim: usize,
    model_id: String,
    /// Whether this model's ONNX graph expects `token_type_ids` as an input.
    needs_token_type_ids: bool,
}

impl OnnxEmbeddingProvider {
    /// Try to load an embedding model by its short ID.
    /// Returns `None` gracefully if model files are missing.
    pub fn try_load(model_id: &str) -> Option<Self> {
        let model_dir = gravai_config::models_dir()
            .join("embeddings")
            .join(model_id);
        let model_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !model_path.exists() || !tokenizer_path.exists() {
            tracing::debug!(
                "Embedding model '{}' not found at {} — falling back to bag-of-words",
                model_id,
                model_dir.display()
            );
            return None;
        }

        let session = match Session::builder().and_then(|mut b| b.commit_from_file(&model_path)) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to load embedding ONNX model '{}': {e}", model_id);
                return None;
            }
        };

        let needs_token_type_ids = session
            .inputs()
            .iter()
            .any(|i| i.name() == "token_type_ids");

        let tokenizer = match Tokenizer::from_file(&tokenizer_path) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Failed to load embedding tokenizer '{}': {e}", model_id);
                return None;
            }
        };

        let dim = embedding_dim(model_id);
        tracing::info!(
            "Embedding model '{}' loaded (dim={}, token_type_ids={})",
            model_id,
            dim,
            needs_token_type_ids
        );

        Some(Self {
            session: Mutex::new(session),
            tokenizer,
            dim,
            model_id: model_id.to_string(),
            needs_token_type_ids,
        })
    }
}

fn embedding_dim(model_id: &str) -> usize {
    match model_id {
        "all-minilm" => 384,
        "gemma-embed" => 768,
        "bge-m3" => 1024,
        _ => 384,
    }
}

impl EmbeddingProvider for OnnxEmbeddingProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>, GravaiError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| GravaiError::Model(format!("Tokenizer error: {e}")))?;

        let len = encoding.get_ids().len().min(MAX_SEQ_LEN);

        let input_ids: Vec<i64> = encoding.get_ids()[..len]
            .iter()
            .map(|&id| id as i64)
            .collect();
        let attention_mask: Vec<i64> = encoding.get_attention_mask()[..len]
            .iter()
            .map(|&m| m as i64)
            .collect();

        let ids_tensor = Tensor::from_array((vec![1i64, len as i64], input_ids))
            .map_err(|e| GravaiError::Model(format!("Tensor error: {e}")))?;
        let mask_tensor = Tensor::from_array((vec![1i64, len as i64], attention_mask.clone()))
            .map_err(|e| GravaiError::Model(format!("Tensor error: {e}")))?;

        let mut session = self
            .session
            .lock()
            .map_err(|_| GravaiError::Model("Session lock poisoned".into()))?;

        let outputs = if self.needs_token_type_ids {
            let type_ids: Vec<i64> = vec![0i64; len];
            let type_tensor = Tensor::from_array((vec![1i64, len as i64], type_ids))
                .map_err(|e| GravaiError::Model(format!("Tensor error: {e}")))?;
            session
                .run(ort::inputs![
                    "input_ids" => ids_tensor,
                    "attention_mask" => mask_tensor,
                    "token_type_ids" => type_tensor,
                ])
                .map_err(|e| GravaiError::Model(format!("ONNX inference: {e}")))?
        } else {
            session
                .run(ort::inputs![
                    "input_ids" => ids_tensor,
                    "attention_mask" => mask_tensor,
                ])
                .map_err(|e| GravaiError::Model(format!("ONNX inference: {e}")))?
        };

        // Some models output `sentence_embedding` directly (already pooled).
        if let Some(embed_out) = outputs.get("sentence_embedding") {
            if let Ok((_shape, data)) = embed_out.try_extract_tensor::<f32>() {
                let flat: Vec<f32> = data.to_vec();
                let vec: Vec<f32> = flat.into_iter().take(self.dim).collect();
                return Ok(l2_normalize(vec));
            }
        }

        // Otherwise use `last_hidden_state` with attention-mask-weighted mean pooling.
        let first_output = outputs
            .iter()
            .next()
            .ok_or_else(|| GravaiError::Model("No outputs from embedding model".into()))?;
        let (_name, tensor) = first_output;
        let (shape, data) = tensor
            .try_extract_tensor::<f32>()
            .map_err(|e| GravaiError::Model(format!("Extract tensor: {e}")))?;

        // shape: [1, seq_len, hidden_dim]
        let seq_len = *shape.get(1).unwrap_or(&0) as usize;
        let hidden_dim = *shape.get(2).unwrap_or(&1) as usize;
        let flat: Vec<f32> = data.to_vec();

        let mask_f: Vec<f32> = attention_mask.iter().map(|&m| m as f32).collect();
        let mask_sum: f32 = mask_f.iter().sum::<f32>().max(1e-9);

        let mut pooled = vec![0.0f32; hidden_dim];
        for (tok, &w) in mask_f
            .iter()
            .enumerate()
            .take(seq_len.min(len).min(mask_f.len()))
        {
            let offset = tok * hidden_dim;
            for d in 0..hidden_dim {
                pooled[d] += flat[offset + d] * w;
            }
        }
        for v in &mut pooled {
            *v /= mask_sum;
        }

        pooled.truncate(self.dim);
        Ok(l2_normalize(pooled))
    }

    fn dimension(&self) -> usize {
        self.dim
    }

    fn name(&self) -> &str {
        &self.model_id
    }
}

fn l2_normalize(mut vec: Vec<f32>) -> Vec<f32> {
    let mag: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag > 1e-9 {
        for v in &mut vec {
            *v /= mag;
        }
    }
    vec
}
