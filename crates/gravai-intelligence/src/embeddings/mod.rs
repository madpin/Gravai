//! Text embedding provider trait.

/// Provider trait for generating text embeddings.
pub trait EmbeddingProvider: Send + Sync {
    /// Generate an embedding vector for the given text.
    fn embed(&self, text: &str) -> Result<Vec<f32>, gravai_core::GravaiError>;

    /// The dimensionality of the embedding vectors.
    fn dimension(&self) -> usize;

    /// Provider name for logging/config.
    fn name(&self) -> &str;
}
