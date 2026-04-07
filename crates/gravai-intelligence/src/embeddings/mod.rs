//! Text embedding provider — generates vector representations for semantic search.
//!
//! Uses a simple TF-IDF-like bag-of-words approach as the default provider.
//! Can be upgraded to all-MiniLM-L6-v2 via ORT when the model is available.

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

/// Create the default embedding provider.
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
