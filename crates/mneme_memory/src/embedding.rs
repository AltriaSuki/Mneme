use anyhow::Result;
use fastembed::{EmbeddingModel as FastEmbedModel, InitOptions, TextEmbedding};
use std::sync::Arc;

pub type Embedding = Vec<f32>;

#[derive(Clone)]
pub struct EmbeddingModel {
    model: Arc<TextEmbedding>,
}

impl EmbeddingModel {
    pub fn new() -> Result<Self> {
        // Initialize with default model (usually BAAI/bge-small-en-v1.5 or similar)
        // We can specify a model that supports Chinese well if needed,
        // but multilingual-e5-small is a good general choice for mixed usage.
        // For now let's use the default which is often BGE-Small-EN,
        // but fastembed supports "BAAI/bge-m3" or "intfloat/multilingual-e5-small".

        let mut options = InitOptions::default();
        options.model_name = FastEmbedModel::MultilingualE5Small;
        options.show_download_progress = true;

        let model = TextEmbedding::try_new(options)?;

        Ok(Self {
            model: Arc::new(model),
        })
    }

    pub fn embed(&self, text: &str) -> Result<Embedding> {
        let embeddings = self.model.embed(vec![text], None)?;
        // embed returns Vec<Embedding>, we just want the first one
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Failed to generate embedding"))
    }

    pub fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Embedding>> {
        let embeddings = self.model.embed(texts, None)?;
        Ok(embeddings)
    }
}

/// Calculate cosine similarity between two vectors
/// Returns a value between -1.0 and 1.0 (1.0 = identical direction)
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_identical_vectors() {
        let a = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_opposite_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_orthogonal_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_empty_vectors() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_scaled_vectors() {
        // Cosine similarity is scale-invariant
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![2.0, 4.0, 6.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }
}
