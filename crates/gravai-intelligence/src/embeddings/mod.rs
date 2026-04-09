//! Text embedding providers — generate vector representations for semantic search.
//!
//! Available providers:
//!   - `BagOfWordsEmbedder`     — hash-based, no download, fast (default fallback)
//!   - `OnnxEmbeddingProvider`  — neural ONNX models for high-quality semantic search
//!
//! Use `create_embedder_from_config` to select the provider based on user config.
//! Use `create_embedder` for the built-in bag-of-words fallback.

mod onnx_embedder;
pub use onnx_embedder::OnnxEmbeddingProvider;

/// Provider trait for generating text embeddings.
pub trait EmbeddingProvider: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>, gravai_core::GravaiError>;
    fn dimension(&self) -> usize;
    fn name(&self) -> &str;
}

/// Simple bag-of-words embedding provider (no external model needed).
/// Produces deterministic 384-dim vectors using hash-based word features.
pub struct BagOfWordsEmbedder {
    dim: usize,
}

impl BagOfWordsEmbedder {
    pub fn new() -> Self {
        Self { dim: 384 }
    }
}

impl Default for BagOfWordsEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddingProvider for BagOfWordsEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, gravai_core::GravaiError> {
        let mut vec = vec![0.0f32; self.dim];
        let lower = text.to_lowercase();
        let words: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
            .collect();

        if words.is_empty() {
            return Ok(vec);
        }

        // Hash each word to a position in the vector and increment
        for word in &words {
            let hash = simple_hash(word);
            let idx = (hash as usize) % self.dim;
            vec[idx] += 1.0;
            // Also add bigram features
            let idx2 = ((hash.wrapping_mul(31)) as usize) % self.dim;
            vec[idx2] += 0.5;
        }

        // L2 normalize
        let mag: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if mag > 0.0 {
            for v in &mut vec {
                *v /= mag;
            }
        }

        Ok(vec)
    }

    fn dimension(&self) -> usize {
        self.dim
    }

    fn name(&self) -> &str {
        "bag-of-words"
    }
}

fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for b in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u64);
    }
    hash
}

/// Create an embedding provider based on user configuration.
///
/// Falls back to `BagOfWordsEmbedder` if the requested ONNX model files
/// are not yet downloaded.
pub fn create_embedder_from_config(
    config: &gravai_config::EmbeddingConfig,
) -> Box<dyn EmbeddingProvider> {
    match config.model.as_str() {
        "all-minilm" | "gemma-embed" | "bge-m3" => {
            match OnnxEmbeddingProvider::try_load(&config.model) {
                Some(p) => {
                    tracing::info!("Using ONNX embedding model: {}", config.model);
                    Box::new(p)
                }
                None => {
                    tracing::warn!(
                        "Embedding model '{}' not downloaded; falling back to bag-of-words. \
                        Download it from the Models tab.",
                        config.model
                    );
                    Box::new(BagOfWordsEmbedder::new())
                }
            }
        }
        _ => Box::new(BagOfWordsEmbedder::new()),
    }
}

/// Create the built-in bag-of-words embedding provider (no download required).
pub fn create_embedder() -> Box<dyn EmbeddingProvider> {
    Box::new(BagOfWordsEmbedder::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_produces_correct_dimension() {
        let e = BagOfWordsEmbedder::new();
        let vec = e.embed("hello world test").unwrap();
        assert_eq!(vec.len(), 384);
    }

    #[test]
    fn similar_texts_have_high_similarity() {
        let e = BagOfWordsEmbedder::new();
        let v1 = e.embed("discuss the AWS migration plan").unwrap();
        let v2 = e.embed("AWS migration plan discussion").unwrap();
        let v3 = e.embed("what is for lunch today").unwrap();

        let sim_12: f32 = v1.iter().zip(&v2).map(|(a, b)| a * b).sum();
        let sim_13: f32 = v1.iter().zip(&v3).map(|(a, b)| a * b).sum();

        assert!(
            sim_12 > sim_13,
            "Similar texts should have higher similarity"
        );
    }
}
